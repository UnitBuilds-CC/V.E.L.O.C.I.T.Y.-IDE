#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Literals
    Number(f64),
    Str(String),
    Template(String),
    Ident(String),
    // Keywords
    Var, Let, Const, Function, Return, If, Else, While, For, Do,
    Break, Continue, Throw, Try, Catch, Finally, New, Typeof, Instanceof,
    In, Of, True, False, Null, Undefined, This, Void, Delete,
    Class, Extends, Super, Static, Async, Await,
    Import, Export, From, Default, As, Yield,
    Switch, Case,
    // Punctuation
    Plus, Minus, Star, Slash, Percent, StarStar,
    Bang, AmpAmp, PipePipe, QuestionQuestion,
    EqEq, BangEq, EqEqEq, BangEqEq,
    Lt, Gt, LtEq, GtEq,
    Eq, PlusEq, MinusEq, StarEq, SlashEq, QuestionQuestionEq,
    Amp, Pipe, Caret, Tilde, LtLt, GtGt, GtGtGt,
    LParen, RParen, LBrace, RBrace, LBracket, RBracket,
    Dot, DotDotDot, QuestionDot, Comma, Colon, Semi, Question, Arrow,
    PlusPlus, MinusMinus,
    Eof,
}
