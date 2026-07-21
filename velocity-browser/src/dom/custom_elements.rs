use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct CustomElementDefinition {
    pub name: String,
    pub class_name: String,
    pub extends_tag: Option<String>,
}

pub struct CustomElementRegistry {
    pub definitions: HashMap<String, CustomElementDefinition>,
}

impl CustomElementRegistry {
    pub fn new() -> Self {
        Self { definitions: HashMap::new() }
    }

    pub fn define(&mut self, name: &str, class_name: &str, extends_tag: Option<&str>) -> Result<(), String> {
        if !name.contains('-') {
            return Err("CustomElementError: Custom element tag name must contain a hyphen".to_string());
        }
        self.definitions.insert(name.to_string(), CustomElementDefinition {
            name: name.to_string(),
            class_name: class_name.to_string(),
            extends_tag: extends_tag.map(|s| s.to_string()),
        });
        Ok(())
    }
}
