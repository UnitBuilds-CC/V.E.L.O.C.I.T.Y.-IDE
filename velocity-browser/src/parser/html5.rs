use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Html5State {
    Data,
    TagOpen,
    EndTagOpen,
    TagName,
    BeforeAttributeName,
    AttributeName,
    AfterAttributeName,
    BeforeAttributeValue,
    AttributeValueDoubleQuoted,
    AttributeValueSingleQuoted,
    AttributeValueUnquoted,
    AfterAttributeValueQuoted,
    SelfClosingStartTag,
    Comment,
    Doctype,
    RawText,
}

#[derive(Debug, Clone)]
pub struct Html5Token {
    pub kind: TokenKind,
    pub name: String,
    pub attributes: HashMap<String, String>,
    pub data: String,
    pub self_closing: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    StartTag,
    EndTag,
    Comment,
    Doctype,
    Character,
    Eof,
}

pub struct Html5Tokenizer {
    chars: Vec<char>,
    pos: usize,
    state: Html5State,
}

impl Html5Tokenizer {
    pub fn new(input: &str) -> Self {
        Self {
            chars: input.chars().collect(),
            pos: 0,
            state: Html5State::Data,
        }
    }

    pub fn tokenize(&mut self) -> Vec<Html5Token> {
        let mut tokens = Vec::new();
        let len = self.chars.len();

        let mut current_tag = String::new();
        let mut current_attr_key = String::new();
        let mut current_attr_val = String::new();
        let mut current_attrs = HashMap::new();
        let mut current_data = String::new();
        let mut self_closing = false;
        let mut is_end_tag = false;

        while self.pos < len {
            let ch = self.chars[self.pos];

            match self.state {
                Html5State::Data => {
                    if ch == '<' {
                        if !current_data.trim().is_empty() {
                            tokens.push(Html5Token {
                                kind: TokenKind::Character,
                                name: String::new(),
                                attributes: HashMap::new(),
                                data: current_data.trim().to_string(),
                                self_closing: false,
                            });
                            current_data.clear();
                        }
                        self.state = Html5State::TagOpen;
                    } else {
                        current_data.push(ch);
                    }
                }
                Html5State::TagOpen => {
                    if ch == '/' {
                        is_end_tag = true;
                        self.state = Html5State::EndTagOpen;
                    } else if ch == '!' {
                        self.state = Html5State::Comment;
                    } else if ch.is_alphabetic() {
                        is_end_tag = false;
                        current_tag.clear();
                        current_tag.push(ch.to_ascii_lowercase());
                        current_attrs.clear();
                        self.state = Html5State::TagName;
                    } else {
                        self.state = Html5State::Data;
                    }
                }
                Html5State::EndTagOpen => {
                    if ch.is_alphabetic() {
                        current_tag.clear();
                        current_tag.push(ch.to_ascii_lowercase());
                        self.state = Html5State::TagName;
                    } else {
                        self.state = Html5State::Data;
                    }
                }
                Html5State::TagName => {
                    if ch.is_whitespace() {
                        self.state = Html5State::BeforeAttributeName;
                    } else if ch == '/' {
                        self_closing = true;
                        self.state = Html5State::SelfClosingStartTag;
                    } else if ch == '>' {
                        tokens.push(Html5Token {
                            kind: if is_end_tag { TokenKind::EndTag } else { TokenKind::StartTag },
                            name: current_tag.clone(),
                            attributes: current_attrs.clone(),
                            data: String::new(),
                            self_closing,
                        });
                        self_closing = false;
                        self.state = Html5State::Data;
                    } else {
                        current_tag.push(ch.to_ascii_lowercase());
                    }
                }
                Html5State::BeforeAttributeName => {
                    if ch.is_whitespace() {
                        // Continue
                    } else if ch == '>' {
                        tokens.push(Html5Token {
                            kind: if is_end_tag { TokenKind::EndTag } else { TokenKind::StartTag },
                            name: current_tag.clone(),
                            attributes: current_attrs.clone(),
                            data: String::new(),
                            self_closing,
                        });
                        self.state = Html5State::Data;
                    } else if ch == '/' {
                        self_closing = true;
                        self.state = Html5State::SelfClosingStartTag;
                    } else {
                        current_attr_key.clear();
                        current_attr_key.push(ch.to_ascii_lowercase());
                        self.state = Html5State::AttributeName;
                    }
                }
                Html5State::AttributeName => {
                    if ch == '=' {
                        self.state = Html5State::BeforeAttributeValue;
                    } else if ch.is_whitespace() || ch == '>' || ch == '/' {
                        current_attrs.insert(current_attr_key.clone(), String::new());
                        if ch == '>' {
                            tokens.push(Html5Token {
                                kind: if is_end_tag { TokenKind::EndTag } else { TokenKind::StartTag },
                                name: current_tag.clone(),
                                attributes: current_attrs.clone(),
                                data: String::new(),
                                self_closing,
                            });
                            self.state = Html5State::Data;
                        } else {
                            self.state = Html5State::BeforeAttributeName;
                        }
                    } else {
                        current_attr_key.push(ch.to_ascii_lowercase());
                    }
                }
                Html5State::BeforeAttributeValue => {
                    if ch.is_whitespace() {
                        // Skip
                    } else if ch == '"' {
                        current_attr_val.clear();
                        self.state = Html5State::AttributeValueDoubleQuoted;
                    } else if ch == '\'' {
                        current_attr_val.clear();
                        self.state = Html5State::AttributeValueSingleQuoted;
                    } else {
                        current_attr_val.clear();
                        current_attr_val.push(ch);
                        self.state = Html5State::AttributeValueUnquoted;
                    }
                }
                Html5State::AttributeValueDoubleQuoted => {
                    if ch == '"' {
                        current_attrs.insert(current_attr_key.clone(), current_attr_val.clone());
                        self.state = Html5State::AfterAttributeValueQuoted;
                    } else {
                        current_attr_val.push(ch);
                    }
                }
                Html5State::AttributeValueSingleQuoted => {
                    if ch == '\'' {
                        current_attrs.insert(current_attr_key.clone(), current_attr_val.clone());
                        self.state = Html5State::AfterAttributeValueQuoted;
                    } else {
                        current_attr_val.push(ch);
                    }
                }
                Html5State::AttributeValueUnquoted => {
                    if ch.is_whitespace() || ch == '>' {
                        current_attrs.insert(current_attr_key.clone(), current_attr_val.clone());
                        if ch == '>' {
                            tokens.push(Html5Token {
                                kind: if is_end_tag { TokenKind::EndTag } else { TokenKind::StartTag },
                                name: current_tag.clone(),
                                attributes: current_attrs.clone(),
                                data: String::new(),
                                self_closing,
                            });
                            self.state = Html5State::Data;
                        } else {
                            self.state = Html5State::BeforeAttributeName;
                        }
                    } else {
                        current_attr_val.push(ch);
                    }
                }
                Html5State::AfterAttributeValueQuoted => {
                    if ch == '>' {
                        tokens.push(Html5Token {
                            kind: if is_end_tag { TokenKind::EndTag } else { TokenKind::StartTag },
                            name: current_tag.clone(),
                            attributes: current_attrs.clone(),
                            data: String::new(),
                            self_closing,
                        });
                        self.state = Html5State::Data;
                    } else {
                        self.state = Html5State::BeforeAttributeName;
                    }
                }
                Html5State::SelfClosingStartTag => {
                    if ch == '>' {
                        tokens.push(Html5Token {
                            kind: TokenKind::StartTag,
                            name: current_tag.clone(),
                            attributes: current_attrs.clone(),
                            data: String::new(),
                            self_closing: true,
                        });
                        self.state = Html5State::Data;
                    }
                }
                _ => {
                    self.state = Html5State::Data;
                }
            }
            self.pos += 1;
        }

        if !current_data.trim().is_empty() {
            tokens.push(Html5Token {
                kind: TokenKind::Character,
                name: String::new(),
                attributes: HashMap::new(),
                data: current_data.trim().to_string(),
                self_closing: false,
            });
        }

        tokens
    }
}

impl Html5Token {
    /// Returns true if this is a start tag with the given name.
    pub fn is_start_tag(&self, name: &str) -> bool {
        self.kind == TokenKind::StartTag && self.name.eq_ignore_ascii_case(name)
    }

    /// Returns true if this is an end tag with the given name.
    pub fn is_end_tag(&self, name: &str) -> bool {
        self.kind == TokenKind::EndTag && self.name.eq_ignore_ascii_case(name)
    }

    /// Get an attribute value by key, if present.
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attributes.get(key).map(|s| s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_start_end_tag() {
        let mut tok = Html5Tokenizer::new("<div></div>");
        let tokens = tok.tokenize();
        assert_eq!(tokens.len(), 2);
        assert!(tokens[0].is_start_tag("div"));
        assert!(tokens[1].is_end_tag("div"));
    }

    #[test]
    fn test_self_closing_tag() {
        let mut tok = Html5Tokenizer::new("<br/>");
        let tokens = tok.tokenize();
        assert_eq!(tokens.len(), 1);
        assert!(tokens[0].is_start_tag("br"));
        assert!(tokens[0].self_closing);
    }

    #[test]
    fn test_attributes_double_quoted() {
        let mut tok = Html5Tokenizer::new(r#"<input type="text" name="user">"#);
        let tokens = tok.tokenize();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].attr("type"), Some("text"));
        assert_eq!(tokens[0].attr("name"), Some("user"));
    }

    #[test]
    fn test_attributes_single_quoted() {
        let mut tok = Html5Tokenizer::new("<input type='hidden'>");
        let tokens = tok.tokenize();
        assert_eq!(tokens[0].attr("type"), Some("hidden"));
    }

    #[test]
    fn test_attributes_unquoted() {
        let mut tok = Html5Tokenizer::new("<input type=text>");
        let tokens = tok.tokenize();
        assert_eq!(tokens[0].attr("type"), Some("text"));
    }

    #[test]
    fn test_text_content() {
        let mut tok = Html5Tokenizer::new("<p>Hello world</p>");
        let tokens = tok.tokenize();
        assert!(tokens.len() >= 2);
        let text_token = tokens.iter().find(|t| t.kind == TokenKind::Character);
        assert!(text_token.is_some());
        assert_eq!(text_token.unwrap().data, "Hello world");
    }

    #[test]
    fn test_tag_name_lowercased() {
        let mut tok = Html5Tokenizer::new("<DIV></DIV>");
        let tokens = tok.tokenize();
        assert_eq!(tokens[0].name, "div");
        assert_eq!(tokens[1].name, "div");
    }

    #[test]
    fn test_attribute_name_lowercased() {
        let mut tok = Html5Tokenizer::new(r#"<div CLASS="foo">"#);
        let tokens = tok.tokenize();
        assert!(tokens[0].attributes.contains_key("class"));
    }

    #[test]
    fn test_multiple_tags() {
        let mut tok = Html5Tokenizer::new("<div><span></span></div>");
        let tokens = tok.tokenize();
        assert_eq!(tokens.len(), 4);
        assert!(tokens[0].is_start_tag("div"));
        assert!(tokens[1].is_start_tag("span"));
        assert!(tokens[2].is_end_tag("span"));
        assert!(tokens[3].is_end_tag("div"));
    }

    #[test]
    fn test_boolean_attribute() {
        let mut tok = Html5Tokenizer::new("<input disabled>");
        let tokens = tok.tokenize();
        assert!(tokens[0].attributes.contains_key("disabled"));
    }

    #[test]
    fn test_is_start_tag_helper() {
        let mut tok = Html5Tokenizer::new("<div>");
        let tokens = tok.tokenize();
        assert!(tokens[0].is_start_tag("div"));
        assert!(tokens[0].is_start_tag("DIV"));
        assert!(!tokens[0].is_start_tag("span"));
    }

    #[test]
    fn test_is_end_tag_helper() {
        let mut tok = Html5Tokenizer::new("</div>");
        let tokens = tok.tokenize();
        assert!(tokens[0].is_end_tag("div"));
        assert!(!tokens[0].is_end_tag("span"));
    }

    #[test]
    fn test_empty_input() {
        let mut tok = Html5Tokenizer::new("");
        let tokens = tok.tokenize();
        assert_eq!(tokens.len(), 0);
    }

    #[test]
    fn test_whitespace_only() {
        let mut tok = Html5Tokenizer::new("   ");
        let tokens = tok.tokenize();
        assert_eq!(tokens.len(), 0);
    }

    #[test]
    fn test_nested_elements() {
        let mut tok = Html5Tokenizer::new("<ul><li>Item 1</li><li>Item 2</li></ul>");
        let tokens = tok.tokenize();
        let start_tags: Vec<_> = tokens.iter().filter(|t| t.kind == TokenKind::StartTag).collect();
        assert_eq!(start_tags.len(), 3);
        assert_eq!(start_tags[0].name, "ul");
        assert_eq!(start_tags[1].name, "li");
        assert_eq!(start_tags[2].name, "li");
    }
}
