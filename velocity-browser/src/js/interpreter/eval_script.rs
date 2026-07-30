use super::signal::*;
use super::lexer::lex;
use super::parser::Parser;
use super::coercion::*;
use super::eval::{eval_program, eval_expr_node};
use crate::js::scope::{Scope, ScopeRef};
use crate::js::vm::JsValue;
use std::collections::HashMap;

/// Evaluate a single JS expression against a flat variable scope.
/// This is the backward-compatible interface used by vm.rs.
pub fn eval_expr(input: &str, scope_map: &HashMap<String, JsValue>) -> Result<JsValue, String> {
    let tokens = lex(input)?;
    if tokens.len() <= 1 { return Ok(JsValue::Undefined); } // only Eof
    let mut parser = Parser::new(tokens);
    let expr = parser.parse_expr().map_err(|e| e.to_string())?;
    let scope = Scope::new_global();
    { let mut s = scope.lock().unwrap(); s.locals = scope_map.clone(); }
    match eval_expr_node(&expr, &scope) {
        Ok(v) => Ok(v),
        Err(Signal::Throw(v)) => Err(to_string(&v)),
        Err(_) => Ok(JsValue::Undefined),
    }
}

/// Parse and evaluate a full script (multiple statements). Used by the new VM.
pub fn eval_script(input: &str, scope: &ScopeRef) -> Result<JsValue, String> {
    let tokens = lex(input)?;
    if tokens.len() <= 1 { return Ok(JsValue::Undefined); }
    let mut parser = Parser::new(tokens);
    let stmts = parser.parse_program().map_err(|e| e.to_string())?;
    match eval_program(&stmts, scope) {
        Ok(v) => Ok(v),
        Err(Signal::Return(v)) => Ok(v),
        Err(Signal::Throw(v)) => Err(to_string(&v)),
        Err(_) => Ok(JsValue::Undefined),
    }
}

/// eval() equivalent - creates a fresh scope for the code.
pub(super) fn eval_script_standalone(input: &str) -> Result<JsValue, String> {
    let scope = Scope::new_global();
    eval_script(input, &scope)
}
