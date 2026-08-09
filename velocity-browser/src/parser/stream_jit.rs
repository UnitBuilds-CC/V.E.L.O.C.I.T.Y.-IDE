/// Token kinds produced by the streaming JIT tokenizer.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamJitTokenKind {
    OpenTag,
    CloseTag,
    SelfClosingTag,
    Text,
    Comment,
    Doctype,
    Attribute,
    Eof,
}

/// A token produced by the streaming tokenizer.
#[derive(Debug, Clone)]
pub struct StreamJitToken {
    pub token_kind: StreamJitTokenKind,
    pub raw_bytes: Vec<u8>,
    /// Tag name for open/close/self-closing tags.
    pub tag_name: String,
    /// Attributes for open/self-closing tags.
    pub attributes: Vec<(String, String)>,
    /// Text content for text tokens.
    pub text: String,
    /// Byte offset in the original stream.
    pub offset: usize,
}

/// Streaming JIT tokenizer that handles partial chunks and buffers incomplete tokens.
pub struct StreamJitTokenizer {
    /// Leftover bytes from the previous chunk that didn't form a complete token.
    buffer: Vec<u8>,
    /// Current byte offset in the overall stream.
    stream_offset: usize,
    /// Whether we are currently inside a tag (< ... >).
    in_tag: bool,
    /// Whether we are currently inside a comment (<!-- ... -->).
    in_comment: bool,
}

impl Default for StreamJitTokenizer {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamJitTokenizer {
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            stream_offset: 0,
            in_tag: false,
            in_comment: false,
        }
    }

    /// Reset the tokenizer state for a new stream.
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.stream_offset = 0;
        self.in_tag = false;
        self.in_comment = false;
    }

    /// Tokenize a chunk of bytes, returning complete tokens.
    /// Incomplete tokens are buffered internally for the next chunk.
    pub fn tokenize_stream_chunk(&mut self, chunk_bytes: &[u8]) -> Vec<StreamJitToken> {
        // Prepend any leftover buffer
        let data = if self.buffer.is_empty() {
            chunk_bytes.to_vec()
        } else {
            let mut combined = std::mem::take(&mut self.buffer);
            combined.extend_from_slice(chunk_bytes);
            combined
        };

        let mut tokens = Vec::new();
        let mut pos = 0;
        let len = data.len();

        while pos < len {
            if self.in_comment {
                // Look for -->
                if let Some(end) = Self::find_bytes(&data[pos..], b"-->") {
                    let comment_text = String::from_utf8_lossy(&data[pos..pos + end]).to_string();
                    tokens.push(StreamJitToken {
                        token_kind: StreamJitTokenKind::Comment,
                        raw_bytes: data[pos..pos + end + 3].to_vec(),
                        tag_name: String::new(),
                        attributes: Vec::new(),
                        text: comment_text,
                        offset: self.stream_offset,
                    });
                    self.stream_offset += end + 3;
                    pos += end + 3;
                    self.in_comment = false;
                } else {
                    // Buffer the rest and wait for more data
                    self.buffer = data[pos..].to_vec();
                    break;
                }
            } else if self.in_tag {
                // Inside a tag, look for '>'
                if let Some(gt_pos) = data[pos..].iter().position(|&b| b == b'>') {
                    let tag_content = &data[pos..pos + gt_pos];
                    let tag_str = String::from_utf8_lossy(tag_content).to_string();
                    let (kind, tag_name, attrs) = Self::parse_tag_content(&tag_str);
                    tokens.push(StreamJitToken {
                        token_kind: kind,
                        raw_bytes: data[pos..pos + gt_pos + 1].to_vec(),
                        tag_name,
                        attributes: attrs,
                        text: String::new(),
                        offset: self.stream_offset,
                    });
                    self.stream_offset += gt_pos + 1;
                    pos += gt_pos + 1;
                    self.in_tag = false;
                } else {
                    // Buffer and wait for more data
                    self.buffer = data[pos..].to_vec();
                    break;
                }
            } else {
                // Normal mode: look for '<'
                if data[pos] == b'<' {
                    // Check for comment start
                    if pos + 4 <= len && &data[pos..pos + 4] == b"<!--" {
                        // Look for comment end
                        if let Some(end) = Self::find_bytes(&data[pos + 4..], b"-->") {
                            let comment_text =
                                String::from_utf8_lossy(&data[pos + 4..pos + 4 + end]).to_string();
                            tokens.push(StreamJitToken {
                                token_kind: StreamJitTokenKind::Comment,
                                raw_bytes: data[pos..pos + 4 + end + 3].to_vec(),
                                tag_name: String::new(),
                                attributes: Vec::new(),
                                text: comment_text,
                                offset: self.stream_offset,
                            });
                            self.stream_offset += 4 + end + 3;
                            pos += 4 + end + 3;
                        } else {
                            self.in_comment = true;
                            self.buffer = data[pos..].to_vec();
                            break;
                        }
                    } else if pos + 9 <= len
                        && data[pos + 1] == b'!'
                        && data[pos + 2..pos + 9].eq_ignore_ascii_case(b"doctype")
                    {
                        // DOCTYPE
                        if let Some(gt) = data[pos..].iter().position(|&b| b == b'>') {
                            let doctype_text =
                                String::from_utf8_lossy(&data[pos..pos + gt + 1]).to_string();
                            tokens.push(StreamJitToken {
                                token_kind: StreamJitTokenKind::Doctype,
                                raw_bytes: data[pos..pos + gt + 1].to_vec(),
                                tag_name: String::new(),
                                attributes: Vec::new(),
                                text: doctype_text,
                                offset: self.stream_offset,
                            });
                            self.stream_offset += gt + 1;
                            pos += gt + 1;
                        } else {
                            self.buffer = data[pos..].to_vec();
                            break;
                        }
                    } else {
                        // Start of a tag
                        self.in_tag = true;
                        pos += 1; // skip '<'
                        self.stream_offset += 1;
                    }
                } else {
                    // Text content — collect until '<' or end
                    let text_start = pos;
                    while pos < len && data[pos] != b'<' {
                        pos += 1;
                    }
                    let text_bytes = &data[text_start..pos];
                    if !text_bytes.is_empty() {
                        let text = String::from_utf8_lossy(text_bytes).to_string();
                        tokens.push(StreamJitToken {
                            token_kind: StreamJitTokenKind::Text,
                            raw_bytes: text_bytes.to_vec(),
                            tag_name: String::new(),
                            attributes: Vec::new(),
                            text,
                            offset: self.stream_offset,
                        });
                        self.stream_offset += pos - text_start;
                    }
                }
            }
        }

        tokens
    }

    /// Find a byte pattern in a slice.
    fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        if needle.is_empty() || haystack.len() < needle.len() {
            return None;
        }
        (0..=(haystack.len() - needle.len())).find(|&i| &haystack[i..i + needle.len()] == needle)
    }

    /// Parse the content of a tag (between < and >) into kind, name, and attributes.
    fn parse_tag_content(content: &str) -> (StreamJitTokenKind, String, Vec<(String, String)>) {
        let trimmed = content.trim();

        // Close tag
        if trimmed.starts_with('/') {
            let tag_name = trimmed[1..]
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string();
            return (StreamJitTokenKind::CloseTag, tag_name, Vec::new());
        }

        // Self-closing tag
        let is_self_closing = trimmed.ends_with('/');
        let content_trimmed = if is_self_closing {
            &trimmed[..trimmed.len() - 1]
        } else {
            trimmed
        };

        // Split tag name and attributes
        let mut parts = content_trimmed.splitn(2, char::is_whitespace);
        let tag_name = parts.next().unwrap_or("").to_string();
        let attrs_str = parts.next().unwrap_or("");

        let attributes = Self::parse_attributes(attrs_str);

        let kind = if is_self_closing {
            StreamJitTokenKind::SelfClosingTag
        } else {
            StreamJitTokenKind::OpenTag
        };

        (kind, tag_name, attributes)
    }

    /// Parse attribute string into key-value pairs.
    fn parse_attributes(attrs_str: &str) -> Vec<(String, String)> {
        let mut attrs = Vec::new();
        let mut chars = attrs_str.chars().peekable();

        while let Some(&ch) = chars.peek() {
            if ch.is_whitespace() {
                chars.next();
                continue;
            }

            // Read attribute name
            let mut name = String::new();
            while let Some(&c) = chars.peek() {
                if c == '=' || c.is_whitespace() {
                    break;
                }
                name.push(c);
                chars.next();
            }

            if name.is_empty() {
                chars.next();
                continue;
            }

            // Skip whitespace
            while let Some(&c) = chars.peek() {
                if !c.is_whitespace() {
                    break;
                }
                chars.next();
            }

            // Check for '='
            if chars.peek() == Some(&'=') {
                chars.next(); // consume '='
                              // Skip whitespace
                while let Some(&c) = chars.peek() {
                    if !c.is_whitespace() {
                        break;
                    }
                    chars.next();
                }
                // Read value
                let mut value = String::new();
                if let Some(&quote) = chars.peek() {
                    if quote == '"' || quote == '\'' {
                        chars.next(); // consume opening quote
                        while let Some(&c) = chars.peek() {
                            if c == quote {
                                chars.next();
                                break;
                            }
                            value.push(c);
                            chars.next();
                        }
                    } else {
                        // Unquoted value
                        while let Some(&c) = chars.peek() {
                            if c.is_whitespace() {
                                break;
                            }
                            value.push(c);
                            chars.next();
                        }
                    }
                }
                attrs.push((name, value));
            } else {
                // Boolean attribute
                attrs.push((name, String::new()));
            }
        }

        attrs
    }

    /// Get the current stream offset.
    pub fn offset(&self) -> usize {
        self.stream_offset
    }

    /// Check if the tokenizer has buffered data.
    pub fn has_buffered(&self) -> bool {
        !self.buffer.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_open_close() {
        let mut tok = StreamJitTokenizer::new();
        let tokens = tok.tokenize_stream_chunk(b"<div>hello</div>");
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].token_kind, StreamJitTokenKind::OpenTag);
        assert_eq!(tokens[0].tag_name, "div");
        assert_eq!(tokens[1].token_kind, StreamJitTokenKind::Text);
        assert_eq!(tokens[1].text, "hello");
        assert_eq!(tokens[2].token_kind, StreamJitTokenKind::CloseTag);
        assert_eq!(tokens[2].tag_name, "div");
    }

    #[test]
    fn test_attributes() {
        let mut tok = StreamJitTokenizer::new();
        let tokens = tok.tokenize_stream_chunk(b"<input type=\"text\" name=\"q\" />");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].token_kind, StreamJitTokenKind::SelfClosingTag);
        assert_eq!(tokens[0].tag_name, "input");
        assert_eq!(tokens[0].attributes.len(), 2);
        assert_eq!(
            tokens[0].attributes[0],
            ("type".to_string(), "text".to_string())
        );
        assert_eq!(
            tokens[0].attributes[1],
            ("name".to_string(), "q".to_string())
        );
    }

    #[test]
    fn test_comment() {
        let mut tok = StreamJitTokenizer::new();
        let tokens = tok.tokenize_stream_chunk(b"<!-- this is a comment -->");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].token_kind, StreamJitTokenKind::Comment);
        assert_eq!(tokens[0].text, " this is a comment ");
    }

    #[test]
    fn test_doctype() {
        let mut tok = StreamJitTokenizer::new();
        let tokens = tok.tokenize_stream_chunk(b"<!DOCTYPE html><html>");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].token_kind, StreamJitTokenKind::Doctype);
        assert_eq!(tokens[1].token_kind, StreamJitTokenKind::OpenTag);
        assert_eq!(tokens[1].tag_name, "html");
    }

    #[test]
    fn test_chunked_streaming() {
        let mut tok = StreamJitTokenizer::new();
        // First chunk: incomplete tag
        let tokens1 = tok.tokenize_stream_chunk(b"<di");
        assert_eq!(tokens1.len(), 0); // buffered
        assert!(tok.has_buffered());

        // Second chunk: completes the tag
        let tokens2 = tok.tokenize_stream_chunk(b"v>hello");
        assert_eq!(tokens2.len(), 2);
        assert_eq!(tokens2[0].token_kind, StreamJitTokenKind::OpenTag);
        assert_eq!(tokens2[0].tag_name, "div");
        assert_eq!(tokens2[1].token_kind, StreamJitTokenKind::Text);
        assert_eq!(tokens2[1].text, "hello");
    }

    #[test]
    fn test_chunked_comment() {
        let mut tok = StreamJitTokenizer::new();
        let tokens1 = tok.tokenize_stream_chunk(b"<!-- partial");
        assert_eq!(tokens1.len(), 0);
        let tokens2 = tok.tokenize_stream_chunk(b" comment -->");
        assert_eq!(tokens2.len(), 1);
        assert_eq!(tokens2[0].token_kind, StreamJitTokenKind::Comment);
    }

    #[test]
    fn test_boolean_attribute() {
        let mut tok = StreamJitTokenizer::new();
        let tokens = tok.tokenize_stream_chunk(b"<input disabled required>");
        assert_eq!(tokens[0].attributes.len(), 2);
        assert_eq!(tokens[0].attributes[0].0, "disabled");
        assert_eq!(tokens[0].attributes[0].1, "");
        assert_eq!(tokens[0].attributes[1].0, "required");
    }

    #[test]
    fn test_reset() {
        let mut tok = StreamJitTokenizer::new();
        tok.tokenize_stream_chunk(b"<div");
        assert!(tok.has_buffered());
        tok.reset();
        assert!(!tok.has_buffered());
        assert_eq!(tok.offset(), 0);
    }

    #[test]
    fn test_multiple_text_runs() {
        let mut tok = StreamJitTokenizer::new();
        let tokens = tok.tokenize_stream_chunk(b"hello <b>world</b> foo");
        assert_eq!(tokens.len(), 5);
        assert_eq!(tokens[0].text, "hello ");
        assert_eq!(tokens[1].tag_name, "b");
        assert_eq!(tokens[2].text, "world");
        assert_eq!(tokens[3].tag_name, "b");
        assert_eq!(tokens[4].text, " foo");
    }

    #[test]
    fn test_empty_input() {
        let mut tok = StreamJitTokenizer::new();
        let tokens = tok.tokenize_stream_chunk(b"");
        assert!(tokens.is_empty());
        assert!(!tok.has_buffered());
    }

    #[test]
    fn test_text_only_no_tags() {
        let mut tok = StreamJitTokenizer::new();
        let tokens = tok.tokenize_stream_chunk(b"just plain text");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].token_kind, StreamJitTokenKind::Text);
        assert_eq!(tokens[0].text, "just plain text");
    }

    #[test]
    fn test_offset_advances() {
        let mut tok = StreamJitTokenizer::new();
        tok.tokenize_stream_chunk(b"<div>hello</div>");
        assert!(tok.offset() > 0);
        assert_eq!(tok.offset(), 16); // "<div>hello</div>" = 16 bytes
    }

    #[test]
    fn test_self_closing_tag() {
        let mut tok = StreamJitTokenizer::new();
        let tokens = tok.tokenize_stream_chunk(b"<br/>");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].token_kind, StreamJitTokenKind::SelfClosingTag);
        assert_eq!(tokens[0].tag_name, "br");
    }

    #[test]
    fn test_close_tag() {
        let mut tok = StreamJitTokenizer::new();
        let tokens = tok.tokenize_stream_chunk(b"</p>");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].token_kind, StreamJitTokenKind::CloseTag);
        assert_eq!(tokens[0].tag_name, "p");
    }

    #[test]
    fn test_unquoted_attribute_value() {
        let mut tok = StreamJitTokenizer::new();
        let tokens = tok.tokenize_stream_chunk(b"<input type=text>");
        assert_eq!(tokens[0].attributes.len(), 1);
        assert_eq!(
            tokens[0].attributes[0],
            ("type".to_string(), "text".to_string())
        );
    }

    #[test]
    fn test_single_quoted_attribute() {
        let mut tok = StreamJitTokenizer::new();
        let tokens = tok.tokenize_stream_chunk(b"<div class='main'>");
        assert_eq!(tokens[0].attributes.len(), 1);
        assert_eq!(
            tokens[0].attributes[0],
            ("class".to_string(), "main".to_string())
        );
    }

    #[test]
    fn test_chunked_tag_split_at_name() {
        let mut tok = StreamJitTokenizer::new();
        // Split right in the middle of the tag name
        let t1 = tok.tokenize_stream_chunk(b"<sp");
        assert!(t1.is_empty()); // buffered
        let t2 = tok.tokenize_stream_chunk(b"an>text");
        assert_eq!(t2.len(), 2);
        assert_eq!(t2[0].tag_name, "span");
        assert_eq!(t2[1].text, "text");
    }

    #[test]
    fn test_reset_clears_state() {
        let mut tok = StreamJitTokenizer::new();
        tok.tokenize_stream_chunk(b"<div");
        assert!(tok.has_buffered());
        assert!(tok.offset() > 0);
        tok.reset();
        assert!(!tok.has_buffered());
        assert_eq!(tok.offset(), 0);
        // Should work normally after reset
        let tokens = tok.tokenize_stream_chunk(b"<p>hi</p>");
        assert_eq!(tokens.len(), 3);
    }
}
