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
    Arrow,  // ->
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
    // End
    Eof,
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

        // Numbers (integers and floats)
        if ch.is_ascii_digit()
            || (ch == '.' && self.peek_next().is_some_and(|c| c.is_ascii_digit()))
        {
            return self.lex_number(line, col);
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
}
