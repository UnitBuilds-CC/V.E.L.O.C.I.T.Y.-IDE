pub mod browser;
pub mod company;
pub mod drone;
pub mod generation;
pub mod instances;
pub mod rules;
pub mod system;
pub mod team;
pub mod wa;

use super::types::Tool;

pub fn get_tools() -> Vec<Tool> {
    let mut tools = system::get_system_tools();
    tools.extend(rules::get_rules_tools());
    tools.extend(browser::get_browser_tools());
    tools.extend(wa::get_wa_tools());
    tools.extend(team::get_team_tools());
    tools.extend(company::get_company_tools());
    tools.extend(drone::get_drone_tools());
    tools.extend(instances::get_instance_tools());
    tools.extend(generation::get_generation_tools());
    // Include dynamically registered custom tools.
    let root = std::env::current_dir().unwrap_or_default();
    tools.extend(super::custom_tools::list_tools(&root));
    tools
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Walk a schema and flag every `"items"` that is not itself a schema
    /// object, recording the JSON-pointer-style path the backend would report.
    fn bad_items(v: &serde_json::Value, path: &str, out: &mut Vec<String>) {
        match v {
            serde_json::Value::Object(map) => {
                for (k, child) in map {
                    let p = format!("{path}/{k}");
                    if k == "items" && !child.is_object() {
                        out.push(format!("{p} -> {child}"));
                    }
                    bad_items(child, &p, out);
                }
            }
            serde_json::Value::Array(arr) => {
                for (i, child) in arr.iter().enumerate() {
                    bad_items(child, &format!("{path}/{i}"), out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn every_tool_array_declares_items_as_a_schema_object() {
        // `"items": ["read", "write"]` is not a schema. Permissive frontends
        // (OpenAI, Anthropic) ignore the malformed `items` and the bug stays
        // invisible; llama.cpp's tool-grammar compiler validates the advertised
        // schema and answers HTTP 500 for the *whole request* —
        // "JSON schema error at #/properties/capabilities/items/0: schema must be
        // an object" — so every turn of every worker fails before the model runs.
        // Validating all tools here keeps one lax provider from hiding it again.
        let mut offenders = Vec::new();
        for tool in get_tools() {
            let mut local = Vec::new();
            bad_items(&tool.input_schema, "", &mut local);
            for site in local {
                offenders.push(format!("{}: {}", tool.name, site));
            }
        }
        assert!(
            offenders.is_empty(),
            "tool schemas with a non-object `items` (strict backends 500 the \
             entire request): {offenders:?}"
        );
    }
}
