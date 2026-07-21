use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Specificity {
    pub inline: usize,
    pub ids: usize,
    pub classes_attrs_pseudos: usize,
    pub tags_elements: usize,
}

impl Specificity {
    pub fn new(inline: usize, ids: usize, classes: usize, tags: usize) -> Self {
        Self {
            inline,
            ids,
            classes_attrs_pseudos: classes,
            tags_elements: tags,
        }
    }

    pub fn compute(selector: &str) -> Self {
        let selector = selector.trim();
        let mut ids = 0;
        let mut classes = 0;
        let mut tags = 0;

        for part in selector.split_whitespace() {
            if part.contains('#') {
                ids += part.matches('#').count();
            }
            if part.contains('.') {
                classes += part.matches('.').count();
            }
            if part.contains('[') {
                classes += part.matches('[').count();
            }
            if part.contains(':') {
                classes += part.matches(':').count();
            }
            let clean = part.split('#').next().unwrap_or(part).split('.').next().unwrap_or(part).split('[').next().unwrap_or(part);
            if !clean.is_empty() && !clean.starts_with('#') && !clean.starts_with('.') && !clean.starts_with('[') {
                tags += 1;
            }
        }

        Self::new(0, ids, classes, tags)
    }
}

#[derive(Debug, Clone)]
pub struct CssRule {
    pub selector: String,
    pub specificity: Specificity,
    pub declarations: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct StyleCascader {
    pub rules: Vec<CssRule>,
}

impl StyleCascader {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    pub fn add_rule(&mut self, selector: &str, declarations: HashMap<String, String>) {
        let spec = Specificity::compute(selector);
        self.rules.push(CssRule {
            selector: selector.to_string(),
            specificity: spec,
            declarations,
        });
    }

    pub fn compute_computed_style(&self, selector_match_fn: impl Fn(&str) -> bool) -> HashMap<String, String> {
        let mut computed = HashMap::new();
        let mut applicable_rules: Vec<&CssRule> = self.rules.iter().filter(|r| selector_match_fn(&r.selector)).collect();

        // Sort rules by specificity ascending so higher specificity overwrites lower
        applicable_rules.sort_by(|a, b| a.specificity.cmp(&b.specificity));

        for rule in applicable_rules {
            for (prop, val) in &rule.declarations {
                computed.insert(prop.clone(), val.clone());
            }
        }

        computed
    }
}
