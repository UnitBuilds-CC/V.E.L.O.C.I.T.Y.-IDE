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
