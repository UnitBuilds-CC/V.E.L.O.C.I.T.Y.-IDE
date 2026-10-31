//! The stdio JSON-RPC face of the server.
//!
//! Three rules hold here because stdout *is* the protocol channel, not a log:
//!
//! - One line in, at most one JSON object out. A notification gets no reply, and
//!   nothing but a reply is ever written — the startup banner used to be
//!   `println!`, and clients that read the first line as a message got a syntax
//!   error or, worse, wrote it into a file as if it were a response.
//! - A refusal is not a crash. The two are different facts on the wire, in
//!   different places, with a machine-readable label on each.
//! - The loop answers a request; it never judges its own input. All the judging
//!   is in [`handle_line`], which is pure so both rules above are gradable
//!   without a terminal, a workspace, or a live tool.

use crate::errors::ToolError;
use crate::registry;
use serde_json::{json, Value};
use std::error::Error;
use std::io::{self, BufRead, Write};

/// Malformed JSON, so there is no request to answer.
pub const PARSE_ERROR: i64 = -32700;
/// Well-formed JSON whose parameters cannot describe a call.
pub const INVALID_PARAMS: i64 = -32602;
/// No such method.
pub const METHOD_NOT_FOUND: i64 = -32601;
/// The server broke. Reserved above for the handler that panicked mid-call.
pub const INTERNAL_ERROR: i64 = -32603;

/// How a call that did not produce output failed.
///
/// Before this, every failure arrived as one prose string prefixed "Error
/// running tool", so a workspace policy refusing an egress call and a handler
/// that could not open a file were the same fact to the caller. The distinction
/// is not cosmetic: one is a decision to route around, the other is a bug to
/// report, and a client that retries the second kind will retry the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    /// A gate — rules, policy, or a lifecycle hook — declined to run the tool.
    PermissionDenied,
    /// No tool by that name is registered.
    ToolNotFound,
    /// The tool exists and the arguments do not fit it.
    InvalidArgument,
    /// Anything else, including I/O and serialisation failures.
    Internal,
}

impl FailureKind {
    /// The label put on the wire. Stable strings, because a client branches on
    /// them and prose is what this whole type exists to avoid.
    pub fn label(self) -> &'static str {
        match self {
            FailureKind::PermissionDenied => "permission_denied",
            FailureKind::ToolNotFound => "tool_not_found",
            FailureKind::InvalidArgument => "invalid_argument",
            FailureKind::Internal => "internal",
        }
    }
}

/// Which kind of failure a dispatch error is, read off its type rather than its
/// text. An error that is not a [`ToolError`] is internal by definition: the
/// gates all wrap their refusals, so an untyped error is one nobody classified.
pub fn classify(err: &(dyn Error + 'static)) -> FailureKind {
    match err.downcast_ref::<ToolError>() {
        Some(ToolError::GovernanceDenied(_)) => FailureKind::PermissionDenied,
        Some(ToolError::ToolNotFound(_)) => FailureKind::ToolNotFound,
        Some(ToolError::InvalidArgument(_)) => FailureKind::InvalidArgument,
        _ => FailureKind::Internal,
    }
}

/// The text a failed call hands back.
///
/// A refusal is quoted verbatim because it already names the authority that
/// declined and the remedy — prefixing it with "Error running tool" would
/// restyle a decision about the workspace as a fault inside the tool, which is
/// the reading that sends a model retrying.
pub fn failure_text(name: &str, message: &str, kind: FailureKind) -> String {
    match kind {
        FailureKind::PermissionDenied => message.to_string(),
        _ => format!("Error running tool '{}': {}", name, message),
    }
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
}

fn rpc_result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// The reply to a `tools/call`, given the dispatch it asked for.
///
/// `dispatch` is a parameter, not a call, so the three ways a tool call fails —
/// declined, faulty, panicking — are gradable without a workspace root, a policy
/// file or a registered handler.
pub fn tool_call_response<F>(id: Value, name: &str, arguments: &Value, dispatch: F) -> Value
where
    F: FnOnce(&str, &Value) -> Result<String, Box<dyn Error>>,
{
    let outcome =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| dispatch(name, arguments)));
    match outcome {
        Ok(Ok(output)) => rpc_result(
            id,
            json!({
                "content": [{ "type": "text", "text": output }],
                "isError": false,
            }),
        ),
        Ok(Err(err)) => {
            let kind = classify(err.as_ref());
            let text = failure_text(name, &err.to_string(), kind);
            rpc_result(
                id,
                json!({
                    "content": [{ "type": "text", "text": text }],
                    "isError": true,
                    "errorKind": kind.label(),
                }),
            )
        }
        Err(panic_payload) => {
            // The handler is gone with an unknown amount of its work done, so
            // there is no tool outcome to report in band. Named on stderr, which
            // is not the protocol channel, and the payload stays out of the
            // reply: a panic can carry a path or a token the caller should not
            // get echoed.
            let note = panic_text(panic_payload.as_ref());
            log::error!("tool '{}' panicked: {}", name, note);
            rpc_error(
                id,
                INTERNAL_ERROR,
                &format!(
                    "Internal error: the handler for '{name}' panicked and did not complete; \
                     partial side effects are possible"
                ),
            )
        }
    }
}

/// A panic payload's text, if it carries any. `catch_unwind` gives back the
/// payload as `&dyn Any`, which is `&str`/`String` for the usual `panic!` and
/// neither for a payload that is not text at all.
fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        return (*s).to_string();
    }
    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }
    "non-string panic payload".to_string()
}

/// Answer one protocol line. `None` means the line asked for nothing back.
pub fn handle_line(line: &str) -> Option<Value> {
    let request: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            return Some(rpc_error(
                Value::Null,
                PARSE_ERROR,
                &format!("Parse error: {e}"),
            ))
        }
    };
    let method = request["method"].as_str().unwrap_or("");
    let id = request["id"].clone();
    // A notification has no id and gets no reply, however the rest of it reads —
    // that is what makes it a notification. Anything else has to be answered,
    // including a request whose method is missing.
    if id.is_null() {
        return None;
    }
    match method {
        "initialize" => Some(rpc_result(
            id,
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": {
                    "name": "velocity-mcp-rust-server",
                    "version": "1.0.0",
                },
            }),
        )),
        // MCP asks a client to be able to measure liveness without spending a
        // tool call. An empty result is the whole of the answer.
        "ping" => Some(rpc_result(id, json!({}))),
        "tools/list" => Some(rpc_result(id, json!({ "tools": registry::get_tools() }))),
        "tools/call" => {
            let name = request["params"]["name"].as_str().unwrap_or("");
            if name.is_empty() {
                return Some(rpc_error(
                    id,
                    INVALID_PARAMS,
                    "tools/call requires params.name",
                ));
            }
            let arguments = &request["params"]["arguments"];
            let owned;
            let arguments = if arguments.is_null() {
                owned = json!({});
                &owned
            } else {
                arguments
            };
            Some(tool_call_response(id, name, arguments, registry::call_tool))
        }
        other => Some(rpc_error(
            id,
            METHOD_NOT_FOUND,
            &format!("Method '{other}' not found"),
        )),
    }
}

/// The reply a request/response transport must put on the wire for one line.
///
/// The stdio loop answers a request and stays silent for a notification, which is
/// correct there because the client reads a message stream. The shared-memory
/// loop blocks its host until the buffer holds something, so silence is a hang:
/// a notification gets an empty result instead of no answer.
pub fn response_for_transport(line: &str) -> Value {
    handle_line(line).unwrap_or_else(|| rpc_result(Value::Null, json!({})))
}

/// Whether a reply answers nothing — the line could not be parsed, so there is no
/// id to echo back and nothing for the host to correlate the answer with.
///
/// This is the condition the shared-memory face marks `STATE_ERROR`, and it is
/// deliberately narrower than "the reply carries an `error` object". A `-32601`
/// or `-32602` is still an answer sitting in the buffer with an id on it, and a
/// host that reads the output region only when the state says a result is ready
/// would wait forever on a reply it should have taken; the old hand-written
/// shared-memory loop marked only the parse failure that way, and this keeps
/// that reading. Whether a *tool* failed is not the buffer's business — that is
/// in the payload (`isError`, then `errorKind`).
pub fn is_unanswerable(response: &Value) -> bool {
    response.get("error").is_some() && response.get("id").map(|id| id.is_null()).unwrap_or(false)
}

pub fn run_stdio_loop() -> Result<(), Box<dyn Error>> {
    let stdin = io::stdin();
    let mut handle = stdin.lock();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    loop {
        let mut line = String::new();
        let bytes_read = handle.read_line(&mut line)?;
        if bytes_read == 0 {
            break; // EOF
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(response) = handle_line(trimmed) {
            writeln!(out, "{}", response)?;
            out.flush()?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn denied(message: &str) -> Box<dyn Error> {
        Box::new(ToolError::GovernanceDenied(message.to_string()))
    }

    #[test]
    fn a_ping_is_answered_and_is_not_an_unknown_method() {
        let response = handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#).unwrap();
        assert_eq!(response["id"], json!(1));
        assert_eq!(
            response["result"],
            json!({}),
            "ping's whole contract is an empty result; a method-not-found error here means \
             the server cannot be probed for liveness"
        );
        assert!(response.get("error").is_none(), "{response}");
    }

    #[test]
    fn an_unknown_method_still_becomes_method_not_found() {
        // The ping did not replace the error; it joined the methods. A client
        // that typos a real method must still get -32601.
        let response = handle_line(r#"{"jsonrpc":"2.0","id":7,"method":"tools/lin"}"#).unwrap();
        assert_eq!(response["error"]["code"], json!(-32601), "{response}");
        assert!(response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("tools/lin"));
    }

    #[test]
    fn a_notification_awaits_no_reply_whether_or_not_we_know_it() {
        for line in [
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":3}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/whatever"}"#,
        ] {
            assert_eq!(
                handle_line(line),
                None,
                "a message with no id is a notification; answering it desynchronises the \
                 client's request stream"
            );
        }
    }

    #[test]
    fn garbage_is_a_parse_error_on_the_channel_rather_than_silence() {
        let response = handle_line("Starting V.E.L.O.C.I.T.Y. NMCP Server...").unwrap();
        assert_eq!(response["error"]["code"], json!(-32700), "{response}");
        assert_eq!(response["id"], Value::Null);
    }

    #[test]
    fn a_call_without_a_tool_name_is_an_invalid_request_not_a_call_of_the_empty_tool() {
        let response =
            handle_line(r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{}}"#).unwrap();
        assert_eq!(response["error"]["code"], json!(-32602), "{response}");
        assert!(
            response.get("result").is_none(),
            "no tool ran, so there is no tool outcome to report: {response}"
        );
    }

    #[test]
    fn a_refusal_and_an_internal_failure_are_different_facts() {
        let refusal = tool_call_response(
            json!(3),
            "fetch_url",
            &json!({"url": "https://example.com"}),
            |_, _| {
                Err(denied(
                    "action blocked — server \"example.com\"; rule \"egress-off\"",
                ))
            },
        );
        assert_eq!(refusal["result"]["isError"], json!(true));
        assert_eq!(refusal["result"]["errorKind"], json!("permission_denied"));

        let broken = tool_call_response(json!(3), "read_file", &json!({}), |_, _| {
            Err(Box::new(ToolError::Internal("disk went away".to_string())))
        });
        assert_eq!(broken["result"]["isError"], json!(true));
        assert_eq!(
            broken["result"]["errorKind"],
            json!("internal"),
            "the two labels must differ, or the type has bought nothing"
        );
        assert_ne!(
            refusal["result"]["errorKind"], broken["result"]["errorKind"],
            "the audit's finding was that these were one fact to the caller"
        );
    }

    #[test]
    fn the_classification_covers_every_tool_error_variant() {
        assert_eq!(
            classify(&ToolError::GovernanceDenied("x".into())),
            FailureKind::PermissionDenied
        );
        assert_eq!(
            classify(&ToolError::ToolNotFound("x".into())),
            FailureKind::ToolNotFound
        );
        assert_eq!(
            classify(&ToolError::InvalidArgument("x".into())),
            FailureKind::InvalidArgument
        );
        assert_eq!(
            classify(&ToolError::Internal("x".into())),
            FailureKind::Internal
        );
        assert_eq!(
            classify(&ToolError::from(std::io::Error::other("x"))),
            FailureKind::Internal,
            "I/O is a fault, not a decision"
        );
        // An error nobody typed — a bare string from an older call path — is
        // internal, which is the honest default: we do not know that it was a
        // refusal, so we must not let a client treat it as one.
        let untyped: Box<dyn Error> = "something said no".into();
        assert_eq!(classify(untyped.as_ref()), FailureKind::Internal);
    }

    #[test]
    fn a_refusal_is_quoted_and_only_a_fault_is_wrapped_in_prose() {
        let reason = "action blocked — rule \"egress-off\"; use the dev server instead";
        assert_eq!(
            failure_text("fetch_url", reason, FailureKind::PermissionDenied),
            reason,
            "the refusal's own words name the authority and the remedy; a prefix would \
             restyle a decision about the workspace as a fault inside the tool"
        );
        let fault = failure_text("read_file", "no such file", FailureKind::Internal);
        assert_eq!(fault, "Error running tool 'read_file': no such file");
    }

    #[test]
    fn a_panicking_handler_is_an_internal_error_and_leaves_the_reply_answerable() {
        let response = tool_call_response(json!(4), "boom_tool", &json!({}), |_, _| {
            panic!("handler lost its footing")
        });
        assert_eq!(response["error"]["code"], json!(-32603), "{response}");
        assert!(response.get("result").is_none());
        assert!(
            response["error"]["message"]
                .as_str()
                .unwrap()
                .contains("boom_tool"),
            "the client is told which call broke: {}",
            response["error"]["message"]
        );
        assert!(
            !response.to_string().contains("lost its footing"),
            "a panic payload can carry a path or a secret; it stays off the channel"
        );
    }

    #[test]
    fn a_successful_call_reports_its_output_with_no_error_kind_attached() {
        let response =
            tool_call_response(json!(5), "read_file", &json!({"path": "a.rs"}), |_, _| {
                Ok("fn main() {}".to_string())
            });
        assert_eq!(response["result"]["isError"], json!(false));
        assert_eq!(
            response["result"]["content"][0]["text"],
            json!("fn main() {}")
        );
        assert!(
            response["result"].get("errorKind").is_none(),
            "a kind is only owed when there is a failure to classify: {}",
            response["result"]
        );
    }

    #[test]
    fn the_startup_banner_never_touches_the_protocol_channel() {
        // The banner used to be a print to stdout before the stdio loop opened, and
        // a client that read the first line as a message got prose — the harness
        // recorded it being written into a file as if it were a response. (Spelled
        // that way because this very test greps this very file for the macro that
        // writes there.)
        // stdout is the channel in stdio mode; only the help screen owns it.
        //
        // The cut is at `fn print_help` because that printer, and the tokenizer
        // demo after it, are the binary's human-facing output: they run *instead
        // of* serving, never alongside a protocol loop.
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let main_path = manifest.join("main.rs");
        let text = std::fs::read_to_string(&main_path)
            .unwrap_or_else(|e| panic!("{} must be readable: {e}", main_path.display()));
        let serve_path = text
            .find("fn print_help")
            .expect("main.rs keeps its help printer");
        assert!(
            stdout_prints(&text[..serve_path]).is_empty(),
            "every bare println! before the help printer writes to stdout, which is the \
             JSON-RPC channel in stdio mode"
        );

        // The two files that own a wire get no bare println either: the stdio loop
        // writes its replies with `writeln!` on purpose, and anything else printed
        // on that stream is a corrupted frame.
        for name in ["json_rpc.rs", "nmcp_binary.rs"] {
            let path = manifest.join("protocol").join(name);
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{} must be readable: {e}", path.display()));
            assert!(
                stdout_prints(&source).is_empty(),
                "{name} writes to stdout with a bare println!; on this channel that is \
                 protocol traffic, not logging"
            );
        }
    }

    /// Byte offsets of every print macro call in `text` that goes to stdout.
    ///
    /// The stderr form ends in the same four characters the matcher looks for, so
    /// the preceding letter decides which stream a hit belongs to — and a guard
    /// that counted `eprintln!` as stdout would fail the day after it was written.
    fn stdout_prints(text: &str) -> Vec<usize> {
        // Spliced rather than written out: this file is one of the files the guard
        // reads, and the literal would match itself.
        let needle = concat!("print", "ln!(");
        let mut hits = Vec::new();
        let mut from = 0;
        while let Some(offset) = text[from..].find(needle) {
            let at = from + offset;
            if !text[..at].ends_with('e') {
                hits.push(at);
            }
            from = at + 1;
        }
        hits
    }

    #[test]
    fn a_request_response_transport_always_gets_something_back() {
        // The shared-memory loop blocks its host until the buffer holds a reply,
        // so the notification rule that is right for stdio would hang it.
        let ack = response_for_transport(r#"{"jsonrpc":"2.0","method":"notifications/x"}"#);
        assert_eq!(ack["jsonrpc"], json!("2.0"));
        assert_eq!(ack["result"], json!({}));
        assert!(!is_unanswerable(&ack), "an empty ack is an answer");

        // An unknown method is answered *wrong*, but answered: the payload is in
        // the buffer with the request's id on it, so the state must keep saying a
        // result is ready. Marking it STATE_ERROR is how a host that reads the
        // output region only on RES_READY ends up waiting for a reply it already
        // has — the hand-written shared-memory loop it replaced marked only the
        // unparseable line, and this face has to keep doing that.
        let unknown = response_for_transport(r#"{"jsonrpc":"2.0","id":1,"method":"bogus"}"#);
        assert_eq!(unknown["error"]["code"], json!(-32601));
        assert!(!is_unanswerable(&unknown), "a -32601 is still a reply");

        let unparseable = response_for_transport("not json at all");
        assert_eq!(unparseable["error"]["code"], json!(-32700));
        assert!(
            is_unanswerable(&unparseable),
            "a line that never became a request has no answer to correlate"
        );

        // A declined tool is a correctly answered request, not a broken envelope.
        let refused = tool_call_response(json!(2), "run_command", &json!({}), |_, _| {
            Err(denied("blocked by policy: tool 'run_command' is denied"))
        });
        assert!(!is_unanswerable(&refused));
    }

    #[test]
    fn every_answer_is_one_json_object_enveloped_as_jsonrpc_2() {
        for line in [
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"nope"}"#,
            "not json at all",
        ] {
            let response = handle_line(line)
                .unwrap_or_else(|| panic!("{line} asked for an answer and got none"));
            assert_eq!(response["jsonrpc"], json!("2.0"), "{response}");
            assert!(
                response.get("result").is_some() != response.get("error").is_some(),
                "a JSON-RPC reply carries exactly one of result or error: {response}"
            );
        }
    }
}
