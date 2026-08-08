use super::token::*;

pub fn lex(input: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = input.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\n' | '\r' => { i += 1; }
            '/' if i + 1 < chars.len() && chars[i + 1] == '/' => {
                while i < chars.len() && chars[i] != '\n' { i += 1; }
            }
            '/' if i + 1 < chars.len() && chars[i + 1] == '*' => {
                i += 2;
                while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') { i += 1; }
                i += 2;
            }
            '+' => { i += 1; tokens.push(if i < chars.len() && chars[i] == '+' { i += 1; Token::PlusPlus } else if i < chars.len() && chars[i] == '=' { i += 1; Token::PlusEq } else { Token::Plus }); }
            '-' => { i += 1; tokens.push(if i < chars.len() && chars[i] == '-' { i += 1; Token::MinusMinus } else if i < chars.len() && chars[i] == '=' { i += 1; Token::MinusEq } else { Token::Minus }); }
            '*' => { i += 1; tokens.push(if i < chars.len() && chars[i] == '*' { i += 1; Token::StarStar } else if i < chars.len() && chars[i] == '=' { i += 1; Token::StarEq } else { Token::Star }); }
            '/' => { i += 1; tokens.push(if i < chars.len() && chars[i] == '=' { i += 1; Token::SlashEq } else { Token::Slash }); }
            '%' => { i += 1; tokens.push(Token::Percent); }
            '(' => { i += 1; tokens.push(Token::LParen); }
            ')' => { i += 1; tokens.push(Token::RParen); }
            '{' => { i += 1; tokens.push(Token::LBrace); }
            '}' => { i += 1; tokens.push(Token::RBrace); }
            '[' => { i += 1; tokens.push(Token::LBracket); }
            ']' => { i += 1; tokens.push(Token::RBracket); }
            ',' => { i += 1; tokens.push(Token::Comma); }
            ':' => { i += 1; tokens.push(Token::Colon); }
            ';' => { i += 1; tokens.push(Token::Semi); }
            '~' => { i += 1; tokens.push(Token::Tilde); }
            '?' => { i += 1; if i < chars.len() && chars[i] == '?' { i += 1; tokens.push(if i < chars.len() && chars[i] == '=' { i += 1; Token::QuestionQuestionEq } else { Token::QuestionQuestion }); } else if i < chars.len() && chars[i] == '.' && (i + 1 >= chars.len() || !chars[i + 1].is_ascii_digit()) { i += 1; tokens.push(Token::QuestionDot); } else { tokens.push(Token::Question); } }
            '.' => {
                if i + 2 < chars.len() && chars[i + 1] == '.' && chars[i + 2] == '.' {
                    i += 3; tokens.push(Token::DotDotDot);
                } else if i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
                    let mut num = String::from("0.");
                    i += 1;
                    while i < chars.len() && (chars[i].is_ascii_digit()) { num.push(chars[i]); i += 1; }
                    tokens.push(Token::Number(num.parse().unwrap_or(0.0)));
                } else {
                    i += 1; tokens.push(Token::Dot);
                }
            }
            '!' => { i += 1; if i < chars.len() && chars[i] == '=' { i += 1; tokens.push(if i < chars.len() && chars[i] == '=' { i += 1; Token::BangEqEq } else { Token::BangEq }); } else { tokens.push(Token::Bang); } }
            '=' => { i += 1; if i < chars.len() && chars[i] == '=' { i += 1; tokens.push(if i < chars.len() && chars[i] == '=' { i += 1; Token::EqEqEq } else { Token::EqEq }); } else if i < chars.len() && chars[i] == '>' { i += 1; tokens.push(Token::Arrow); } else { tokens.push(Token::Eq); } }
            '&' => { i += 1; tokens.push(if i < chars.len() && chars[i] == '&' { i += 1; Token::AmpAmp } else { Token::Amp }); }
            '|' => { i += 1; tokens.push(if i < chars.len() && chars[i] == '|' { i += 1; Token::PipePipe } else { Token::Pipe }); }
            '^' => { i += 1; tokens.push(Token::Caret); }
            '<' => { i += 1; if i < chars.len() && chars[i] == '=' { i += 1; tokens.push(Token::LtEq); } else if i < chars.len() && chars[i] == '<' { i += 1; tokens.push(Token::LtLt); } else { tokens.push(Token::Lt); } }
            '>' => { i += 1; if i < chars.len() && chars[i] == '=' { i += 1; tokens.push(Token::GtEq); } else if i < chars.len() && chars[i] == '>' { i += 1; if i < chars.len() && chars[i] == '>' { i += 1; tokens.push(Token::GtGtGt); } else { tokens.push(Token::GtGt); } } else { tokens.push(Token::Gt); } }
            '"' | '\'' => {
                let quote = c; i += 1;
                let mut s = String::new();
                while i < chars.len() && chars[i] != quote {
                    if chars[i] == '\\' && i + 1 < chars.len() { i += 1; s.push(match chars[i] { 'n' => '\n', 't' => '\t', 'r' => '\r', '0' => '\0', o => o }); }
                    else { s.push(chars[i]); }
                    i += 1;
                }
                if i < chars.len() { i += 1; }
                tokens.push(Token::Str(s));
            }
            '`' => {
                i += 1;
                let mut s = String::new();
                while i < chars.len() && chars[i] != '`' {
                    if chars[i] == '\\' && i + 1 < chars.len() { i += 1; s.push(match chars[i] { 'n' => '\n', 't' => '\t', o => o }); }
                    else { s.push(chars[i]); }
                    i += 1;
                }
                if i < chars.len() { i += 1; }
                tokens.push(Token::Template(s));
            }
            c if c.is_ascii_digit() => {
                let mut num = String::new();
                if c == '0' && i + 1 < chars.len() && (chars[i + 1] == 'x' || chars[i + 1] == 'X') {
                    i += 2;
                    while i < chars.len() && chars[i].is_ascii_hexdigit() { num.push(chars[i]); i += 1; }
                    tokens.push(Token::Number(i64::from_str_radix(&num, 16).unwrap_or(0) as f64));
                } else {
                    while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') { num.push(chars[i]); i += 1; }
                    if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                        num.push(chars[i]); i += 1;
                        if i < chars.len() && (chars[i] == '+' || chars[i] == '-') { num.push(chars[i]); i += 1; }
                        while i < chars.len() && chars[i].is_ascii_digit() { num.push(chars[i]); i += 1; }
                    }
                    tokens.push(Token::Number(num.parse().unwrap_or(f64::NAN)));
                }
            }
            c if c.is_alphabetic() || c == '_' || c == '$' => {
                let mut id = String::new();
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$') { id.push(chars[i]); i += 1; }
                tokens.push(match id.as_str() {
                    "var" => Token::Var, "let" => Token::Let, "const" => Token::Const,
                    "function" => Token::Function, "return" => Token::Return,
                    "if" => Token::If, "else" => Token::Else,
                    "while" => Token::While, "for" => Token::For, "do" => Token::Do,
                    "break" => Token::Break, "continue" => Token::Continue,
                    "throw" => Token::Throw, "try" => Token::Try,
                    "catch" => Token::Catch, "finally" => Token::Finally,
                    "new" => Token::New, "typeof" => Token::Typeof, "instanceof" => Token::Instanceof,
                    "in" => Token::In, "of" => Token::Of,
                    "true" => Token::True, "false" => Token::False,
                    "null" => Token::Null, "undefined" => Token::Undefined,
                    "this" => Token::This, "void" => Token::Void, "delete" => Token::Delete,
                    "class" => Token::Class, "extends" => Token::Extends,
                    "super" => Token::Super, "static" => Token::Static,
                    "async" => Token::Async, "await" => Token::Await,
                    "import" => Token::Import, "export" => Token::Export,
                    "from" => Token::From, "default" => Token::Default, "as" => Token::As,
                    "yield" => Token::Yield,
                    "switch" => Token::Switch, "case" => Token::Case,
                    _ => Token::Ident(id),
                });
            }
            other => return Err(format!("unexpected character '{}'", other)),
        }
    }
    tokens.push(Token::Eof);
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_returns_eof() {
        let tokens = lex("").unwrap();
        assert_eq!(tokens, vec![Token::Eof]);
    }

    #[test]
    fn whitespace_only_returns_eof() {
        let tokens = lex("   \t\n\r  ").unwrap();
        assert_eq!(tokens, vec![Token::Eof]);
    }

    #[test]
    fn arithmetic_operators() {
        let tokens = lex("+ - * / % **").unwrap();
        assert_eq!(tokens, vec![
            Token::Plus, Token::Minus, Token::Star, Token::Slash,
            Token::Percent, Token::StarStar, Token::Eof,
        ]);
    }

    #[test]
    fn compound_assignment_operators() {
        let tokens = lex("+= -= *= /=").unwrap();
        assert_eq!(tokens, vec![
            Token::PlusEq, Token::MinusEq, Token::StarEq, Token::SlashEq, Token::Eof,
        ]);
    }

    #[test]
    fn increment_decrement() {
        let tokens = lex("++ --").unwrap();
        assert_eq!(tokens, vec![Token::PlusPlus, Token::MinusMinus, Token::Eof]);
    }

    #[test]
    fn comparison_operators() {
        let tokens = lex("< > <= >= == != === !==").unwrap();
        assert_eq!(tokens, vec![
            Token::Lt, Token::Gt, Token::LtEq, Token::GtEq,
            Token::EqEq, Token::BangEq, Token::EqEqEq, Token::BangEqEq,
            Token::Eof,
        ]);
    }

    #[test]
    fn logical_and_bitwise_operators() {
        let tokens = lex("&& || ! & | ^ ~").unwrap();
        assert_eq!(tokens, vec![
            Token::AmpAmp, Token::PipePipe, Token::Bang,
            Token::Amp, Token::Pipe, Token::Caret, Token::Tilde,
            Token::Eof,
        ]);
    }

    #[test]
    fn shift_operators() {
        let tokens = lex("<< >> >>>").unwrap();
        assert_eq!(tokens, vec![Token::LtLt, Token::GtGt, Token::GtGtGt, Token::Eof]);
    }

    #[test]
    fn punctuation_tokens() {
        let tokens = lex("( ) { } [ ] , : ; . ... ?").unwrap();
        assert_eq!(tokens, vec![
            Token::LParen, Token::RParen, Token::LBrace, Token::RBrace,
            Token::LBracket, Token::RBracket, Token::Comma, Token::Colon,
            Token::Semi, Token::Dot, Token::DotDotDot, Token::Question,
            Token::Eof,
        ]);
    }

    #[test]
    fn arrow_and_nullish() {
        let tokens = lex("=> ?? ??= ?.").unwrap();
        assert_eq!(tokens, vec![
            Token::Arrow, Token::QuestionQuestion, Token::QuestionQuestionEq,
            Token::QuestionDot, Token::Eof,
        ]);
    }

    #[test]
    fn string_literal_double_quote() {
        let tokens = lex(r#""hello""#).unwrap();
        assert_eq!(tokens, vec![Token::Str("hello".into()), Token::Eof]);
    }

    #[test]
    fn string_literal_single_quote_with_escapes() {
        let tokens = lex(r#"'a\nb\tc'"#).unwrap();
        assert_eq!(tokens, vec![Token::Str("a\nb\tc".into()), Token::Eof]);
    }

    #[test]
    fn template_literal() {
        let tokens = lex("`hello world`").unwrap();
        assert_eq!(tokens, vec![Token::Template("hello world".into()), Token::Eof]);
    }

    #[test]
    fn decimal_number() {
        let tokens = lex("42 2.72").unwrap();
        assert_eq!(tokens, vec![Token::Number(42.0), Token::Number(2.72), Token::Eof]);
    }

    #[test]
    fn hex_number() {
        let tokens = lex("0xff 0X10").unwrap();
        assert_eq!(tokens, vec![Token::Number(255.0), Token::Number(16.0), Token::Eof]);
    }

    #[test]
    fn scientific_notation() {
        let tokens = lex("1e3 2.5e-2").unwrap();
        assert_eq!(tokens, vec![Token::Number(1000.0), Token::Number(0.025), Token::Eof]);
    }

    #[test]
    fn dot_followed_by_digit() {
        // `.5` is lexed as the number 0.5
        let tokens = lex(".5").unwrap();
        assert_eq!(tokens, vec![Token::Number(0.5), Token::Eof]);
    }

    #[test]
    fn keywords() {
        let tokens = lex("var let const function return if else while for do break continue").unwrap();
        assert_eq!(tokens, vec![
            Token::Var, Token::Let, Token::Const, Token::Function,
            Token::Return, Token::If, Token::Else, Token::While,
            Token::For, Token::Do, Token::Break, Token::Continue,
            Token::Eof,
        ]);
    }

    #[test]
    fn more_keywords() {
        let tokens = lex("throw try catch finally new typeof instanceof in of").unwrap();
        assert_eq!(tokens, vec![
            Token::Throw, Token::Try, Token::Catch, Token::Finally,
            Token::New, Token::Typeof, Token::Instanceof, Token::In, Token::Of,
            Token::Eof,
        ]);
    }

    #[test]
    fn literal_keywords() {
        let tokens = lex("true false null undefined this void delete").unwrap();
        assert_eq!(tokens, vec![
            Token::True, Token::False, Token::Null, Token::Undefined,
            Token::This, Token::Void, Token::Delete, Token::Eof,
        ]);
    }

    #[test]
    fn class_and_module_keywords() {
        let tokens = lex("class extends super static async await import export from default as yield switch case").unwrap();
        assert_eq!(tokens, vec![
            Token::Class, Token::Extends, Token::Super, Token::Static,
            Token::Async, Token::Await, Token::Import, Token::Export,
            Token::From, Token::Default, Token::As, Token::Yield,
            Token::Switch, Token::Case, Token::Eof,
        ]);
    }

    #[test]
    fn identifier() {
        let tokens = lex("foo bar_baz $el _private").unwrap();
        assert_eq!(tokens, vec![
            Token::Ident("foo".into()),
            Token::Ident("bar_baz".into()),
            Token::Ident("$el".into()),
            Token::Ident("_private".into()),
            Token::Eof,
        ]);
    }

    #[test]
    fn single_line_comment() {
        let tokens = lex("x // comment\ny").unwrap();
        assert_eq!(tokens, vec![Token::Ident("x".into()), Token::Ident("y".into()), Token::Eof]);
    }

    #[test]
    fn multi_line_comment() {
        let tokens = lex("a /* skip\nthis */ b").unwrap();
        assert_eq!(tokens, vec![Token::Ident("a".into()), Token::Ident("b".into()), Token::Eof]);
    }

    #[test]
    fn unexpected_character_returns_err() {
        assert!(lex("@").is_err());
        assert!(lex("#").is_err());
    }

    #[test]
    fn full_expression_lex() {
        let tokens = lex("var x = 1 + 2;").unwrap();
        assert_eq!(tokens, vec![
            Token::Var, Token::Ident("x".into()), Token::Eq,
            Token::Number(1.0), Token::Plus, Token::Number(2.0),
            Token::Semi, Token::Eof,
        ]);
    }
}
