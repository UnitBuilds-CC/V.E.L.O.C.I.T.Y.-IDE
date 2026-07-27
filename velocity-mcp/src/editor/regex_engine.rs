//! A small, dependency-free regular-expression engine used by the editor's
//! Find & Replace. It compiles a practical subset of regex syntax to a
//! bytecode program and runs it with a backtracking VM that memoizes failed
//! `(pc, pos)` states, guaranteeing termination in `O(program_len * input_len)`
//! without stack-overflow risk.
//!
//! Supported syntax:
//!   * literals and escaped metacharacters (`\.`, `\*`, `\\`, `\n`, `\t`, `\r`)
//!   * `.` (any char except newline)
//!   * character classes `[abc]`, ranges `[a-z]`, negation `[^...]`
//!   * class shorthands `\d \D \w \W \s \S` (also usable inside `[...]`)
//!   * anchors `^` and `$` (line-anchored: match at start/end of any line)
//!   * grouping `( ... )` and alternation `a|b`
//!   * greedy quantifiers `*`, `+`, `?`, `{n}`, `{n,}`, `{n,m}`
//!
//! Capturing is not exposed — Find & Replace only needs whole-match spans, so
//! the replacement is inserted literally.

use std::collections::HashSet;

/// One item inside a character class.
#[derive(Debug, Clone)]
enum ClassItem {
    Ch(char),
    Range(char, char),
    Digit,
    NotDigit,
    Word,
    NotWord,
    Space,
    NotSpace,
}

/// A compiled character class.
#[derive(Debug, Clone)]
struct CharClass {
    negated: bool,
    items: Vec<ClassItem>,
}

impl CharClass {
    fn matches(&self, c: char, case_insensitive: bool) -> bool {
        let hit = self.items.iter().any(|it| item_matches(it, c, case_insensitive));
        hit ^ self.negated
    }
}

fn item_matches(item: &ClassItem, c: char, ci: bool) -> bool {
    match item {
        ClassItem::Ch(t) => char_eq(*t, c, ci),
        ClassItem::Range(a, b) => {
            if *a <= c && c <= *b {
                return true;
            }
            if ci {
                let lc = c.to_ascii_lowercase();
                let uc = c.to_ascii_uppercase();
                (*a <= lc && lc <= *b) || (*a <= uc && uc <= *b)
            } else {
                false
            }
        }
        ClassItem::Digit => c.is_ascii_digit(),
        ClassItem::NotDigit => !c.is_ascii_digit(),
        ClassItem::Word => c.is_alphanumeric() || c == '_',
        ClassItem::NotWord => !(c.is_alphanumeric() || c == '_'),
        ClassItem::Space => c.is_whitespace(),
        ClassItem::NotSpace => !c.is_whitespace(),
    }
}

fn char_eq(a: char, b: char, ci: bool) -> bool {
    if a == b {
        return true;
    }
    if ci {
        a.eq_ignore_ascii_case(&b)
    } else {
        false
    }
}

// ─── AST ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum Ast {
    Char(char),
    Any,
    Class(usize),
    Start,
    End,
    Concat(Vec<Ast>),
    Alt(Vec<Ast>),
    Repeat {
        node: Box<Ast>,
        min: usize,
        max: Option<usize>,
    },
    Empty,
}

// ─── Parser ────────────────────────────────────────────────────────────────

struct Parser {
    chars: Vec<char>,
    pos: usize,
    classes: Vec<CharClass>,
}

impl Parser {
    fn new(pattern: &str) -> Self {
        Self {
            chars: pattern.chars().collect(),
            pos: 0,
            classes: Vec::new(),
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.chars.get(self.pos).copied();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn parse(&mut self) -> Result<Ast, String> {
        let node = self.parse_alt()?;
        if self.pos != self.chars.len() {
            return Err(format!("unexpected character at position {}", self.pos));
        }
        Ok(node)
    }

    fn parse_alt(&mut self) -> Result<Ast, String> {
        let mut branches = vec![self.parse_concat()?];
        while self.peek() == Some('|') {
            self.bump();
            branches.push(self.parse_concat()?);
        }
        if branches.len() == 1 {
            Ok(branches.pop().unwrap())
        } else {
            Ok(Ast::Alt(branches))
        }
    }

    fn parse_concat(&mut self) -> Result<Ast, String> {
        let mut nodes = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            nodes.push(self.parse_repeat()?);
        }
        if nodes.is_empty() {
            Ok(Ast::Empty)
        } else if nodes.len() == 1 {
            Ok(nodes.pop().unwrap())
        } else {
            Ok(Ast::Concat(nodes))
        }
    }

    fn parse_repeat(&mut self) -> Result<Ast, String> {
        let atom = self.parse_atom()?;
        match self.peek() {
            Some('*') => {
                self.bump();
                Ok(Ast::Repeat { node: Box::new(atom), min: 0, max: None })
            }
            Some('+') => {
                self.bump();
                Ok(Ast::Repeat { node: Box::new(atom), min: 1, max: None })
            }
            Some('?') => {
                self.bump();
                Ok(Ast::Repeat { node: Box::new(atom), min: 0, max: Some(1) })
            }
            Some('{') => {
                let (min, max) = self.parse_brace()?;
                Ok(Ast::Repeat { node: Box::new(atom), min, max })
            }
            _ => Ok(atom),
        }
    }

    fn parse_brace(&mut self) -> Result<(usize, Option<usize>), String> {
        self.bump(); // consume '{'
        let mut min_str = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                min_str.push(c);
                self.bump();
            } else {
                break;
            }
        }
        let min: usize = min_str.parse().map_err(|_| "invalid {n} quantifier".to_string())?;
        let max = if self.peek() == Some(',') {
            self.bump();
            let mut max_str = String::new();
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    max_str.push(c);
                    self.bump();
                } else {
                    break;
                }
            }
            if max_str.is_empty() {
                None
            } else {
                Some(max_str.parse().map_err(|_| "invalid {n,m} quantifier".to_string())?)
            }
        } else {
            Some(min)
        };
        if self.bump() != Some('}') {
            return Err("unterminated { quantifier".to_string());
        }
        Ok((min, max))
    }

    fn parse_atom(&mut self) -> Result<Ast, String> {
        match self.peek() {
            Some('(') => {
                self.bump();
                // Support non-capturing group prefix (?:...) transparently.
                if self.peek() == Some('?') {
                    let save = self.pos;
                    self.bump();
                    if self.peek() == Some(':') {
                        self.bump();
                    } else {
                        self.pos = save;
                    }
                }
                let inner = self.parse_alt()?;
                if self.bump() != Some(')') {
                    return Err("unclosed group".to_string());
                }
                Ok(inner)
            }
            Some('[') => self.parse_class(),
            Some('.') => {
                self.bump();
                Ok(Ast::Any)
            }
            Some('^') => {
                self.bump();
                Ok(Ast::Start)
            }
            Some('$') => {
                self.bump();
                Ok(Ast::End)
            }
            Some('\\') => {
                self.bump();
                let esc = self.bump().ok_or("dangling escape")?;
                if let Some(item) = shorthand_class(esc) {
                    let idx = self.classes.len();
                    self.classes.push(CharClass { negated: false, items: vec![item] });
                    Ok(Ast::Class(idx))
                } else {
                    Ok(Ast::Char(unescape(esc)))
                }
            }
            Some(c) => {
                self.bump();
                Ok(Ast::Char(c))
            }
            None => Ok(Ast::Empty),
        }
    }

    fn parse_class(&mut self) -> Result<Ast, String> {
        self.bump(); // consume '['
        let negated = if self.peek() == Some('^') {
            self.bump();
            true
        } else {
            false
        };
        let mut items = Vec::new();
        loop {
            match self.peek() {
                None => return Err("unterminated character class".to_string()),
                Some(']') => {
                    self.bump();
                    break;
                }
                Some('\\') => {
                    self.bump();
                    let esc = self.bump().ok_or("dangling escape in class")?;
                    if let Some(item) = shorthand_class(esc) {
                        items.push(item);
                    } else {
                        items.push(ClassItem::Ch(unescape(esc)));
                    }
                }
                Some(c) => {
                    self.bump();
                    // Range like a-z (but not a trailing '-').
                    if self.peek() == Some('-')
                        && self.chars.get(self.pos + 1).is_some_and(|&n| n != ']')
                    {
                        self.bump(); // consume '-'
                        let end = self.bump().unwrap();
                        let end = if end == '\\' {
                            unescape(self.bump().ok_or("dangling escape in range")?)
                        } else {
                            end
                        };
                        items.push(ClassItem::Range(c, end));
                    } else {
                        items.push(ClassItem::Ch(c));
                    }
                }
            }
        }
        let idx = self.classes.len();
        self.classes.push(CharClass { negated, items });
        Ok(Ast::Class(idx))
    }
}

fn shorthand_class(esc: char) -> Option<ClassItem> {
    match esc {
        'd' => Some(ClassItem::Digit),
        'D' => Some(ClassItem::NotDigit),
        'w' => Some(ClassItem::Word),
        'W' => Some(ClassItem::NotWord),
        's' => Some(ClassItem::Space),
        'S' => Some(ClassItem::NotSpace),
        _ => None,
    }
}

fn unescape(esc: char) -> char {
    match esc {
        'n' => '\n',
        't' => '\t',
        'r' => '\r',
        '0' => '\0',
        other => other,
    }
}

// ─── Bytecode ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum Inst {
    Char(char),
    Any,
    Class(usize),
    /// Match start of text or immediately after a newline.
    AssertLineStart,
    /// Match end of text or immediately before a newline.
    AssertLineEnd,
    Jmp(usize),
    /// Try `x` first (greedy), then fall back to `y`.
    Split(usize, usize),
    Match,
}

/// A compiled regular expression.
#[derive(Debug, Clone)]
pub struct Regex {
    insts: Vec<Inst>,
    classes: Vec<CharClass>,
    case_insensitive: bool,
}

struct Compiler {
    insts: Vec<Inst>,
}

impl Compiler {
    fn emit(&mut self, inst: Inst) -> usize {
        let idx = self.insts.len();
        self.insts.push(inst);
        idx
    }

    fn compile(&mut self, node: &Ast) {
        match node {
            Ast::Empty => {}
            Ast::Char(c) => {
                self.emit(Inst::Char(*c));
            }
            Ast::Any => {
                self.emit(Inst::Any);
            }
            Ast::Class(i) => {
                self.emit(Inst::Class(*i));
            }
            Ast::Start => {
                self.emit(Inst::AssertLineStart);
            }
            Ast::End => {
                self.emit(Inst::AssertLineEnd);
            }
            Ast::Concat(nodes) => {
                for n in nodes {
                    self.compile(n);
                }
            }
            Ast::Alt(branches) => {
                // Chain of splits: Split(b0, next); b0; Jmp end; ...
                let mut jmp_ends = Vec::new();
                for (i, branch) in branches.iter().enumerate() {
                    let is_last = i == branches.len() - 1;
                    if is_last {
                        self.compile(branch);
                    } else {
                        let split = self.emit(Inst::Split(0, 0));
                        let branch_start = self.insts.len();
                        self.compile(branch);
                        let jmp = self.emit(Inst::Jmp(0));
                        jmp_ends.push(jmp);
                        let alt_start = self.insts.len();
                        self.insts[split] = Inst::Split(branch_start, alt_start);
                    }
                }
                let end = self.insts.len();
                for j in jmp_ends {
                    self.insts[j] = Inst::Jmp(end);
                }
            }
            Ast::Repeat { node, min, max } => {
                self.compile_repeat(node, *min, *max);
            }
        }
    }

    fn compile_repeat(&mut self, node: &Ast, min: usize, max: Option<usize>) {
        // Emit `min` mandatory copies.
        for _ in 0..min {
            self.compile(node);
        }
        match max {
            None => {
                // Unbounded tail: greedy star.
                // L1: Split(body, end); body; Jmp L1; end:
                let l1 = self.emit(Inst::Split(0, 0));
                let body = self.insts.len();
                self.compile(node);
                self.emit(Inst::Jmp(l1));
                let end = self.insts.len();
                self.insts[l1] = Inst::Split(body, end);
            }
            Some(max) => {
                // `max - min` optional copies.
                let optional = max.saturating_sub(min);
                let mut splits = Vec::new();
                for _ in 0..optional {
                    let split = self.emit(Inst::Split(0, 0));
                    let body = self.insts.len();
                    splits.push((split, body));
                    self.compile(node);
                }
                let end = self.insts.len();
                for (split, body) in splits {
                    self.insts[split] = Inst::Split(body, end);
                }
            }
        }
    }
}

impl Regex {
    /// Compile `pattern`. Returns an error string on invalid syntax.
    pub fn compile(pattern: &str, case_insensitive: bool) -> Result<Self, String> {
        let mut parser = Parser::new(pattern);
        let ast = parser.parse()?;
        let mut compiler = Compiler { insts: Vec::new() };
        compiler.compile(&ast);
        compiler.emit(Inst::Match);
        Ok(Self {
            insts: compiler.insts,
            classes: parser.classes,
            case_insensitive,
        })
    }

    /// Attempt a match anchored at char index `start`; returns the end char
    /// index on success.
    fn run_at(&self, chars: &[char], start: usize) -> Option<usize> {
        let mut stack: Vec<(usize, usize)> = vec![(0, start)];
        // Memoize dead `(pc, pos)` states: behaviour depends only on that pair,
        // so a state that failed once can never succeed later.
        let mut dead: HashSet<(usize, usize)> = HashSet::new();
        while let Some((mut pc, mut pos)) = stack.pop() {
            loop {
                if !dead.insert((pc, pos)) {
                    break;
                }
                match &self.insts[pc] {
                    Inst::Match => return Some(pos),
                    Inst::Char(c) => {
                        if pos < chars.len() && char_eq(*c, chars[pos], self.case_insensitive) {
                            pc += 1;
                            pos += 1;
                        } else {
                            break;
                        }
                    }
                    Inst::Any => {
                        if pos < chars.len() && chars[pos] != '\n' {
                            pc += 1;
                            pos += 1;
                        } else {
                            break;
                        }
                    }
                    Inst::Class(i) => {
                        if pos < chars.len()
                            && self.classes[*i].matches(chars[pos], self.case_insensitive)
                        {
                            pc += 1;
                            pos += 1;
                        } else {
                            break;
                        }
                    }
                    Inst::AssertLineStart => {
                        if pos == 0 || chars.get(pos - 1) == Some(&'\n') {
                            pc += 1;
                        } else {
                            break;
                        }
                    }
                    Inst::AssertLineEnd => {
                        if pos == chars.len() || chars.get(pos) == Some(&'\n') {
                            pc += 1;
                        } else {
                            break;
                        }
                    }
                    Inst::Jmp(x) => {
                        pc = *x;
                    }
                    Inst::Split(x, y) => {
                        // Explore `x` first (greedy); `y` is the fallback.
                        stack.push((*y, pos));
                        pc = *x;
                    }
                }
            }
        }
        None
    }

    /// Find all non-overlapping, left-to-right matches in `text`. Returns byte
    /// spans `(start, end)` suitable for slicing the original string.
    pub fn find_all(&self, text: &str) -> Vec<(usize, usize)> {
        let chars: Vec<char> = text.chars().collect();
        // char index -> byte offset (with a trailing entry for end-of-text).
        let mut byte_at = Vec::with_capacity(chars.len() + 1);
        let mut b = 0usize;
        for c in &chars {
            byte_at.push(b);
            b += c.len_utf8();
        }
        byte_at.push(b);

        let mut out = Vec::new();
        let mut i = 0usize;
        while i <= chars.len() {
            if let Some(end) = self.run_at(&chars, i) {
                out.push((byte_at[i], byte_at[end]));
                if end > i {
                    i = end;
                } else {
                    i += 1; // zero-width match: advance to avoid an infinite loop
                }
            } else {
                i += 1;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(pattern: &str, text: &str, ci: bool) -> Vec<(usize, usize)> {
        Regex::compile(pattern, ci).unwrap().find_all(text)
    }

    #[test]
    fn literal_and_dot() {
        assert_eq!(spans("a.c", "abc axc adc", false).len(), 3);
    }

    #[test]
    fn star_greedy() {
        // ab*c matches "ac", "abc", "abbc"
        assert_eq!(spans("ab*c", "ac abc abbc", false).len(), 3);
    }

    #[test]
    fn plus_and_optional() {
        assert_eq!(spans("ab+", "a ab abb", false).len(), 2);
        assert_eq!(spans("colou?r", "color colour", false).len(), 2);
    }

    #[test]
    fn char_class_and_shorthand() {
        assert_eq!(spans(r"\d+", "x12 y345 z", false).len(), 2);
        assert_eq!(spans("[a-c]+", "abc def cba", false).len(), 2);
        assert_eq!(spans("[^0-9]+", "12ab34", false).len(), 1);
    }

    #[test]
    fn alternation_and_group() {
        assert_eq!(spans("cat|dog", "a cat and a dog", false).len(), 2);
        assert_eq!(spans("(ab)+", "ababab x ab", false).len(), 2);
    }

    #[test]
    fn anchors_line() {
        assert_eq!(spans("^foo", "foo\nbar\nfoo", false).len(), 2);
        // Matches the trailing "bar" on every line, including inside "barbar".
        assert_eq!(spans("bar$", "bar\nbarbar\nbar", false).len(), 3);
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(spans("hello", "Hello HELLO hello", true).len(), 3);
        assert_eq!(spans("[a-z]+", "ABC", true).len(), 1);
    }

    #[test]
    fn quantifier_braces() {
        // Matches "aa", "aaa", and the greedy "aaa" prefix of "aaaa".
        assert_eq!(spans("a{2,3}", "a aa aaa aaaa", false).len(), 3);
    }

    #[test]
    fn invalid_regex_errors() {
        assert!(Regex::compile("(unclosed", false).is_err());
        assert!(Regex::compile("[unterminated", false).is_err());
    }

    #[test]
    fn byte_offsets_utf8() {
        // Ensure multibyte chars don't corrupt spans.
        let s = "héllo world héllo";
        let m = spans("héllo", s, false);
        assert_eq!(m.len(), 2);
        for (a, b) in m {
            assert_eq!(&s[a..b], "héllo");
        }
    }
}
