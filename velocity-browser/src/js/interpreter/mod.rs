// Sub-modules
mod agent_layer;
mod ast;
mod browser_env;
mod canvas;
mod coercion;
mod collections;
mod console;
mod constructors;
mod core_methods;
mod dom_bridge;
mod eval;
mod eval_script;
mod function;
mod intl;
mod lexer;
mod method_dispatch;
mod module;
mod native;
mod parser;
mod property;
mod signal;
mod streams;
mod token;
mod web_apis;
mod web_apis2;
mod web_platform;

#[cfg(test)]
mod tests;

// Public API re-exports — preserves external paths like
// `crate::js::interpreter::call_function`, `crate::js::interpreter::Expr`, etc.
pub use agent_layer::export_agent_state_nda;
pub use ast::*;
pub use browser_env::{network_enabled, set_network_enabled};
pub use coercion::{to_boolean, to_number, to_string, typeof_str};
pub use console::*;
pub use eval::*;
pub use eval_script::{eval_expr, eval_script};
pub use function::{call_function, call_function_with_this, parse_float_js, parse_int_js};
pub use lexer::lex;
pub use method_dispatch::call_method;
pub use module::*;
pub use native::{
    call_native, decode_uri_component, encode_uri_component, json_parse, json_stringify,
};
pub use property::{
    delete_property, enumerable_keys, get_property, has_property, own_keys_of, own_property_names,
    set_property,
};
pub use signal::*;
pub use token::*;
