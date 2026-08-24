// compiler/nda_lexer.rs — Tokenizer for the NDA programming language
//
// Converts raw .nda source text into a stream of tokens.
// Supports: keywords, identifiers, numbers, operators, delimiters, comments.
#![allow(dead_code)]

/// Token types for the NDA language.
#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    // Keywords
    Fn,
    Let,
    Loop,
    While,
    If,
    Else,
    Return,
    Break,
    Print,
    // Type keywords
    Vec,
    Matrix,
    Norm,
    Int,
    // Built-in function keywords
    Add,
    Silu,
    Negate,
    Abs,
    ReduceSum,
    // Identifiers and literals
    Ident(String),
    IntLit(i64),
    FloatLit(f64),
    StringLit(String),
    // Operators
    Eq,     // ==
    Ne,     // !=
    Lt,     // <
    Gt,     // >
    Le,     // <=
    Ge,     // >=
    Assign, // =
    Plus,   // +
    Minus,  // -
    Star,   // *
    Slash,  // /
    Percent,// %
    Arrow,  // ->
    Dot,    // .
    // Delimiters
    LParen,   // (
    RParen,   // )
    LBrace,   // {
    RBrace,   // }
    LBracket, // [
    RBracket, // ]
    Comma,    // ,
    Semi,     // ;
    Colon,    // :
    Pipe,     // |
    Amp,      // &
    // End
    Eof,
}

impl Token {
    /// Human-readable name for error messages.
    pub fn display_name(&self) -> &'static str {
        match self {
            Token::Fn => "'fn'",
            Token::Let => "'let'",
            Token::Loop => "'loop'",
            Token::While => "'while'",
            Token::If => "'if'",
            Token::Else => "'else'",
            Token::Return => "'return'",
            Token::Break => "'break'",
            Token::Print => "'print'",
            Token::Vec => "'vec'",
            Token::Matrix => "'matrix'",
            Token::Norm => "'norm'",
            Token::Int => "'int'",
            Token::Add => "'add'",
            Token::Silu => "'silu'",
            Token::Negate => "'negate'",
            Token::Abs => "'abs'",
            Token::ReduceSum => "'reduce_sum'",
            Token::Ident(_) => "identifier",
            Token::IntLit(_) => "integer literal",
            Token::FloatLit(_) => "float literal",
            Token::StringLit(_) => "string literal",
            Token::Eq => "'=='",
            Token::Ne => "'!='",
            Token::Lt => "'<'",
            Token::Gt => "'>'",
            Token::Le => "'<='",
            Token::Ge => "'>='",
            Token::Assign => "'='",
            Token::Plus => "'+'",
            Token::Minus => "'-'",
            Token::Star => "'*'",
            Token::Slash => "'/'",
            Token::Percent => "'%'",
            Token::Arrow => "'->'",
            Token::Dot => "'.'",
            Token::LParen => "'('",
            Token::RParen => "')'",
            Token::LBrace => "'{'",
            Token::RBrace => "'}'",
            Token::LBracket => "'['",
            Token::RBracket => "']'",
            Token::Comma => "','",
            Token::Semi => "';'",
            Token::Colon => "':'",
            Token::Pipe => "'|'",
            Token::Amp => "'&'",
            Token::Eof => "end of file",
        }
    }
}

/// A token with its source location.
#[derive(Clone, Debug)]
pub struct Located {
    pub token: Token,
    pub line: usize,
    pub col: usize,
}

/// Lexer for NDA source code.
pub struct NdaLexer {
    chars: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
}

impl NdaLexer {
    pub fn new(source: &str) -> Self {
        Self {
            chars: source.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    /// Tokenize the entire source into a Vec of Located tokens.
    pub fn tokenize(&mut self) -> Result<Vec<Located>, String> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token()?;
            let is_eof = tok.token == Token::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        Ok(tokens)
    }

    /// Error-recovering tokenization: collects errors instead of stopping.
    /// Returns the tokens that were successfully lexed plus any errors.
    pub fn tokenize_with_errors(&mut self) -> (Vec<Located>, Vec<String>) {
        let mut tokens = Vec::new();
        let mut errors = Vec::new();
        loop {
            match self.next_token() {
                Ok(tok) => {
                    let is_eof = tok.token == Token::Eof;
                    tokens.push(tok);
                    if is_eof {
                        break;
                    }
                }
                Err(e) => {
                    errors.push(e);
                    // Skip the offending character and continue
                    self.advance();
                    if self.peek().is_none() {
                        tokens.push(Located {
                            token: Token::Eof,
                            line: self.line,
                            col: self.col,
                        });
                        break;
                    }
                }
            }
        }
        (tokens, errors)
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.chars.get(self.pos).copied()?;
        self.pos += 1;
        if ch == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(ch)
    }

    fn peek_next(&self) -> Option<char> {
        self.chars.get(self.pos + 1).copied()
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() {
                self.advance();
            } else if ch == '/' && self.peek_next() == Some('/') {
                // Line comment — skip to end of line
                while let Some(c) = self.advance() {
                    if c == '\n' {
                        break;
                    }
                }
            } else {
                break;
            }
        }
    }

    fn next_token(&mut self) -> Result<Located, String> {
        self.skip_whitespace();
        let line = self.line;
        let col = self.col;

        let ch = match self.peek() {
            Some(c) => c,
            None => {
                return Ok(Located {
                    token: Token::Eof,
                    line,
                    col,
                })
            }
        };

        // Single/double-char operators and delimiters
        match ch {
            '(' => {
                self.advance();
                return Ok(Located {
                    token: Token::LParen,
                    line,
                    col,
                });
            }
            ')' => {
                self.advance();
                return Ok(Located {
                    token: Token::RParen,
                    line,
                    col,
                });
            }
            '{' => {
                self.advance();
                return Ok(Located {
                    token: Token::LBrace,
                    line,
                    col,
                });
            }
            '}' => {
                self.advance();
                return Ok(Located {
                    token: Token::RBrace,
                    line,
                    col,
                });
            }
            '[' => {
                self.advance();
                return Ok(Located {
                    token: Token::LBracket,
                    line,
                    col,
                });
            }
            ']' => {
                self.advance();
                return Ok(Located {
                    token: Token::RBracket,
                    line,
                    col,
                });
            }
            ',' => {
                self.advance();
                return Ok(Located {
                    token: Token::Comma,
                    line,
                    col,
                });
            }
            ';' => {
                self.advance();
                return Ok(Located {
                    token: Token::Semi,
                    line,
                    col,
                });
            }
            ':' => {
                self.advance();
                return Ok(Located {
                    token: Token::Colon,
                    line,
                    col,
                });
            }
            '+' => {
                self.advance();
                return Ok(Located {
                    token: Token::Plus,
                    line,
                    col,
                });
            }
            '*' => {
                self.advance();
                return Ok(Located {
                    token: Token::Star,
                    line,
                    col,
                });
            }
            '/' => {
                self.advance();
                return Ok(Located {
                    token: Token::Slash,
                    line,
                    col,
                });
            }
            '%' => {
                self.advance();
                return Ok(Located {
                    token: Token::Percent,
                    line,
                    col,
                });
            }
            '.' => {
                self.advance();
                return Ok(Located {
                    token: Token::Dot,
                    line,
                    col,
                });
            }
            '|' => {
                self.advance();
                return Ok(Located {
                    token: Token::Pipe,
                    line,
                    col,
                });
            }
            '&' => {
                self.advance();
                return Ok(Located {
                    token: Token::Amp,
                    line,
                    col,
                });
            }
            '-' => {
                self.advance();
                if self.peek() == Some('>') {
                    self.advance();
                    return Ok(Located {
                        token: Token::Arrow,
                        line,
                        col,
                    });
                }
                return Ok(Located {
                    token: Token::Minus,
                    line,
                    col,
                });
            }
            '=' => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    return Ok(Located {
                        token: Token::Eq,
                        line,
                        col,
                    });
                }
                return Ok(Located {
                    token: Token::Assign,
                    line,
                    col,
                });
            }
            '!' => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    return Ok(Located {
                        token: Token::Ne,
                        line,
                        col,
                    });
                }
                return Err(format!("{}:{}: Unexpected '!'", line, col));
            }
            '<' => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    return Ok(Located {
                        token: Token::Le,
                        line,
                        col,
                    });
                }
                return Ok(Located {
                    token: Token::Lt,
                    line,
                    col,
                });
            }
            '>' => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    return Ok(Located {
                        token: Token::Ge,
                        line,
                        col,
                    });
                }
                return Ok(Located {
                    token: Token::Gt,
                    line,
                    col,
                });
            }
            _ => {}
        }

        // Numbers (integers and floats, including hex)
        if ch.is_ascii_digit()
            || (ch == '.' && self.peek_next().is_some_and(|c| c.is_ascii_digit()))
        {
            return self.lex_number(line, col);
        }

        // String literals
        if ch == '"' {
            return self.lex_string(line, col);
        }

        // Identifiers and keywords
        if ch.is_ascii_alphabetic() || ch == '_' {
            return self.lex_ident(line, col);
        }

        Err(format!("{}:{}: Unexpected character '{}'", line, col, ch))
    }

    fn lex_number(&mut self, line: usize, col: usize) -> Result<Located, String> {
        let mut s = String::new();
        let mut is_float = false;

        // Check for hex literal: 0x or 0X
        if self.peek() == Some('0')
            && self.peek_next().is_some_and(|c| c == 'x' || c == 'X')
        {
            s.push('0');
            self.advance();
            s.push('x');
            self.advance();
            while let Some(ch) = self.peek() {
                if ch.is_ascii_hexdigit() || ch == '_' {
                    if ch != '_' {
                        s.push(ch);
                    }
                    self.advance();
                } else {
                    break;
                }
            }
            let hex_str = s.trim_start_matches("0x");
            match i64::from_str_radix(hex_str, 16) {
                Ok(v) => {
                    return Ok(Located {
                        token: Token::IntLit(v),
                        line,
                        col,
                    })
                }
                Err(_) => {
                    return Err(format!(
                        "{}:{}: Invalid hex literal '{}'",
                        line, col, s
                    ))
                }
            }
        }

        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() || ch == '.' || ch == 'e' || ch == 'E' {
                if ch == '.' || ch == 'e' || ch == 'E' {
                    is_float = true;
                }
                s.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        if is_float {
            match s.parse::<f64>() {
                Ok(v) => Ok(Located {
                    token: Token::FloatLit(v),
                    line,
                    col,
                }),
                Err(_) => Err(format!("{}:{}: Invalid float literal '{}'", line, col, s)),
            }
        } else {
            match s.parse::<i64>() {
                Ok(v) => Ok(Located {
                    token: Token::IntLit(v),
                    line,
                    col,
                }),
                Err(_) => Err(format!("{}:{}: Invalid integer literal '{}'", line, col, s)),
            }
        }
    }

    fn lex_string(&mut self, line: usize, col: usize) -> Result<Located, String> {
        self.advance(); // consume opening quote
        let mut s = String::new();
        loop {
            match self.advance() {
                Some('"') => break,
                Some('\\') => {
                    // Escape sequences
                    match self.advance() {
                        Some('n') => s.push('\n'),
                        Some('t') => s.push('\t'),
                        Some('\\') => s.push('\\'),
                        Some('"') => s.push('"'),
                        Some('0') => s.push('\0'),
                        Some(c) => {
                            return Err(format!(
                                "{}:{}: Unknown escape sequence '\\{}'",
                                line, col, c
                            ))
                        }
                        None => {
                            return Err(format!(
                                "{}:{}: Unterminated string literal",
                                line, col
                            ))
                        }
                    }
                }
                Some(c) => s.push(c),
                None => {
                    return Err(format!(
                        "{}:{}: Unterminated string literal",
                        line, col
                    ))
                }
            }
        }
        Ok(Located {
            token: Token::StringLit(s),
            line,
            col,
        })
    }

    fn lex_ident(&mut self, line: usize, col: usize) -> Result<Located, String> {
        let mut s = String::new();
        while let Some(ch) = self.peek() {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                s.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        let token = match s.as_str() {
            "fn" => Token::Fn,
            "let" => Token::Let,
            "loop" => Token::Loop,
            "while" => Token::While,
            "if" => Token::If,
            "else" => Token::Else,
            "return" => Token::Return,
            "break" => Token::Break,
            "print" => Token::Print,
            "vec" => Token::Vec,
            "matrix" => Token::Matrix,
            "norm" => Token::Norm,
            "int" => Token::Int,
            "add" => Token::Add,
            "silu" => Token::Silu,
            "negate" => Token::Negate,
            "abs" => Token::Abs,
            "reduce_sum" => Token::ReduceSum,
            _ => Token::Ident(s),
        };
        Ok(Located { token, line, col })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lex_simple_function() {
        let src = "fn main() { let x = 42 }";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Fn);
        assert_eq!(tokens[1].token, Token::Ident("main".to_string()));
        assert_eq!(tokens[2].token, Token::LParen);
        assert_eq!(tokens[3].token, Token::RParen);
        assert_eq!(tokens[4].token, Token::LBrace);
        assert_eq!(tokens[5].token, Token::Let);
        assert_eq!(tokens[6].token, Token::Ident("x".to_string()));
        assert_eq!(tokens[7].token, Token::Assign);
        assert_eq!(tokens[8].token, Token::IntLit(42));
        assert_eq!(tokens[9].token, Token::RBrace);
        assert_eq!(tokens[10].token, Token::Eof);
    }

    #[test]
    fn lex_comparison_operators() {
        let src = "a == b != c < d > e <= f >= g";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[1].token, Token::Eq);
        assert_eq!(tokens[3].token, Token::Ne);
        assert_eq!(tokens[5].token, Token::Lt);
        assert_eq!(tokens[7].token, Token::Gt);
        assert_eq!(tokens[9].token, Token::Le);
        assert_eq!(tokens[11].token, Token::Ge);
    }

    #[test]
    fn lex_float_literal() {
        let src = "1.23";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::FloatLit(1.23));
    }

    #[test]
    fn lex_arrow() {
        let src = "-> vec";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Arrow);
        assert_eq!(tokens[1].token, Token::Vec);
    }

    #[test]
    fn lex_comments_are_skipped() {
        let src = "let x = 1 // this is a comment\nlet y = 2";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        // Should have: let x = 1 let y = 2 Eof
        assert_eq!(tokens.len(), 9); // let x = 1 let y = 2 Eof
    }

    #[test]
    fn lex_hex_literal() {
        let src = "0xFF";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::IntLit(255));
    }

    #[test]
    fn lex_hex_literal_with_underscores() {
        let src = "0xDEAD_BEEF";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::IntLit(0xDEAD_BEEF));
    }

    #[test]
    fn lex_string_literal() {
        let src = r#""hello world""#;
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(
            tokens[0].token,
            Token::StringLit("hello world".to_string())
        );
    }

    #[test]
    fn lex_string_with_escapes() {
        let src = r#""line\n\ttab""#;
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(
            tokens[0].token,
            Token::StringLit("line\n\ttab".to_string())
        );
    }

    #[test]
    fn lex_new_operators() {
        let src = "a / b % c . d | e & f";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[1].token, Token::Slash);
        assert_eq!(tokens[3].token, Token::Percent);
        assert_eq!(tokens[5].token, Token::Dot);
        assert_eq!(tokens[7].token, Token::Pipe);
        assert_eq!(tokens[9].token, Token::Amp);
    }

    #[test]
    fn error_recovery_skips_bad_chars() {
        let src = "let x = 1\nlet y = @2";
        let mut lexer = NdaLexer::new(src);
        let (tokens, errors) = lexer.tokenize_with_errors();
        assert!(!errors.is_empty(), "should have at least one error");
        // Should still produce tokens for the valid parts
        assert!(tokens.len() > 3, "should produce tokens despite error");
        // Last token should be Eof
        assert_eq!(tokens.last().unwrap().token, Token::Eof);
    }

    #[test]
    fn token_display_names() {
        assert_eq!(Token::Fn.display_name(), "'fn'");
        assert_eq!(Token::Plus.display_name(), "'+'");
        assert_eq!(Token::Ident("x".into()).display_name(), "identifier");
        assert_eq!(Token::Eof.display_name(), "end of file");
    }

    // ─── Keyword tests ─────────────────────────────────────────────────────

    #[test]
    fn lex_all_keywords() {
        let src = "fn let loop while if else return break print";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Fn);
        assert_eq!(tokens[1].token, Token::Let);
        assert_eq!(tokens[2].token, Token::Loop);
        assert_eq!(tokens[3].token, Token::While);
        assert_eq!(tokens[4].token, Token::If);
        assert_eq!(tokens[5].token, Token::Else);
        assert_eq!(tokens[6].token, Token::Return);
        assert_eq!(tokens[7].token, Token::Break);
        assert_eq!(tokens[8].token, Token::Print);
    }

    #[test]
    fn lex_type_keywords() {
        let src = "vec matrix norm int";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Vec);
        assert_eq!(tokens[1].token, Token::Matrix);
        assert_eq!(tokens[2].token, Token::Norm);
        assert_eq!(tokens[3].token, Token::Int);
    }

    #[test]
    fn lex_builtin_function_keywords() {
        let src = "add silu negate abs reduce_sum";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Add);
        assert_eq!(tokens[1].token, Token::Silu);
        assert_eq!(tokens[2].token, Token::Negate);
        assert_eq!(tokens[3].token, Token::Abs);
        assert_eq!(tokens[4].token, Token::ReduceSum);
    }

    // ─── Delimiter tests ───────────────────────────────────────────────────

    #[test]
    fn lex_all_delimiters() {
        let src = "( ) { } [ ] , ; :";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::LParen);
        assert_eq!(tokens[1].token, Token::RParen);
        assert_eq!(tokens[2].token, Token::LBrace);
        assert_eq!(tokens[3].token, Token::RBrace);
        assert_eq!(tokens[4].token, Token::LBracket);
        assert_eq!(tokens[5].token, Token::RBracket);
        assert_eq!(tokens[6].token, Token::Comma);
        assert_eq!(tokens[7].token, Token::Semi);
        assert_eq!(tokens[8].token, Token::Colon);
    }

    // ─── Operator tests ────────────────────────────────────────────────────

    #[test]
    fn lex_minus_vs_arrow() {
        let src = "- -> -x";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Minus);
        assert_eq!(tokens[1].token, Token::Arrow);
        assert_eq!(tokens[2].token, Token::Minus);
    }

    #[test]
    fn lex_assign_vs_eq() {
        let src = "= ==";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Assign);
        assert_eq!(tokens[1].token, Token::Eq);
    }

    #[test]
    fn lex_all_arithmetic_operators() {
        let src = "+ - * / %";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Plus);
        assert_eq!(tokens[1].token, Token::Minus);
        assert_eq!(tokens[2].token, Token::Star);
        assert_eq!(tokens[3].token, Token::Slash);
        assert_eq!(tokens[4].token, Token::Percent);
    }

    // ─── Number tests ──────────────────────────────────────────────────────

    #[test]
    fn lex_integer_literal() {
        let src = "42";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::IntLit(42));
    }

    #[test]
    fn lex_zero() {
        let src = "0";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::IntLit(0));
    }

    #[test]
    fn lex_scientific_notation() {
        let src = "1e10";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        match &tokens[0].token {
            Token::FloatLit(v) => assert!((v - 1e10).abs() < 1.0),
            _ => panic!("Expected FloatLit, got {:?}", tokens[0].token),
        }
    }

    #[test]
    fn lex_scientific_notation_uppercase() {
        let src = "3E2";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        match &tokens[0].token {
            Token::FloatLit(v) => assert!((v - 300.0).abs() < 0.1),
            _ => panic!("Expected FloatLit, got {:?}", tokens[0].token),
        }
    }

    #[test]
    fn lex_hex_upper() {
        let src = "0XAB";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::IntLit(0xAB));
    }

    #[test]
    fn lex_integer_followed_by_dot() {
        // The lexer reads '.' as part of a number, so "5." becomes FloatLit(5.0)
        let src = "5.";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        match &tokens[0].token {
            Token::FloatLit(v) => assert!((v - 5.0).abs() < f64::EPSILON),
            _ => panic!("Expected FloatLit, got {:?}", tokens[0].token),
        }
    }

    // ─── String tests ──────────────────────────────────────────────────────

    #[test]
    fn lex_empty_string() {
        let src = r#""""#;
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::StringLit("".to_string()));
    }

    #[test]
    fn lex_string_backslash_escape() {
        let src = r#""a\\b""#;
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::StringLit("a\\b".to_string()));
    }

    #[test]
    fn lex_string_quote_escape() {
        let src = r#""say \"hi\"""#;
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::StringLit("say \"hi\"".to_string()));
    }

    #[test]
    fn lex_string_null_escape() {
        let src = r#""null\0byte""#;
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::StringLit("null\0byte".to_string()));
    }

    #[test]
    fn lex_unterminated_string() {
        let src = r#""hello"#;
        let mut lexer = NdaLexer::new(src);
        let result = lexer.tokenize();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unterminated"));
    }

    #[test]
    fn lex_unknown_escape() {
        let src = r#""\q""#;
        let mut lexer = NdaLexer::new(src);
        let result = lexer.tokenize();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown escape"));
    }

    // ─── Identifier tests ──────────────────────────────────────────────────

    #[test]
    fn lex_identifier_with_underscores() {
        let src = "my_var_name";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Ident("my_var_name".to_string()));
    }

    #[test]
    fn lex_identifier_starting_with_underscore() {
        let src = "_private";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Ident("_private".to_string()));
    }

    #[test]
    fn lex_identifier_with_digits() {
        let src = "x42";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Ident("x42".to_string()));
    }

    // ─── Location tracking tests ───────────────────────────────────────────

    #[test]
    fn lex_line_tracking() {
        let src = "fn\nmain\n()";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].line, 1); // fn
        assert_eq!(tokens[1].line, 2); // main
        assert_eq!(tokens[2].line, 3); // (
    }

    #[test]
    fn lex_col_tracking() {
        let src = "fn main()";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].col, 1); // fn at col 1
        assert_eq!(tokens[1].col, 4); // main at col 4
        assert_eq!(tokens[2].col, 8); // ( at col 8
    }

    // ─── Edge case tests ───────────────────────────────────────────────────

    #[test]
    fn lex_empty_source() {
        let src = "";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].token, Token::Eof);
    }

    #[test]
    fn lex_whitespace_only() {
        let src = "   \n\t  \n  ";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].token, Token::Eof);
    }

    #[test]
    fn lex_comment_only() {
        let src = "// just a comment";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].token, Token::Eof);
    }

    #[test]
    fn lex_lone_bang_error() {
        let src = "!";
        let mut lexer = NdaLexer::new(src);
        let result = lexer.tokenize();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unexpected"));
    }

    #[test]
    fn lex_unexpected_char_error() {
        let src = "@";
        let mut lexer = NdaLexer::new(src);
        let result = lexer.tokenize();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unexpected character"));
    }

    #[test]
    fn tokenize_with_errors_clean() {
        let src = "fn main() {}";
        let mut lexer = NdaLexer::new(src);
        let (tokens, errors) = lexer.tokenize_with_errors();
        assert!(errors.is_empty());
        assert_eq!(tokens.last().unwrap().token, Token::Eof);
    }

    #[test]
    fn tokenize_with_errors_multiple() {
        let src = "let x = @#%";
        let mut lexer = NdaLexer::new(src);
        let (tokens, errors) = lexer.tokenize_with_errors();
        assert!(errors.len() >= 2, "expected multiple errors, got: {:?}", errors);
        // Should still produce tokens for the valid parts
        assert!(tokens.len() > 3);
    }

    // ─── Display name coverage ─────────────────────────────────────────────

    #[test]
    fn all_token_display_names_nonempty() {
        let tokens = vec![
            Token::Fn, Token::Let, Token::Loop, Token::While, Token::If,
            Token::Else, Token::Return, Token::Break, Token::Print,
            Token::Vec, Token::Matrix, Token::Norm, Token::Int,
            Token::Add, Token::Silu, Token::Negate, Token::Abs, Token::ReduceSum,
            Token::Ident("x".into()), Token::IntLit(0), Token::FloatLit(0.0),
            Token::StringLit("".into()),
            Token::Eq, Token::Ne, Token::Lt, Token::Gt, Token::Le, Token::Ge,
            Token::Assign, Token::Plus, Token::Minus, Token::Star, Token::Slash,
            Token::Percent, Token::Arrow, Token::Dot,
            Token::LParen, Token::RParen, Token::LBrace, Token::RBrace,
            Token::LBracket, Token::RBracket, Token::Comma, Token::Semi,
            Token::Colon, Token::Pipe, Token::Amp, Token::Eof,
        ];
        for tok in &tokens {
            let name = tok.display_name();
            assert!(!name.is_empty(), "display_name for {:?} is empty", tok);
        }
    }

    // ── Display name: exact values ───────────────────────────────────────

    #[test]
    fn display_name_keywords_exact() {
        assert_eq!(Token::Fn.display_name(), "'fn'");
        assert_eq!(Token::Let.display_name(), "'let'");
        assert_eq!(Token::Loop.display_name(), "'loop'");
        assert_eq!(Token::While.display_name(), "'while'");
        assert_eq!(Token::If.display_name(), "'if'");
        assert_eq!(Token::Else.display_name(), "'else'");
        assert_eq!(Token::Return.display_name(), "'return'");
        assert_eq!(Token::Break.display_name(), "'break'");
        assert_eq!(Token::Print.display_name(), "'print'");
    }

    #[test]
    fn display_name_type_keywords_exact() {
        assert_eq!(Token::Vec.display_name(), "'vec'");
        assert_eq!(Token::Matrix.display_name(), "'matrix'");
        assert_eq!(Token::Norm.display_name(), "'norm'");
        assert_eq!(Token::Int.display_name(), "'int'");
    }

    #[test]
    fn display_name_builtin_keywords_exact() {
        assert_eq!(Token::Add.display_name(), "'add'");
        assert_eq!(Token::Silu.display_name(), "'silu'");
        assert_eq!(Token::Negate.display_name(), "'negate'");
        assert_eq!(Token::Abs.display_name(), "'abs'");
        assert_eq!(Token::ReduceSum.display_name(), "'reduce_sum'");
    }

    #[test]
    fn display_name_operators_exact() {
        assert_eq!(Token::Eq.display_name(), "'=='");
        assert_eq!(Token::Ne.display_name(), "'!='");
        assert_eq!(Token::Lt.display_name(), "'<'");
        assert_eq!(Token::Gt.display_name(), "'>'");
        assert_eq!(Token::Le.display_name(), "'<='");
        assert_eq!(Token::Ge.display_name(), "'>='");
        assert_eq!(Token::Assign.display_name(), "'='");
        assert_eq!(Token::Plus.display_name(), "'+'");
        assert_eq!(Token::Minus.display_name(), "'-'");
        assert_eq!(Token::Star.display_name(), "'*'");
        assert_eq!(Token::Slash.display_name(), "'/'");
        assert_eq!(Token::Percent.display_name(), "'%'");
        assert_eq!(Token::Arrow.display_name(), "'->'");
        assert_eq!(Token::Dot.display_name(), "'.'");
    }

    #[test]
    fn display_name_delimiters_exact() {
        assert_eq!(Token::LParen.display_name(), "'('");
        assert_eq!(Token::RParen.display_name(), "')'");
        assert_eq!(Token::LBrace.display_name(), "'{'");
        assert_eq!(Token::RBrace.display_name(), "'}'");
        assert_eq!(Token::LBracket.display_name(), "'['");
        assert_eq!(Token::RBracket.display_name(), "']'");
        assert_eq!(Token::Comma.display_name(), "','");
        assert_eq!(Token::Semi.display_name(), "';'");
        assert_eq!(Token::Colon.display_name(), "':'");
        assert_eq!(Token::Pipe.display_name(), "'|'");
        assert_eq!(Token::Amp.display_name(), "'&'");
    }

    #[test]
    fn display_name_literals_exact() {
        assert_eq!(Token::Ident("x".into()).display_name(), "identifier");
        assert_eq!(Token::IntLit(0).display_name(), "integer literal");
        assert_eq!(Token::FloatLit(0.0).display_name(), "float literal");
        assert_eq!(Token::StringLit("".into()).display_name(), "string literal");
    }

    // ── Located struct ───────────────────────────────────────────────────

    #[test]
    fn located_clone() {
        let loc = Located { token: Token::Fn, line: 1, col: 1 };
        let cloned = loc.clone();
        assert_eq!(cloned.line, 1);
        assert_eq!(cloned.col, 1);
        assert_eq!(cloned.token, Token::Fn);
    }

    #[test]
    fn located_debug_format() {
        let loc = Located { token: Token::IntLit(42), line: 3, col: 5 };
        let debug = format!("{:?}", loc);
        assert!(debug.contains("line: 3"));
        assert!(debug.contains("col: 5"));
    }

    // ── Lexer initialization ─────────────────────────────────────────────

    #[test]
    fn lexer_new_starts_at_line_1_col_1() {
        let lexer = NdaLexer::new("test");
        assert_eq!(lexer.line, 1);
        assert_eq!(lexer.col, 1);
        assert_eq!(lexer.pos, 0);
    }

    // ── Tokenization: complex programs ───────────────────────────────────

    #[test]
    fn lex_multiline_program() {
        let src = "fn main() {\n  let x = 10;\n  let y = 20;\n}";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        // fn main ( ) { let x = 10 ; let y = 20 ; } Eof
        assert_eq!(tokens.len(), 17);
        assert_eq!(tokens.last().unwrap().token, Token::Eof);
    }

    #[test]
    fn lex_nested_braces() {
        let src = "{{}}";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::LBrace);
        assert_eq!(tokens[1].token, Token::LBrace);
        assert_eq!(tokens[2].token, Token::RBrace);
        assert_eq!(tokens[3].token, Token::RBrace);
    }

    #[test]
    fn lex_mixed_operators_sequence() {
        let src = "a+b-c*d/e%f";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        // a + b - c * d / e % f Eof
        assert_eq!(tokens.len(), 12);
    }

    // ── Number edge cases ────────────────────────────────────────────────

    #[test]
    fn lex_large_integer() {
        let src = "999999999";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::IntLit(999999999));
    }

    #[test]
    fn lex_dot_then_number() {
        // The lexer matches '.' as Dot operator before number check,
        // so ".5" becomes Dot + IntLit(5)
        let src = ".5";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Dot);
        assert_eq!(tokens[1].token, Token::IntLit(5));
    }

    #[test]
    fn lex_float_negative_exponent() {
        let src = "1e-2";
        let mut lexer = NdaLexer::new(src);
        // The lexer reads '1', then 'e', then '-' stops the number (so 1e is invalid)
        // Actually: the lexer reads digits, '.', 'e', 'E' — so '1e' then '-' stops
        // Let's see what happens: "1e" is parsed, then '-' stops
        // Actually, looking at the code: it reads '1', then 'e' (is_float=true), then '-' is not digit/dot/e/E so stops
        // "1e" won't parse as f64 → error
        // But wait, the test for "1e10" works. "1e-2" would read "1e" then stop at '-'.
        // "1e" doesn't parse → error
        let result = lexer.tokenize();
        // This will error because "1e" is not a valid float
        assert!(result.is_err());
    }

    #[test]
    fn lex_hex_zero() {
        let src = "0x0";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::IntLit(0));
    }

    #[test]
    fn lex_multiple_numbers() {
        let src = "1 2 3 4 5";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::IntLit(1));
        assert_eq!(tokens[1].token, Token::IntLit(2));
        assert_eq!(tokens[2].token, Token::IntLit(3));
        assert_eq!(tokens[3].token, Token::IntLit(4));
        assert_eq!(tokens[4].token, Token::IntLit(5));
    }

    // ── String edge cases ────────────────────────────────────────────────

    #[test]
    fn lex_string_with_spaces() {
        let src = r#""hello   world""#;
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::StringLit("hello   world".to_string()));
    }

    #[test]
    fn lex_two_adjacent_strings() {
        let src = r#""a""b""#;
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::StringLit("a".to_string()));
        assert_eq!(tokens[1].token, Token::StringLit("b".to_string()));
    }

    // ── Identifier edge cases ────────────────────────────────────────────

    #[test]
    fn lex_single_letter_ident() {
        let src = "x";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Ident("x".to_string()));
    }

    #[test]
    fn lex_underscore_only_ident() {
        let src = "_";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Ident("_".to_string()));
    }

    #[test]
    fn lex_long_ident() {
        let src = "abcdefghijklmnopqrstuvwxyz";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Ident("abcdefghijklmnopqrstuvwxyz".to_string()));
    }

    // ── Keyword boundaries ───────────────────────────────────────────────

    #[test]
    fn lex_fn_not_keyword_when_followed_by_alpha() {
        let src = "fnx";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Ident("fnx".to_string()));
    }

    #[test]
    fn lex_let_not_keyword_with_digits() {
        let src = "let1";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Ident("let1".to_string()));
    }

    #[test]
    fn lex_if_followed_by_underscore() {
        let src = "if_cond";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Ident("if_cond".to_string()));
    }

    // ── Error recovery details ───────────────────────────────────────────

    #[test]
    fn error_recovery_error_text_contains_line_col() {
        let src = "@";
        let mut lexer = NdaLexer::new(src);
        let (_, errors) = lexer.tokenize_with_errors();
        assert!(!errors.is_empty());
        assert!(errors[0].contains("1:"));
    }

    #[test]
    fn error_recovery_preserves_good_tokens() {
        let src = "let @ = 1";
        let mut lexer = NdaLexer::new(src);
        let (tokens, errors) = lexer.tokenize_with_errors();
        assert_eq!(errors.len(), 1);
        // Should have: let, =, 1, Eof (plus possibly more)
        assert!(tokens.iter().any(|t| t.token == Token::Let));
        assert!(tokens.iter().any(|t| t.token == Token::IntLit(1)));
    }

    #[test]
    fn error_recovery_ends_with_eof() {
        let src = "@@@";
        let mut lexer = NdaLexer::new(src);
        let (tokens, errors) = lexer.tokenize_with_errors();
        assert_eq!(tokens.last().unwrap().token, Token::Eof);
        assert_eq!(errors.len(), 3);
    }

    // ── Line/col tracking advanced ───────────────────────────────────────

    #[test]
    fn lex_col_tracking_after_newline() {
        let src = "a\n  b";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].line, 1);
        assert_eq!(tokens[0].col, 1);
        assert_eq!(tokens[1].line, 2);
        assert_eq!(tokens[1].col, 3); // after 2 spaces
    }

    #[test]
    fn lex_multiple_newlines_tracking() {
        let src = "a\n\n\nb";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].line, 1); // a
        assert_eq!(tokens[1].line, 4); // b
    }

    // ── Comment edge cases ───────────────────────────────────────────────

    #[test]
    fn lex_comment_at_end_of_file() {
        let src = "x // comment";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Ident("x".to_string()));
        assert_eq!(tokens[1].token, Token::Eof);
    }

    #[test]
    fn lex_multiple_comments() {
        let src = "// first\n// second\nx";
        let mut lexer = NdaLexer::new(src);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Ident("x".to_string()));
        assert_eq!(tokens[1].token, Token::Eof);
    }

    // ── Token equality ───────────────────────────────────────────────────

    #[test]
    fn token_eq_same_variant() {
        assert_eq!(Token::Fn, Token::Fn);
        assert_eq!(Token::IntLit(42), Token::IntLit(42));
        assert_eq!(Token::Ident("x".into()), Token::Ident("x".into()));
    }

    #[test]
    fn token_ne_different_variant() {
        assert_ne!(Token::Fn, Token::Let);
        assert_ne!(Token::IntLit(1), Token::IntLit(2));
        assert_ne!(Token::Ident("a".into()), Token::Ident("b".into()));
    }
}
