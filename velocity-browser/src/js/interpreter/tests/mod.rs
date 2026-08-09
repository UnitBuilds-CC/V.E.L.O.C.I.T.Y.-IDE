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

pub mod agent;
pub mod agent_layer;
pub mod async_tests;
pub mod basics;
pub mod browser_env;
pub mod builtins;
pub mod canvas;
pub mod dom_bridge;
pub mod es2024;
pub mod functions;
pub mod intl;
pub mod modules;
pub mod objects;
pub mod streams;
pub mod web_platform;
