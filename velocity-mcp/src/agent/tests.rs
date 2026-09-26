use super::executor::utils::*;
#[allow(unused_imports)]
use super::executor::*;
use super::models::*;
use super::nda::*;
use super::provider::*;

fn message() -> ChatMessage {
    ChatMessage {
        role: "user".into(),
        content: "hello".into(),
        name: None,
        tool_call_id: None,
        tool_calls: None,
    }
}

#[test]
fn openai_chat_profile_omits_tools_and_thinking() {
    let profile = ModelInfo {
        id: "@cf/example/chat".into(),
        label: "chat".into(),
        api_style: ApiStyle::OpenAiChat,
        supports_tools: false,
        supports_thinking: false,
    };
    let request = build_request(
        &profile,
        &profile.id,
        &[message()],
        &[serde_json::json!({"type": "function"})],
        true,
        AiProvider::CloudflareWorkersAi,
    );
    assert!(request.get("messages").is_some());
    assert!(request.get("tools").is_none());
    assert!(request.get("thinking").is_none());
}

#[test]
fn serializes_last_request_as_nda() {
    let profile = ModelInfo {
        id: "@cf/example/chat".into(),
        label: "chat".into(),
        api_style: ApiStyle::OpenAiTools,
        supports_tools: true,
        supports_thinking: true,
    };
    let messages = vec![
        ChatMessage {
            role: "system".into(),
            content: "system prompt".into(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        },
        ChatMessage {
            role: "assistant".into(),
            content: "calling tool".into(),
            name: None,
            tool_call_id: None,
            tool_calls: Some(
                serde_json::json!([{"id":"call_1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"src/main.rs\"}"}}]),
            ),
        },
    ];
    let tools = vec![serde_json::json!({
        "type": "function",
        "function": {
            "name": "read_file",
            "description": "Read a file",
            "parameters": {"type": "object"}
        }
    })];
    let request = build_request(
        &profile,
        &profile.id,
        &messages,
        &tools,
        true,
        AiProvider::CloudflareWorkersAi,
    );

    let nda = serialize_last_request_nda(
        &profile,
        &profile.id,
        AiProvider::CloudflareWorkersAi,
        true,
        &messages,
        &tools,
        &request,
    );

    assert!(nda.starts_with("last-request version 3\n"));
    assert!(nda.contains("field\tprovider\tcloudflare-workers-ai"));
    assert!(nda.contains("field\tapi_style\topenai-tools"));
    assert!(nda.contains("field\tprofile_id\t@cf/example/chat"));
    assert!(nda.contains("message_count 2"));
    assert!(nda.contains("tool_count 1"));
    assert!(nda.contains("message_field\t1\trole\tassistant"));
    assert!(nda.contains("message_field\t1\tcontent\tcalling tool"));
    assert!(nda.contains("message_tool_call\t1\t0"));
    assert!(nda.contains("message_tool_call_field\t1\t0\tid\tcall_1"));
    assert!(nda.contains("message_tool_call_field\t1\t0\tfunction_name\tread_file"));
    assert!(nda.contains("message_tool_call_arg\t1\t0\t$.path\tstring\tsrc/main.rs"));
    assert!(nda.contains("tool_field\t0\tname\tread_file"));
    assert!(nda.contains("tool_field\t0\tdescription\tRead a file"));
    assert!(nda.contains("tool_parameter\t0\t$\tobject\t-"));
    assert!(nda.contains("tool_parameter\t0\t$.type\tstring\tobject"));
}

#[test]
fn writes_plaintext_transcript_nda() {
    let tmp = tempfile::tempdir().unwrap();
    let content = b"{\"role\":\"user\"}\n{\"role\":\"assistant\"}\n";

    write_workspace_transcript_nda(tmp.path(), content);
    let raw = std::fs::read(tmp.path().join(".velocity").join("transcript.nda")).unwrap();
    let transcript =
        String::from_utf8(crate::agent::crypto::open(tmp.path(), b"transcript", &raw)).unwrap();
    assert!(transcript.starts_with("transcript version 2\n"));
    assert!(transcript.contains("field_count 2\n"));
    assert!(transcript.contains("field\tsource\tjsonl\n"));
    assert!(transcript.contains("field\ttrailing_newline\ttrue\n"));
    assert!(transcript.contains("line_count 2\n"));
    assert!(transcript.contains("line\t0\t{\"role\":\"user\"}"));
    assert!(transcript.contains("line\t1\t{\"role\":\"assistant\"}"));
}

#[test]
fn writes_plaintext_sitemap_nda() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src").join("nested")).unwrap();
    std::fs::create_dir_all(tmp.path().join(".velocity")).unwrap();
    std::fs::write(tmp.path().join("src").join("main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(
        tmp.path().join("src").join("nested").join("lib.rs"),
        "pub fn x() {}\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join(".velocity").join("ignored.txt"),
        "ignore me",
    )
    .unwrap();

    write_sitemap_nda(tmp.path());
    let raw = std::fs::read(tmp.path().join(".velocity").join("sitemap.nda")).unwrap();
    let sitemap =
        String::from_utf8(crate::agent::crypto::open(tmp.path(), b"sitemap", &raw)).unwrap();
    assert!(sitemap.starts_with("sitemap version 2\n"));
    assert!(sitemap.contains("entry_count 4\n"));
    // The escaper doubles a backslash and has no reason to touch `/`, so the
    // separator inside these records is whichever one the platform produced.
    // Writing the Windows form as a literal asserts nothing on Unix, where the
    // same tree is recorded as `src/nested`; deriving it keeps both honest.
    let sep = encode_nda_text(std::path::MAIN_SEPARATOR_STR);
    assert!(sitemap.contains("\tdir\tsrc\t-"));
    assert!(sitemap.contains(&format!("\tdir\tsrc{sep}nested\t-")));
    assert!(sitemap.contains(&format!("\tfile\tsrc{sep}main.rs\t")));
    assert!(sitemap.contains(&format!("\tfile\tsrc{sep}nested{sep}lib.rs\t")));
    assert!(!sitemap.contains("V.E.L.O.C.I.T.Y. Codebase Sitemap Registry"));
    assert!(!sitemap.contains("ignored.txt"));
}

#[test]
fn writes_plaintext_chatlogs_nda() {
    let tmp = tempfile::tempdir().unwrap();
    let messages = vec![
        ChatMessage {
            role: "assistant".into(),
            content: "hello\nworld".into(),
            name: None,
            tool_call_id: None,
            tool_calls: Some(
                serde_json::json!([{"id":"call_1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"src/main.rs\"}"}}]),
            ),
        },
        ChatMessage {
            role: "tool".into(),
            content: "done".into(),
            name: Some("read_file".into()),
            tool_call_id: Some("call_1".into()),
            tool_calls: None,
        },
    ];

    save_chatlogs_nda(tmp.path(), &messages);
    let raw = std::fs::read(tmp.path().join(".velocity").join("chatlogs.nda")).unwrap();
    let chatlogs =
        String::from_utf8(crate::agent::crypto::open(tmp.path(), b"chatlogs", &raw)).unwrap();
    assert!(chatlogs.starts_with("chatlogs version 3\n"));
    assert!(chatlogs.contains("message_count 2"));
    assert!(chatlogs.contains("field\t0\trole\tassistant"));
    assert!(chatlogs.contains("field\t0\tcontent\thello\\nworld"));
    assert!(chatlogs.contains("tool_call\t0\t0"));
    assert!(chatlogs.contains("tool_call_field\t0\t0\tid\tcall_1"));
    assert!(chatlogs.contains("tool_call_field\t0\t0\tfunction_name\tread_file"));
    assert!(chatlogs.contains("tool_call_field\t0\t0\targuments\t{\"path\":\"src/main.rs\"}"));
    assert!(chatlogs.contains("field\t1\tname\tread_file"));
    assert!(chatlogs.contains("field\t1\ttool_call_id\tcall_1"));

    let loaded = load_chatlogs_nda(tmp.path()).unwrap();
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].content, "hello\nworld");
    assert!(loaded[0].tool_calls.is_some());
    assert_eq!(loaded[1].name.as_deref(), Some("read_file"));
    assert_eq!(loaded[1].tool_call_id.as_deref(), Some("call_1"));
}

#[test]
fn loads_legacy_v2_chatlogs_nda() {
    let tmp = tempfile::tempdir().unwrap();
    let velocity_dir = tmp.path().join(".velocity");
    std::fs::create_dir_all(&velocity_dir).unwrap();
    let legacy_tool_calls = encode_nda_text(
        "[{\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"src/main.rs\\\"}\"}}]",
    );
    std::fs::write(
        velocity_dir.join("chatlogs.nda"),
        format!(
            "chatlogs version 2\nfield\t0\trole\tassistant\nfield\t0\tname\t-\nfield\t0\ttool_call_id\t-\nfield\t0\tcontent\thello\\nworld\nfield\t0\ttool_calls\t{}\n",
            legacy_tool_calls
        ),
    )
    .unwrap();

    let loaded = load_chatlogs_nda(tmp.path()).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].role, "assistant");
    assert_eq!(loaded[0].content, "hello\nworld");
    assert!(loaded[0].tool_calls.is_some());
}

#[test]
fn loads_legacy_v1_chatlogs_nda() {
    let tmp = tempfile::tempdir().unwrap();
    let velocity_dir = tmp.path().join(".velocity");
    std::fs::create_dir_all(&velocity_dir).unwrap();
    std::fs::write(
        velocity_dir.join("chatlogs.nda"),
        "chatlogs version 1\nmessage\t0\tassistant\t-\t-\thello\\nworld\t-\nmessage\t1\ttool\tread_file\tcall_1\tdone\t-\n",
    )
    .unwrap();

    let loaded = load_chatlogs_nda(tmp.path()).unwrap();
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].role, "assistant");
    assert_eq!(loaded[0].content, "hello\nworld");
    assert_eq!(loaded[1].role, "tool");
    assert_eq!(loaded[1].name.as_deref(), Some("read_file"));
    assert_eq!(loaded[1].tool_call_id.as_deref(), Some("call_1"));
    assert_eq!(loaded[1].content, "done");
}

#[test]
fn loads_legacy_ndav_chatlogs_nda() {
    let tmp = tempfile::tempdir().unwrap();
    let velocity_dir = tmp.path().join(".velocity");
    std::fs::create_dir_all(&velocity_dir).unwrap();
    let legacy = "user\nhello\n---\ntool\nread_file\tcall_1\ndone";
    std::fs::write(
        velocity_dir.join("chatlogs.nda"),
        pack_ndav("chatlogs.txt", legacy.as_bytes()),
    )
    .unwrap();

    let loaded = load_chatlogs_nda(tmp.path()).unwrap();
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].role, "user");
    assert_eq!(loaded[0].content, "hello");
    assert_eq!(loaded[1].role, "tool");
    assert_eq!(loaded[1].name.as_deref(), Some("read_file"));
    assert_eq!(loaded[1].tool_call_id.as_deref(), Some("call_1"));
    assert_eq!(loaded[1].content, "done");
}

#[test]
fn writes_plaintext_handover_nda() {
    let tmp = tempfile::tempdir().unwrap();
    write_handover_nda(tmp.path(), "self_correcting", 7, "compile failed", true);
    let raw = std::fs::read(tmp.path().join(".velocity").join("handover.nda")).unwrap();
    let handover =
        String::from_utf8(crate::agent::crypto::open(tmp.path(), b"handover", &raw)).unwrap();
    assert!(handover.starts_with("handover version 2\n"));
    assert!(handover.contains("field_count 4\n"));
    assert!(handover.contains("field\tstate\tself_correcting"));
    assert!(handover.contains("field\tturn\t7"));
    assert!(handover.contains("field\tbuild\tcompile failed"));
    assert!(handover.contains("field\tinterrupted\ttrue"));
}

#[test]
fn appends_plaintext_changelog_nda() {
    let tmp = tempfile::tempdir().unwrap();
    append_changelog_nda(tmp.path(), "src/main.rs", "edited");
    append_changelog_nda(tmp.path(), "src/lib.rs", "created");
    let raw = std::fs::read(tmp.path().join(".velocity").join("changelog.nda")).unwrap();
    let changelog =
        String::from_utf8(crate::agent::crypto::open(tmp.path(), b"changelog", &raw)).unwrap();
    assert!(changelog.starts_with("changelog version 2\n"));
    assert!(changelog.contains("entry_count 2\n"));
    assert!(changelog.contains("\tsrc/main.rs\tedited"));
    assert!(changelog.contains("\tsrc/lib.rs\tcreated"));
}

#[test]
fn appends_to_legacy_ndav_changelog() {
    let tmp = tempfile::tempdir().unwrap();
    let velocity_dir = tmp.path().join(".velocity");
    std::fs::create_dir_all(&velocity_dir).unwrap();
    std::fs::write(
        velocity_dir.join("changelog.nda"),
        pack_ndav("changelog.txt", b"123\tsrc/old.rs\tupdated\n"),
    )
    .unwrap();

    append_changelog_nda(tmp.path(), "src/new.rs", "created");
    let raw = std::fs::read(velocity_dir.join("changelog.nda")).unwrap();
    let changelog =
        String::from_utf8(crate::agent::crypto::open(tmp.path(), b"changelog", &raw)).unwrap();
    assert!(changelog.starts_with("changelog version 2\n"));
    assert!(changelog.contains("entry_count 2\n"));
    assert!(changelog.contains("\tsrc/old.rs\tupdated"));
    assert!(changelog.contains("\tsrc/new.rs\tcreated"));
}

#[test]
fn test_sanitize_chat_token_removes_tags() {
    let input =
        "Hello there </tool_call>\n</tool_call>\n<parameter=path>src/main.rs</parameter> checking.";
    let expected = "Hello there src/main.rs checking.";
    assert_eq!(sanitize_chat_token(input).trim(), expected);
}

#[test]
fn prompt_profile_uses_prompt_and_no_tools() {
    let profile = ModelInfo {
        id: "@cf/example/base".into(),
        label: "base".into(),
        api_style: ApiStyle::PromptCompletion,
        supports_tools: false,
        supports_thinking: false,
    };
    let request = build_request(
        &profile,
        &profile.id,
        &[message()],
        &[serde_json::json!({"type": "function"})],
        false,
        AiProvider::CloudflareWorkersAi,
    );
    assert_eq!(request["prompt"], "user: hello");
    assert!(request.get("messages").is_none());
    assert!(request.get("tools").is_none());
}

#[test]
fn compress_history_flattens_tools_when_unsupported() {
    let original_messages = vec![
        ChatMessage {
            role: "assistant".to_string(),
            content: "".to_string(),
            name: None,
            tool_call_id: None,
            tool_calls: Some(serde_json::json!([
                {
                    "id": "call_abc",
                    "type": "function",
                    "function": {
                        "name": "write_file",
                        "arguments": "{\"path\":\"hello.txt\"}"
                    }
                }
            ])),
        },
        ChatMessage {
            role: "tool".to_string(),
            content: "Success".to_string(),
            name: Some("write_file".to_string()),
            tool_call_id: Some("call_abc".to_string()),
            tool_calls: None,
        },
    ];

    let compressed = compress_history(&original_messages, false);
    assert_eq!(compressed.len(), 2);

    assert_eq!(compressed[0].role, "assistant");
    assert_eq!(
        compressed[0].content,
        "[Calling tool 'write_file' with arguments '{\"path\":\"hello.txt\"}']"
    );
    assert!(compressed[0].tool_calls.is_none());

    assert_eq!(compressed[1].role, "user");
    assert_eq!(
        compressed[1].content,
        "[Tool result for 'write_file']: Success"
    );
    assert!(compressed[1].name.is_none());
    assert!(compressed[1].tool_call_id.is_none());
}

#[test]
fn writes_last_request_artifacts() {
    let tmp = tempfile::tempdir().unwrap();
    let profile = ModelInfo {
        id: "@cf/example/chat".into(),
        label: "chat".into(),
        api_style: ApiStyle::OpenAiChat,
        supports_tools: false,
        supports_thinking: false,
    };
    let messages = vec![message()];
    let tools = vec![
        serde_json::json!({"type": "function", "function": {"name": "search", "description": "Search"}}),
    ];
    let request = build_request(
        &profile,
        &profile.id,
        &messages,
        &tools,
        false,
        AiProvider::CloudflareWorkersAi,
    );

    write_last_request_artifacts(
        tmp.path(),
        &profile,
        &profile.id,
        AiProvider::CloudflareWorkersAi,
        false,
        &messages,
        &tools,
        &request,
    );

    let raw = std::fs::read(tmp.path().join(".velocity").join("last_request.nda")).unwrap();
    let nda = String::from_utf8(crate::agent::crypto::open(
        tmp.path(),
        b"last_request",
        &raw,
    ))
    .unwrap();
    let json =
        std::fs::read_to_string(tmp.path().join(".velocity").join("last_request.json")).unwrap();
    assert!(nda.contains("last-request version 3"));
    assert!(nda.contains("field\tmodel\t@cf/example/chat"));
    assert!(json.contains("\"model\""));
}

#[allow(dead_code)]
#[derive(serde::Deserialize, serde::Serialize)]
struct ToolCallFunction {
    name: String,
    arguments: String,
}

#[allow(dead_code)]
#[derive(serde::Deserialize, serde::Serialize)]
struct ToolCall {
    id: String,
    function: ToolCallFunction,
}

#[test]
fn test_eager_merkle_compaction() {
    let mut long_content = String::new();
    long_content.push_str("fn test_function() {\n    println!(\"Hello\");\n}\nclass TestClass {}");
    for i in 0..100 {
        long_content.push_str(&format!("\n// Dummy line padding number {} to ensure we are well above the four thousand character compaction threshold.", i));
    }

    let original_messages = vec![
        ChatMessage {
            role: "assistant".to_string(),
            content: "Calling read_file".to_string(),
            name: None,
            tool_call_id: None,
            tool_calls: Some(serde_json::json!([{
                "id": "call_xyz",
                "function": {
                    "name": "read_file",
                    "arguments": "{}"
                }
            }])),
        },
        ChatMessage {
            role: "tool".to_string(),
            content: long_content,
            name: Some("read_file".to_string()),
            tool_call_id: Some("call_xyz".to_string()),
            tool_calls: None,
        },
        ChatMessage {
            role: "assistant".to_string(),
            content: "I have read the file.".to_string(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        },
    ];

    let compressed = compress_history(&original_messages, true);
    assert_eq!(compressed.len(), 3);
    assert_eq!(compressed[1].role, "tool");

    let content = &compressed[1].content;
    assert!(content.contains("compressed to optimize context"));
    assert!(content.contains("Merkle Hash:"));
    assert!(content.contains("fn test_function"));
    assert!(content.contains("class TestClass"));
}

#[test]
fn prose_read_file_stub_gets_excerpt_not_fake_declarations() {
    // read_file on a markdown doc that merely *mentions* code (a fenced
    // "fn read_at" example) must not be summarised as a declaration index:
    // the extracted "declarations" were prose artefacts the model then
    // believed were real symbols.
    let mut prose = String::from("# Storage Guide\n\nThis document explains the storage API.\n");
    prose.push_str("```rust\nfn read_at(offset: u64) -> Vec<u8> { load(offset) }\n```\n");
    while prose.len() < 5_000 {
        prose.push_str("The layer above handles caching, durability and compaction.\n");
    }

    let messages = vec![
        ChatMessage {
            role: "assistant".to_string(),
            content: "Let me read the guide.".to_string(),
            name: None,
            tool_call_id: None,
            tool_calls: Some(serde_json::json!([{
                "id": "call_doc",
                "function": {
                    "name": "read_file",
                    "arguments": "{\"relativeFilePath\":\"docs/STORAGE.md\"}"
                }
            }])),
        },
        ChatMessage {
            role: "tool".to_string(),
            content: prose,
            name: Some("read_file".to_string()),
            tool_call_id: Some("call_doc".to_string()),
            tool_calls: None,
        },
        ChatMessage {
            role: "assistant".to_string(),
            content: "Understood.".to_string(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        },
    ];

    let compressed = compress_history(&messages, true);
    let stub = &compressed[1].content;
    assert!(stub.contains("compressed to optimize context"), "{stub}");
    assert!(!stub.contains("Parsed Declarations"), "prose must not fake a symbol index: {stub}");
    assert!(!stub.contains("fn read_at"), "prose fence leaked into the stub: {stub}");
    assert!(stub.contains("Excerpt: # Storage Guide"), "{stub}");
    assert!(stub.contains("Call read_file again"), "{stub}");
}

#[test]
fn code_read_file_stub_still_indexes_declarations() {
    // The path check must not swallow the useful case: a real source file
    // keeps its declaration index in the compressed stub.
    let mut code = String::from("pub fn load_page(id: u64) -> Page {\n    fetch(id)\n}\n");
    while code.len() < 5_000 {
        code.push_str("fn helper_x() {\n    run();\n}\n");
    }

    let messages = vec![
        ChatMessage {
            role: "assistant".to_string(),
            content: String::new(),
            name: None,
            tool_call_id: None,
            tool_calls: Some(serde_json::json!([{
                "id": "call_code",
                "function": {
                    "name": "read_file",
                    "arguments": "{\"relativeFilePath\":\"src/pager.rs\"}"
                }
            }])),
        },
        ChatMessage {
            role: "tool".to_string(),
            content: code,
            name: Some("read_file".to_string()),
            tool_call_id: Some("call_code".to_string()),
            tool_calls: None,
        },
        ChatMessage {
            role: "assistant".to_string(),
            content: "Done.".to_string(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        },
    ];

    let compressed = compress_history(&messages, true);
    let stub = &compressed[1].content;
    assert!(stub.contains("Parsed Declarations"), "code stub must keep the index: {stub}");
    assert!(stub.contains("fn load_page") || stub.contains("fn helper_x"), "{stub}");
}

#[test]
fn mid_slice_truncation_snaps_to_char_boundaries() {
    // The >12k fallback cut at fixed byte offsets panicked whenever a
    // multi-byte character happened to straddle the 6,000-byte mark.
    let mut giant = "a".repeat(5_999);
    giant.push('é'); // occupies bytes 5999..6001, straddling the head cut
    giant.push_str(&"b".repeat(7_000));
    assert!(giant.len() > 12_000);

    let messages = vec![ChatMessage {
        role: "tool".to_string(),
        content: giant,
        name: Some("read_file".to_string()),
        tool_call_id: Some("call_giant_utf8".to_string()),
        tool_calls: None,
    }];

    let compressed = compress_history(&messages, true);
    assert!(compressed[0]
        .content
        .contains("Truncated middle output of 'read_file'"));
}

#[test]
fn test_fallback_provider_resolution() {
    assert_eq!(
        fallback_provider(AiProvider::CloudflareWorkersAi),
        AiProvider::OpenRouter
    );
    assert_eq!(
        fallback_provider(AiProvider::OpenRouter),
        AiProvider::AzureOpenAi
    );
    assert_eq!(
        fallback_provider(AiProvider::AzureOpenAi),
        AiProvider::LocalOllama
    );
    assert_eq!(
        fallback_provider(AiProvider::LocalOllama),
        AiProvider::CloudflareWorkersAi
    );
    assert_eq!(
        default_provider_model(AiProvider::CloudflareWorkersAi),
        "@cf/moonshotai/kimi-k2.7-code"
    );
    assert_eq!(
        default_provider_model(AiProvider::OpenRouter),
        "tencent/hy3:free"
    );
    assert_eq!(default_provider_model(AiProvider::AzureOpenAi), "gpt-4o");
    assert_eq!(
        default_provider_model(AiProvider::LocalOllama),
        "qwen2.5-coder:0.5b"
    );
}

#[test]
fn test_compress_history_truncates_giant_uncompressed_tool_output() {
    let mut giant_output = String::with_capacity(15_000);
    for i in 0..1500 {
        giant_output.push_str(&format!("Line {:04}: sample text content.\n", i));
    }

    let messages = vec![ChatMessage {
        role: "tool".to_string(),
        content: giant_output,
        name: Some("grep_search".to_string()),
        tool_call_id: Some("call_giant".to_string()),
        tool_calls: None,
    }];

    let compressed = compress_history(&messages, true);
    assert_eq!(compressed.len(), 1);
    assert!(compressed[0]
        .content
        .contains("Truncated middle output of 'grep_search'"));
    assert!(compressed[0].content.len() < 13_000);
}

#[test]
fn test_compress_history_converts_orphan_tool_messages() {
    let orphan_tool = ChatMessage {
        role: "tool".to_string(),
        content: "Success output".to_string(),
        name: Some("read_file".to_string()),
        tool_call_id: Some("call_orphan_123".to_string()),
        tool_calls: None,
    };

    let compressed = compress_history(&[orphan_tool], true);
    assert_eq!(compressed.len(), 1);
    assert_eq!(compressed[0].role, "user");
    assert!(compressed[0]
        .content
        .contains("[Tool result for 'read_file']: Success output"));
    assert!(compressed[0].tool_call_id.is_none());
    assert!(compressed[0].name.is_none());
}

fn antigravity_prompt_with_docs() -> ChatMessage {
    ChatMessage {
        role: "system".to_string(),
        content: "You are Antigravity, a high-performance agent running directly in V.E.L.O.C.I.T.Y.-IDE workspace. \
                  Mode: Coder. Workspace: demo.\n\n\
                  ## Available Tools\nCall tools using this exact syntax.\n### read_file\nReads a file.\n\n\
                  ## Recalled Context (from past sessions)\n- [k1] remembered fact"
            .to_string(),
        name: None,
        tool_call_id: None,
        tool_calls: None,
    }
}

#[test]
fn test_compress_history_strips_inline_docs_for_native_tool_models() {
    // For tool-capable models the schemas ride in the request's `tools` array,
    // so the inline "## Available Tools" block must be stripped from the system
    // prompt while the runtime-injected sections survive.
    let messages = vec![antigravity_prompt_with_docs()];
    let compressed = compress_history(&messages, true);
    let sys = compressed.iter().find(|m| m.role == "system").unwrap();
    assert!(!sys.content.contains("## Available Tools"));
    assert!(sys.content.contains("Mode: Coder"));
    assert!(sys
        .content
        .contains("## Recalled Context (from past sessions)"));
    assert!(sys.content.contains("[k1] remembered fact"));
}

#[test]
fn test_compress_history_rebuilds_inline_docs_for_inline_tool_models() {
    // Models without native tool calling must keep receiving the full inline
    // tool catalog in the system prompt.
    let messages = vec![antigravity_prompt_with_docs()];
    let compressed = compress_history(&messages, false);
    let sys = compressed.iter().find(|m| m.role == "system").unwrap();
    assert!(sys.content.contains("## Available Tools"));
    // Fresh catalog content (built from the live registry), not the stale copy.
    assert!(sys.content.contains("read_file"));
    assert!(sys
        .content
        .contains("## Recalled Context (from past sessions)"));
}

#[test]
fn test_compress_history_dedupes_new_velocity_identity_prompt() {
    // The shipped prompt no longer says "Antigravity"; dedupe must follow the
    // new identity prefix (and still honour the legacy one, tested above).
    let mut msg = antigravity_prompt_with_docs();
    msg.content = msg.content.replace(
        "You are Antigravity, a high-performance agent running directly in V.E.L.O.C.I.T.Y.-IDE workspace.",
        "You are Velocity, the native AI agent of V.E.L.O.C.I.T.Y.-IDE, running directly in this workspace.");
    let compressed = compress_history(&[msg], true);
    let sys = compressed.iter().find(|m| m.role == "system").unwrap();
    assert!(sys
        .content
        .starts_with("You are Velocity, the native AI agent"));
    assert!(!sys.content.contains("## Available Tools"));
    assert!(sys
        .content
        .contains("## Recalled Context (from past sessions)"));
}

#[test]
fn test_system_prompt_base_carries_grounding_clause() {
    // Regression for the fabricated-summary defect: a model with the full
    // source verbatim in context still invented "file types, directories and
    // structures" that were not in it. The shared prompt base must forbid
    // inventing content and bless sparse-but-true output.
    assert!(SYSTEM_PROMPT_BASE.starts_with("You are Velocity, the native AI agent"));
    assert!(SYSTEM_PROMPT_BASE.contains("never invent structure"));
    assert!(SYSTEM_PROMPT_BASE.contains("an accurate sparse summary beats a polished fabricated one"));
    // The old polish-pressure wording that encouraged padding must not return.
    assert!(!SYSTEM_PROMPT_BASE.contains("high-quality responses"));
    // The dedupe/legacy-accept marker matches the constant's prefix, so
    // restored chat logs built from it keep getting per-request doc stripping.
    let mut sys = ChatMessage {
        role: "system".to_string(),
        content: SYSTEM_PROMPT_BASE.to_string(),
        name: None,
        tool_call_id: None,
        tool_calls: None,
    };
    sys.content.push_str("\n\n## Available Tools\n### read_file\nold copy\n");
    let compressed = compress_history(&[sys], true);
    let out = compressed.iter().find(|m| m.role == "system").unwrap();
    assert!(!out.content.contains("## Available Tools"));
}

#[test]
fn test_mission_anchor_survives_base_truncation() {
    // The first substantive user message carries the mission brief and must be
    // preserved verbatim even when the history blows past the character budget
    // and everything else collapses into the rolling summary.
    let mission = "You must complete this exact mission: step one, do the thing; \
                   step two, write the report file. Repeat neither from memory. "
        .repeat(10);
    let mut messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: "You are a helpful assistant.".to_string(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        },
        ChatMessage {
            role: "user".to_string(),
            content: mission.clone(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        },
    ];
    // Blow past the 200k-char base budget with plain filler.
    for i in 0..12 {
        messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: format!("filler {} {}", i, "z".repeat(25_000)),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        });
    }

    let compressed = compress_history(&messages, true);
    assert!(
        compressed.iter().any(|m| m.content == mission),
        "mission brief must survive truncation verbatim"
    );
    assert!(
        compressed.len() < messages.len(),
        "filler should have been truncated away"
    );
}
