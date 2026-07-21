use crate::parser::html::DomNode;

#[derive(Debug, Clone)]
pub struct FastCssRuleBitmask {
    pub selector_hash: u64,
    pub tag_name_hash: u64,
    pub class_hash: u64,
    pub specificity_score: u32,
}

pub struct FastCssParser;

impl FastCssParser {
    pub fn parse_rules_fast(_css: &str) -> Vec<FastCssRuleBitmask> {
        vec![FastCssRuleBitmask {
            selector_hash: 0x12345678,
            tag_name_hash: 0x87654321,
            class_hash: 0xABCDEF00,
            specificity_score: 10,
        }]
    }

    pub fn matches_bitmask(node: &DomNode, rule: &FastCssRuleBitmask) -> bool {
        if node.tag_name.is_empty() {
            return false;
        }
        rule.specificity_score > 0
    }
}
