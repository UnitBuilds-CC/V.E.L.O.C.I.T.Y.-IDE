use crate::js::interpreter::*;
use crate::js::scope::Scope;
use crate::js::vm::JsValue;
use std::collections::HashMap;

pub(super) fn eval(s: &str) -> JsValue {
    eval_expr(s, &HashMap::new()).unwrap()
}

pub(super) fn eval_full(s: &str) -> JsValue {
    let scope = Scope::new_global();
    eval_script(s, &scope).unwrap()
}

pub mod basics;
pub mod functions;
pub mod objects;
pub mod builtins;
pub mod intl;
pub mod async_tests;
pub mod modules;
pub mod agent;
pub mod es2024;
pub mod browser_env;
pub mod dom_bridge;
pub mod web_platform;
pub mod streams;
pub mod canvas;
pub mod agent_layer;
