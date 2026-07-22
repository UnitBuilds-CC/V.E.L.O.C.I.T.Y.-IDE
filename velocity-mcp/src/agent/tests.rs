use super::executor::utils::*;
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
    let transcript =
        std::fs::read_to_string(tmp.path().join(".velocity").join("transcript.nda")).unwrap();
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
    let sitemap =
        std::fs::read_to_string(tmp.path().join(".velocity").join("sitemap.nda")).unwrap();
    assert!(sitemap.starts_with("sitemap version 2\n"));
    assert!(sitemap.contains("entry_count 4\n"));
    assert!(sitemap.contains("\tdir\tsrc\t-"));
    assert!(sitemap.contains("\tdir\tsrc\\\\nested\t-"));
    assert!(sitemap.contains("\tfile\tsrc\\\\main.rs\t"));
    assert!(sitemap.contains("\tfile\tsrc\\\\nested\\\\lib.rs\t"));
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
    let chatlogs =
        std::fs::read_to_string(tmp.path().join(".velocity").join("chatlogs.nda")).unwrap();
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
    let handover =
        std::fs::read_to_string(tmp.path().join(".velocity").join("handover.nda")).unwrap();
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
    let changelog =
        std::fs::read_to_string(tmp.path().join(".velocity").join("changelog.nda")).unwrap();
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
    let changelog = std::fs::read_to_string(velocity_dir.join("changelog.nda")).unwrap();
    assert!(changelog.starts_with("changelog version 2\n"));
    assert!(changelog.contains("entry_count 2\n"));
    assert!(changelog.contains("\tsrc/old.rs\tupdated"));
    assert!(changelog.contains("\tsrc/new.rs\tcreated"));
}

#[test]
fn test_sanitize_chat_token_removes_tags() {
    let input = "Hello there </tool_call>\n</tool_call>\n<parameter=path>src/main.rs</parameter> checking.";
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

    let nda =
        std::fs::read_to_string(tmp.path().join(".velocity").join("last_request.nda")).unwrap();
    let json = std::fs::read_to_string(tmp.path().join(".velocity").join("last_request.json"))
        .unwrap();
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
    long_content
        .push_str("fn test_function() {\n    println!(\"Hello\");\n}\nclass TestClass {}");
    for i in 0..100 {
        long_content.push_str(&format!("\n// Dummy line padding number {} to ensure we are well above the one thousand character compaction threshold.", i));
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
    assert_eq!(
        default_provider_model(AiProvider::AzureOpenAi),
        "gpt-4o"
    );
    assert_eq!(
        default_provider_model(AiProvider::LocalOllama),
        "llama3.2"
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
    assert!(compressed[0].content.contains("Truncated middle output of 'grep_search'"));
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
    assert!(compressed[0].content.contains("[Tool result for 'read_file']: Success output"));
    assert!(compressed[0].tool_call_id.is_none());
    assert!(compressed[0].name.is_none());
}
