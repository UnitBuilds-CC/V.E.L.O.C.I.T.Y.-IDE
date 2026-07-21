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

pub struct Html5Tokenizer<'a> {
    input: &'a str,
    chars: Vec<char>,
    pos: usize,
    state: Html5State,
}

impl<'a> Html5Tokenizer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
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
