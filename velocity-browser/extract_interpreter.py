#!/usr/bin/env python3
"""Extract interpreter.rs into a directory module at interpreter/."""
import os
import re

BASE = r"c:\Users\visse\OneDrive\Documentos\Kimi Code\velocity-workspace\velocity-browser\src\js"
SRC = os.path.join(BASE, "interpreter.rs")
DST = os.path.join(BASE, "interpreter")

os.makedirs(os.path.join(DST, "tests"), exist_ok=True)

with open(SRC, "r", encoding="utf-8") as f:
    lines = f.readlines()

total = len(lines)
print(f"Source file: {total} lines")

def get(a, b):
    return "".join(lines[a-1:b])

COMMON = """use crate::js::scope::{Scope, ScopeRef};
use crate::js::vm::JsValue;
use std::collections::HashMap;
"""

COMMON_REGEX = """use crate::js::scope::{Scope, ScopeRef};
use crate::js::vm::JsValue;
use std::collections::HashMap;
use regex::Regex;
"""

def adjust_vis(text):
    """Make all non-pub fn/static/const/type items pub for cross-module access."""
    out = []
    pat = re.compile(r'^((?:    )*)(pub(?:\([^)]*\))?\s+)?(const\s+|fn\s+|static\s+|type\s+)')
    for line in text.split('\n'):
        stripped = line.lstrip()
        if stripped.startswith('//') or stripped.startswith('///') or stripped.startswith('#') or stripped == '':
            out.append(line)
            continue
        m = pat.match(line)
        if m:
            indent = m.group(1)
            existing_vis = m.group(2)
            keyword = m.group(3)
            if existing_vis is None:
                line = f"{indent}pub {keyword}{line[m.end():]}"
        out.append(line)
    return '\n'.join(out)

def write_file(name, content):
    path = os.path.join(DST, name)
    with open(path, "w", encoding="utf-8", newline='\n') as f:
        f.write(content)
    lc = content.count('\n')
    print(f"  {name}: {lc} lines")

print("Creating files...")

# lexer.rs
write_file("lexer.rs",
    "//! Lexer: converts source text into a token stream.\n\n"
    "use super::token::*;\n\n"
    + adjust_vis(get(49, 175)))

# ast.rs
write_file("ast.rs",
    "//! AST node definitions: statements, expressions, and supporting types.\n\n"
    "use super::token::*;\n\n"
    + get(182, 313))

# parser.rs
write_file("parser.rs",
    "//! Parser: converts a token stream into an AST.\n\n"
    "use super::token::*;\n"
    "use super::ast::*;\n\n"
    + adjust_vis(get(319, 1233)))

# signal.rs
write_file("signal.rs",
    "//! Control flow signals for the evaluator.\n\n"
    "use crate::js::vm::JsValue;\n\n"
    + get(1240, 1248))

# eval.rs
write_file("eval.rs",
    "//! Tree-walking evaluator: evaluates AST nodes.\n\n"
    + COMMON
    + "use super::token::*;\n"
    + "use super::ast::*;\n"
    + "use super::signal::*;\n\n"
    + adjust_vis(get(1251, 3062)))

# function.rs
func_content = get(3064, 3274) + "\n" + get(8398, 8418)
write_file("function.rs",
    "//! Function call machinery: parameter binding, invocation, number parsing.\n\n"
    + COMMON
    + "use super::token::*;\n"
    + "use super::ast::*;\n"
    + "use super::signal::*;\n\n"
    + adjust_vis(func_content))

# native.rs
write_file("native.rs",
    "//! Native built-in function dispatch and serialization helpers.\n\n"
    + COMMON_REGEX
    + "use super::token::*;\n"
    + "use super::ast::*;\n"
    + "use super::signal::*;\n"
    + "use serde_json;\n\n"
    + adjust_vis(get(3276, 4518)))

# module.rs - includes use std::sync::Mutex from line 4524
# Skip the standalone "use std::sync::Mutex;" line since it's in COMMON already? No,
# Mutex is NOT in COMMON. We need it here specifically.
module_raw = get(4524, 4653)
# Remove the standalone "use std::sync::Mutex;" line since we'll add it in the header
module_raw = module_raw.replace("use std::sync::Mutex;\n", "")
write_file("module.rs",
    "//! ES module registry: import/export resolution and evaluation.\n\n"
    + COMMON
    + "use super::token::*;\n"
    + "use super::ast::*;\n"
    + "use super::signal::*;\n"
    + "use std::sync::Mutex;\n\n"
    + adjust_vis(module_raw))

# console.rs - JS_CALL_STACK is in property.rs; reference via super::property::JS_CALL_STACK
console_raw = get(4678, 4752)
console_raw = console_raw.replace(
    "JS_CALL_STACK.with(|stack|",
    "super::property::JS_CALL_STACK.with(|stack|")
write_file("console.rs",
    "//! Console output capture and performance timing API.\n\n"
    + COMMON
    + "use super::token::*;\n"
    + "use super::ast::*;\n"
    + "use super::signal::*;\n\n"
    + adjust_vis(console_raw))

# property.rs - includes section header + thread locals (4655-4676) + iterate_values onwards (4753-5392)
prop_content = get(4655, 4676) + "\n" + get(4753, 5392)
write_file("property.rs",
    "//! Property access, iteration, and descriptor management.\n\n"
    + COMMON
    + "use super::token::*;\n"
    + "use super::ast::*;\n"
    + "use super::signal::*;\n\n"
    + adjust_vis(prop_content))

# method_dispatch.rs
write_file("method_dispatch.rs",
    "//! Method dispatch: JsValue-based routing and utility functions.\n\n"
    + COMMON_REGEX
    + "use super::token::*;\n"
    + "use super::ast::*;\n"
    + "use super::signal::*;\n\n"
    + adjust_vis(get(5394, 5772)))

# builtin_methods.rs
write_file("builtin_methods.rs",
    "//! Built-in method implementations for all JS standard types.\n\n"
    + COMMON_REGEX
    + "use super::token::*;\n"
    + "use super::ast::*;\n"
    + "use super::signal::*;\n\n"
    + adjust_vis(get(5777, 8305)))

# coercion.rs
write_file("coercion.rs",
    "//! Type coercion and comparison helpers.\n\n"
    + COMMON
    + "use super::token::*;\n"
    + "use super::ast::*;\n"
    + "use super::signal::*;\n\n"
    + adjust_vis(get(8313, 8548)))

# eval_script.rs
write_file("eval_script.rs",
    "//! Public eval entry points: eval_expr, eval_script, eval_script_standalone.\n\n"
    + COMMON
    + "use super::token::*;\n"
    + "use super::ast::*;\n"
    + "use super::signal::*;\n\n"
    + adjust_vis(get(8556, 8588)))

# tests/mod.rs
test_content = get(8594, 12358)
# Remove the trailing closing brace of the original `mod tests { ... }`
if test_content.rstrip().endswith('}'):
    # Find the last } and remove it
    idx = test_content.rstrip().rfind('}')
    test_content = test_content.rstrip()[:idx] + '\n'
write_file("tests/mod.rs",
    "//! Test suite for the JS interpreter.\n\n"
    "#[allow(unused_imports)]\n"
    "use crate::js::interpreter::*;\n"
    "#[allow(unused_imports)]\n"
    "use crate::js::scope::Scope;\n"
    "#[allow(unused_imports)]\n"
    "use std::collections::HashMap;\n\n"
    "fn eval(s: &str) -> JsValue {\n"
    "    eval_expr(s, &HashMap::new()).unwrap()\n"
    "}\n\n"
    "fn eval_full(s: &str) -> JsValue {\n"
    "    let scope = Scope::new_global();\n"
    "    eval_script(s, &scope).unwrap()\n"
    "}\n\n"
    + test_content)

# mod.rs
mod_rs = '''//! Full JavaScript interpreter: lexer -> parser -> tree-walking evaluator.
//!
//! Supports: variable declarations, assignments, if/else, while, for,
//! for-in/of, functions (declarations + arrows), closures, objects, arrays,
//! property access (dot + bracket), method calls, try/catch/finally,
//! throw, return, break, continue, ternary, typeof, template literals,
//! spread, and all standard operators. This is the agent-first JS surface:
//! enough to execute the scripts real pages ship, not a spec-complete engine.

// Sub-modules (split from the original monolithic interpreter.rs)
pub mod token;
pub mod lexer;
pub mod ast;
pub mod parser;
pub mod signal;
pub mod eval;
pub mod function;
pub mod native;
pub mod module;
pub mod console;
pub mod property;
pub mod method_dispatch;
pub mod builtin_methods;
pub mod coercion;
pub mod eval_script;

#[cfg(test)]
pub mod tests;

// Re-export the public API
pub use token::*;
pub use ast::*;
pub use signal::*;
pub use eval::*;
pub use lexer::lex;
pub use parser::Parser;
pub use function::call_function;
pub use function::call_function_with_this;
pub use native::call_native;
pub use native::json_parse;
pub use native::json_stringify;
pub use native::json_stringify_pretty;
pub use native::encode_uri_component;
pub use native::decode_uri_component;
pub use native::serde_to_js;
pub use module::*;
pub use console::ConsoleRecord;
pub use console::get_console_output;
pub use console::clear_console_output;
pub use console::PerformanceEntry;
pub use console::get_performance_entries;
pub use console::clear_performance_entries;
pub use property::*;
pub use method_dispatch::call_method;
pub use method_dispatch::is_known_native;
pub use builtin_methods::*;
pub use coercion::*;
pub use eval_script::{eval_expr, eval_script};
'''
write_file("mod.rs", mod_rs)

print(f"\nDone! Created files from {total} lines of source.")
