// Sub-modules
mod token;
mod lexer;
mod ast;
mod parser;
mod signal;
mod eval;
mod constructors;
mod function;
mod native;
mod console;
mod module;
mod property;
mod method_dispatch;
mod collections;
mod core_methods;
mod web_apis;
mod web_apis2;
mod browser_env;
mod dom_bridge;
mod web_platform;
mod streams;
mod canvas;
mod agent_layer;
mod intl;
mod coercion;
mod eval_script;

#[cfg(test)]
mod tests;

// Public API re-exports — preserves external paths like
// `crate::js::interpreter::call_function`, `crate::js::interpreter::Expr`, etc.
pub use token::*;
pub use lexer::lex;
pub use ast::*;
pub use signal::*;
pub use eval::*;
pub use function::{call_function, call_function_with_this, parse_int_js, parse_float_js};
pub use module::*;
pub use property::{get_property, set_property, has_property, delete_property, own_keys_of, own_property_names, enumerable_keys};
pub use method_dispatch::call_method;
pub use coercion::{to_number, to_string, to_boolean, typeof_str};
pub use eval_script::{eval_expr, eval_script};
pub use native::{call_native, json_parse, json_stringify, encode_uri_component, decode_uri_component};
pub use console::*;
