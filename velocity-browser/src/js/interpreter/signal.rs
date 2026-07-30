use crate::js::vm::JsValue;

/// Control flow signals propagated through Result.
#[derive(Debug, Clone)]
pub enum Signal {
    Return(JsValue),
    Break,
    Continue,
    Throw(JsValue),
}

pub type EvalResult = Result<JsValue, Signal>;
