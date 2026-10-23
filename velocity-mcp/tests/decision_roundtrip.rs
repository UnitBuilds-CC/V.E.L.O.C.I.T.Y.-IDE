//! End-to-end tests for the decision layer's HTTP path.
//!
//! These do NOT require a real Laya/Jev service. They stand up a tiny,
//! local, single-shot HTTP responder on `127.0.0.1` that speaks the same wire
//! contract Laya exposes (a JSON envelope of keyed, typed answers), then drive
//! the *real* [`LayaModel::decide`] code path — request build, socket POST,
//! response parse, answer finalization — through `ureq`. That makes them a
//! genuine round-trip test rather than a mock of our own logic.
//!
//! They also pin the two guarantees the whole design rests on:
//!   * a well-formed answer set parses back into typed [`Answer`]s;
//!   * an unreachable / slow endpoint yields a `Transport` error fast, so the
//!     caller (e.g. `intent::decide_intent`) can fall back to the deterministic
//!     scorer instead of hanging the chat.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use velocity_mcp::agent::decision::{
    Answer, DecisionError, DecisionModel, DecisionState, LayaModel, Question,
};

/// Start a one-connection HTTP responder that replies with `body`, and return
/// the ephemeral port it is listening on. The listener is bound synchronously
/// so the caller has a real port before it constructs a client.
fn serve_once(body: &'static str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        // Accept exactly one request, then stop. Tests each get a fresh server.
        if let Ok((mut stream, _)) = listener.accept() {
            drain_request(&mut stream);
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.flush();
        }
    });
    port
}

/// Start a responder that accepts the connection but never replies, so a client
/// with a short timeout must give up on its own. Used to prove the fast-fallback.
fn serve_hang() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        // Hold the socket open (headers read) but write nothing; drop after a
        // beat so the OS does not keep the port wedged for the whole suite.
        if let Ok((stream, _)) = listener.accept() {
            let mut s = stream;
            drain_request(&mut s);
            thread::sleep(Duration::from_millis(1500));
        }
    });
    port
}

/// Read the request line + headers, then the declared body, so the client's
/// write fully completes (avoids a broken-pipe race on some platforms).
fn drain_request(stream: &mut TcpStream) {
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(c) => c,
        Err(_) => return,
    });
    let mut content_len = 0usize;
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if line.trim().is_empty() {
            break; // end of headers
        }
        let lower = line.to_ascii_lowercase();
        if let Some(rest) = lower.trim().strip_prefix("content-length:") {
            content_len = rest.trim().parse().unwrap_or(0);
        }
    }
    if content_len > 0 {
        let mut buf = vec![0u8; content_len];
        let _ = reader.read_exact(&mut buf);
    }
}

fn intent_questions() -> Vec<Question> {
    vec![
        Question::Choice {
            id: "scope".to_string(),
            prompt: "single agent or team?".to_string(),
            choices: vec!["single_agent".to_string(), "team".to_string()],
            descriptions: vec![],
        },
        Question::Noul {
            id: "requires_repo_ingest".to_string(),
            prompt: "needs a repo pull?".to_string(),
        },
    ]
}

#[test]
fn laya_round_trip_over_real_http_parses_typed_answers() {
    // A response exactly in the shape llama.cpp's /v1/systemone returns for a
    // loaded Laya: `choice` selected label, per-option probabilities, and a
    // `noul` probability keyed under "noul" (not "yes").
    let body: &'static str = r#"{"model":"ggml-org/Laya-GGUF","answers":{
        "scope":{"type":"choice","choice":"team","probabilities":{"single_agent":0.1,"team":0.9},"confidence":0.9},
        "requires_repo_ingest":{"type":"noul","noul":0.82}
    },"usage":{"input_tokens":18,"output_tokens":0}}"#;
    let port = serve_once(body);
    let model = LayaModel {
        endpoint: format!("http://127.0.0.1:{port}"),
        path: "/decide".to_string(),
        timeout_ms: 2000,
    };

    let out = model
        .decide(
            &DecisionState::text("clone litellm and rewrite it in rust"),
            &intent_questions(),
        )
        .expect("round trip should succeed");

    assert_eq!(out.backend, "laya");
    match out.get("scope").expect("scope answer") {
        Answer::Choice {
            selected,
            confidence,
            probabilities,
            ..
        } => {
            assert_eq!(selected, "team");
            assert!((*confidence - 0.9).abs() < 1e-6);
            assert_eq!(probabilities.len(), 2);
        }
        other => panic!("expected a Choice, got {other:?}"),
    }
    match out.get("requires_repo_ingest").expect("noul answer") {
        Answer::Noul {
            yes_probability, ..
        } => {
            assert!((*yes_probability - 0.82).abs() < 1e-6)
        }
        other => panic!("expected a Noul, got {other:?}"),
    }
}

#[test]
fn laya_transport_error_when_endpoint_is_dead() {
    // Bind then immediately drop the listener so the port is closed.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let model = LayaModel {
        endpoint: format!("http://127.0.0.1:{port}"),
        path: "/decide".to_string(),
        timeout_ms: 500,
    };
    let err = model
        .decide(&DecisionState::text("anything"), &intent_questions())
        .expect_err("a closed port must not look like a success");
    assert!(
        matches!(err, DecisionError::Transport(_)),
        "expected Transport, got {err:?}"
    );
}

#[test]
fn laya_honors_the_timeout_budget_instead_of_hanging() {
    let port = serve_hang();
    let model = LayaModel {
        endpoint: format!("http://127.0.0.1:{port}"),
        path: "/decide".to_string(),
        timeout_ms: 300,
    };
    let started = std::time::Instant::now();
    let result = model.decide(&DecisionState::text("slow reflex"), &intent_questions());
    let elapsed = started.elapsed();

    assert!(result.is_err(), "a non-responsive server must error out");
    // Comfortably below the server's 1.5s hold: the client gave up on budget.
    assert!(
        elapsed < Duration::from_millis(1200),
        "decide exceeded its timeout budget: {elapsed:?}"
    );
}

#[test]
fn malformed_json_body_is_reported_not_panicked() {
    let body: &'static str = "this is not json";
    let port = serve_once(body);
    let model = LayaModel {
        endpoint: format!("http://127.0.0.1:{port}"),
        path: "/decide".to_string(),
        timeout_ms: 1000,
    };
    let err = model
        .decide(&DecisionState::text("x"), &intent_questions())
        .expect_err("non-JSON must be an error");
    assert!(
        matches!(
            err,
            DecisionError::Malformed(_) | DecisionError::Transport(_)
        ),
        "expected Malformed/Transport, got {err:?}"
    );
}
