use std::collections::HashMap;

/// Lifecycle callback types for custom elements.
#[derive(Debug, Clone, PartialEq)]
pub enum LifecycleCallback {
    ConnectedCallback,
    DisconnectedCallback,
    AdoptedCallback,
    AttributeChangedCallback { name: String, old_value: Option<String>, new_value: Option<String> },
}

/// A custom element definition with lifecycle callbacks.
#[derive(Debug, Clone)]
pub struct CustomElementDefinition {
    pub name: String,
    pub class_name: String,
    pub extends_tag: Option<String>,
    /// Observed attributes that trigger attributeChangedCallback.
    pub observed_attributes: Vec<String>,
    /// Whether the element has been constructed.
    pub is_constructed: bool,
    /// Pending lifecycle callbacks.
    pub pending_callbacks: Vec<LifecycleCallback>,
    /// Shadow root mode (if the element creates a shadow root).
    pub shadow_root_mode: Option<String>,
}

/// Custom element registry (window.customElements).
pub struct CustomElementRegistry {
    pub definitions: HashMap<String, CustomElementDefinition>,
    /// When-defined promises (element name -> list of pending resolves).
    pub when_defined: HashMap<String, Vec<String>>,
}

impl Default for CustomElementRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl CustomElementRegistry {
    pub fn new() -> Self {
        Self {
            definitions: HashMap::new(),
            when_defined: HashMap::new(),
        }
    }

    /// Define a new custom element.
    pub fn define(&mut self, name: &str, class_name: &str, extends_tag: Option<&str>) -> Result<(), String> {
        if !name.contains('-') {
            return Err("CustomElementError: Custom element tag name must contain a hyphen".to_string());
        }
        if self.definitions.contains_key(name) {
            return Err(format!("CustomElementError: '{}' is already defined", name));
        }
        // Reserved names
        let reserved = ["annotation-xml", "color-profile", "font-face", "font-face-src",
            "font-face-uri", "font-face-format", "font-face-name", "missing-glyph"];
        if reserved.contains(&name.to_lowercase().as_str()) {
            return Err(format!("CustomElementError: '{}' is a reserved name", name));
        }

        self.definitions.insert(name.to_string(), CustomElementDefinition {
            name: name.to_string(),
            class_name: class_name.to_string(),
            extends_tag: extends_tag.map(|s| s.to_string()),
            observed_attributes: Vec::new(),
            is_constructed: false,
            pending_callbacks: Vec::new(),
            shadow_root_mode: None,
        });

        // Resolve any whenDefined promises
        if let Some(resolves) = self.when_defined.remove(name) {
            let _ = resolves; // In a real impl, these would resolve JS promises
        }

        Ok(())
    }

    /// Get a custom element definition by name.
    pub fn get(&self, name: &str) -> Option<&CustomElementDefinition> {
        self.definitions.get(name)
    }

    /// Set observed attributes for a custom element.
    pub fn set_observed_attributes(&mut self, name: &str, attrs: Vec<String>) -> Result<(), String> {
        let def = self.definitions.get_mut(name)
            .ok_or_else(|| format!("CustomElementError: '{}' not defined", name))?;
        def.observed_attributes = attrs;
        Ok(())
    }

    /// Queue a connectedCallback for an element.
    pub fn enqueue_connected(&mut self, name: &str) -> Result<(), String> {
        let def = self.definitions.get_mut(name)
            .ok_or_else(|| format!("CustomElementError: '{}' not defined", name))?;
        def.pending_callbacks.push(LifecycleCallback::ConnectedCallback);
        Ok(())
    }

    /// Queue a disconnectedCallback for an element.
    pub fn enqueue_disconnected(&mut self, name: &str) -> Result<(), String> {
        let def = self.definitions.get_mut(name)
            .ok_or_else(|| format!("CustomElementError: '{}' not defined", name))?;
        def.pending_callbacks.push(LifecycleCallback::DisconnectedCallback);
        Ok(())
    }

    /// Queue an attributeChangedCallback if the attribute is observed.
    pub fn enqueue_attribute_changed(
        &mut self,
        name: &str,
        attr_name: &str,
        old_value: Option<&str>,
        new_value: Option<&str>,
    ) -> Result<(), String> {
        let def = self.definitions.get_mut(name)
            .ok_or_else(|| format!("CustomElementError: '{}' not defined", name))?;
        if !def.observed_attributes.contains(&attr_name.to_string()) {
            return Ok(()); // Not observed, skip
        }
        def.pending_callbacks.push(LifecycleCallback::AttributeChangedCallback {
            name: attr_name.to_string(),
            old_value: old_value.map(|s| s.to_string()),
            new_value: new_value.map(|s| s.to_string()),
        });
        Ok(())
    }

    /// Drain and return pending lifecycle callbacks for an element.
    pub fn drain_callbacks(&mut self, name: &str) -> Vec<LifecycleCallback> {
        self.definitions.get_mut(name)
            .map(|def| std::mem::take(&mut def.pending_callbacks))
            .unwrap_or_default()
    }

    /// Check if a custom element is defined.
    pub fn is_defined(&self, name: &str) -> bool {
        self.definitions.contains_key(name)
    }

    /// Register a whenDefined callback.
    pub fn when_defined(&mut self, name: &str, resolve_id: &str) {
        if self.definitions.contains_key(name) {
            return; // Already defined, resolve immediately
        }
        self.when_defined
            .entry(name.to_string())
            .or_default()
            .push(resolve_id.to_string());
    }

    /// Upgrade an existing element to a custom element definition.
    pub fn upgrade_element(&mut self, name: &str) -> Result<bool, String> {
        if !self.definitions.contains_key(name) {
            return Err(format!("CustomElementError: '{}' not defined", name));
        }
        let def = self.definitions.get_mut(name).unwrap();
        if def.is_constructed {
            return Ok(false); // Already upgraded
        }
        def.is_constructed = true;
        def.pending_callbacks.push(LifecycleCallback::ConnectedCallback);
        Ok(true)
    }

    /// Get all registered custom element names.
    pub fn registered_names(&self) -> Vec<String> {
        self.definitions.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_define_valid() {
        let mut reg = CustomElementRegistry::new();
        assert!(reg.define("my-element", "MyElement", None).is_ok());
        assert!(reg.is_defined("my-element"));
    }

    #[test]
    fn test_define_no_hyphen() {
        let mut reg = CustomElementRegistry::new();
        assert!(reg.define("myelement", "MyElement", None).is_err());
    }

    #[test]
    fn test_define_duplicate() {
        let mut reg = CustomElementRegistry::new();
        reg.define("my-element", "MyElement", None).unwrap();
        assert!(reg.define("my-element", "MyElement2", None).is_err());
    }

    #[test]
    fn test_define_reserved() {
        let mut reg = CustomElementRegistry::new();
        assert!(reg.define("annotation-xml", "Foo", None).is_err());
    }

    #[test]
    fn test_observed_attributes() {
        let mut reg = CustomElementRegistry::new();
        reg.define("my-el", "MyEl", None).unwrap();
        reg.set_observed_attributes("my-el", vec!["color".to_string(), "size".to_string()]).unwrap();
        let def = reg.get("my-el").unwrap();
        assert_eq!(def.observed_attributes, vec!["color", "size"]);
    }

    #[test]
    fn test_lifecycle_callbacks() {
        let mut reg = CustomElementRegistry::new();
        reg.define("my-el", "MyEl", None).unwrap();
        reg.enqueue_connected("my-el").unwrap();
        reg.enqueue_disconnected("my-el").unwrap();
        let callbacks = reg.drain_callbacks("my-el");
        assert_eq!(callbacks.len(), 2);
        assert_eq!(callbacks[0], LifecycleCallback::ConnectedCallback);
        assert_eq!(callbacks[1], LifecycleCallback::DisconnectedCallback);
    }

    #[test]
    fn test_attribute_changed_observed() {
        let mut reg = CustomElementRegistry::new();
        reg.define("my-el", "MyEl", None).unwrap();
        reg.set_observed_attributes("my-el", vec!["color".to_string()]).unwrap();
        reg.enqueue_attribute_changed("my-el", "color", Some("red"), Some("blue")).unwrap();
        let callbacks = reg.drain_callbacks("my-el");
        assert_eq!(callbacks.len(), 1);
        match &callbacks[0] {
            LifecycleCallback::AttributeChangedCallback { name, old_value, new_value } => {
                assert_eq!(name, "color");
                assert_eq!(old_value.as_deref(), Some("red"));
                assert_eq!(new_value.as_deref(), Some("blue"));
            }
            _ => panic!("Expected AttributeChangedCallback"),
        }
    }

    #[test]
    fn test_attribute_changed_not_observed() {
        let mut reg = CustomElementRegistry::new();
        reg.define("my-el", "MyEl", None).unwrap();
        reg.set_observed_attributes("my-el", vec!["color".to_string()]).unwrap();
        reg.enqueue_attribute_changed("my-el", "size", None, Some("large")).unwrap();
        let callbacks = reg.drain_callbacks("my-el");
        assert_eq!(callbacks.len(), 0); // 'size' not observed
    }

    #[test]
    fn test_upgrade_element() {
        let mut reg = CustomElementRegistry::new();
        reg.define("my-el", "MyEl", None).unwrap();
        assert!(reg.upgrade_element("my-el").unwrap());
        assert!(!reg.upgrade_element("my-el").unwrap()); // already upgraded
    }

    #[test]
    fn test_when_defined() {
        let mut reg = CustomElementRegistry::new();
        reg.when_defined("my-el", "resolve_1");
        assert_eq!(reg.when_defined.get("my-el").unwrap().len(), 1);
        reg.define("my-el", "MyEl", None).unwrap();
        assert!(!reg.when_defined.contains_key("my-el")); // resolved
    }

    #[test]
    fn test_registered_names() {
        let mut reg = CustomElementRegistry::new();
        reg.define("a-el", "A", None).unwrap();
        reg.define("b-el", "B", None).unwrap();
        let names = reg.registered_names();
        assert_eq!(names.len(), 2);
    }
}
