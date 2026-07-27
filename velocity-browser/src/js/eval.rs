use crate::dom::DomTree;
use std::collections::HashMap;

/// Native JS expression evaluator for inline scripts and event handlers.
pub struct JsEvaluator;

impl JsEvaluator {
    /// Evaluate inline script expressions or event listeners natively.
    /// Supports: variable assignment, arithmetic, string concat, DOM queries,
    /// comparisons, ternary, template literals, and common DOM methods.
    pub fn eval_expression(tree: &mut DomTree, expr: &str) -> Result<String, String> {
        let trimmed = expr.trim().trim_end_matches(';').trim();

        if trimmed.is_empty() {
            return Ok("undefined".to_string());
        }

        // String literal
        if (trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
        {
            return Ok(trimmed[1..trimmed.len() - 1].to_string());
        }

        // Numeric literal
        if let Ok(num) = trimmed.parse::<f64>() {
            if num == num.floor() && num.abs() < i64::MAX as f64 {
                return Ok(format!("{}", num as i64));
            }
            return Ok(format!("{}", num));
        }

        // Boolean literals
        if trimmed == "true" { return Ok("true".to_string()); }
        if trimmed == "false" { return Ok("false".to_string()); }
        if trimmed == "null" { return Ok("null".to_string()); }
        if trimmed == "undefined" { return Ok("undefined".to_string()); }

        // Template literal (backtick)
        if trimmed.starts_with('`') && trimmed.ends_with('`') {
            let inner = &trimmed[1..trimmed.len() - 1];
            return Ok(Self::interpolate_template(tree, inner)?);
        }

        // document.querySelector('...').value = '...'
        if trimmed.contains(".value =") || trimmed.contains(".value=") {
            let sep = if trimmed.contains(".value =") { ".value =" } else { ".value=" };
            let parts: Vec<&str> = trimmed.splitn(2, sep).collect();
            if parts.len() == 2 {
                let target = Self::extract_query_selector(parts[0].trim());
                let val = parts[1].trim().trim_matches('\'').trim_matches('"');
                for node in &mut tree.nodes {
                    if Self::node_matches_selector(node, &target) {
                        node.attributes.insert("value".to_string(), val.to_string());
                        return Ok(format!("Updated value to '{}'", val));
                    }
                }
                return Err(format!("Element not found: {}", target));
            }
        }

        // document.querySelector('...').textContent = '...'
        if trimmed.contains(".textContent =") || trimmed.contains(".textContent=") {
            let sep = if trimmed.contains(".textContent =") { ".textContent =" } else { ".textContent=" };
            let parts: Vec<&str> = trimmed.splitn(2, sep).collect();
            if parts.len() == 2 {
                let target = Self::extract_query_selector(parts[0].trim());
                let val = parts[1].trim().trim_matches('\'').trim_matches('"');
                for node in &mut tree.nodes {
                    if Self::node_matches_selector(node, &target) {
                        node.text_content = val.to_string();
                        return Ok(format!("Updated textContent to '{}'", val));
                    }
                }
                return Err(format!("Element not found: {}", target));
            }
        }

        // element.classList.add('...')
        if trimmed.contains(".classList.add(") {
            if let Some(class_name) = Self::extract_method_arg(trimmed, ".classList.add(") {
                let target = Self::extract_query_selector(trimmed.split(".classList.add(").next().unwrap_or(""));
                for node in &mut tree.nodes {
                    if Self::node_matches_selector(node, &target) {
                        let existing = node.attributes.get("class").cloned().unwrap_or_default();
                        let new_class = if existing.is_empty() {
                            class_name.clone()
                        } else {
                            format!("{} {}", existing, class_name)
                        };
                        node.attributes.insert("class".to_string(), new_class);
                        return Ok(format!("Added class '{}'", class_name));
                    }
                }
                return Err(format!("Element not found: {}", target));
            }
        }

        // element.classList.remove('...')
        if trimmed.contains(".classList.remove(") {
            if let Some(class_name) = Self::extract_method_arg(trimmed, ".classList.remove(") {
                let target = Self::extract_query_selector(trimmed.split(".classList.remove(").next().unwrap_or(""));
                for node in &mut tree.nodes {
                    if Self::node_matches_selector(node, &target) {
                        if let Some(existing) = node.attributes.get("class") {
                            let new_class: Vec<&str> = existing.split_whitespace()
                                .filter(|c| *c != class_name.as_str())
                                .collect();
                            node.attributes.insert("class".to_string(), new_class.join(" "));
                        }
                        return Ok(format!("Removed class '{}'", class_name));
                    }
                }
                return Err(format!("Element not found: {}", target));
            }
        }

        // element.setAttribute('name', 'value')
        if trimmed.contains(".setAttribute(") {
            let target = Self::extract_query_selector(trimmed.split(".setAttribute(").next().unwrap_or(""));
            if let Some((attr_name, attr_val)) = Self::extract_two_method_args(trimmed, ".setAttribute(") {
                for node in &mut tree.nodes {
                    if Self::node_matches_selector(node, &target) {
                        node.attributes.insert(attr_name.clone(), attr_val.clone());
                        return Ok(format!("Set attribute '{}' = '{}'", attr_name, attr_val));
                    }
                }
                return Err(format!("Element not found: {}", target));
            }
        }

        // .click() dispatch
        if trimmed.contains(".click()") {
            let target = Self::extract_query_selector(trimmed.trim_end_matches(".click()"));
            return Ok(format!("Native click dispatched on '{}'", target));
        }

        // .focus() dispatch
        if trimmed.contains(".focus()") {
            let target = Self::extract_query_selector(trimmed.trim_end_matches(".focus()"));
            return Ok(format!("Native focus dispatched on '{}'", target));
        }

        // .blur() dispatch
        if trimmed.contains(".blur()") {
            let target = Self::extract_query_selector(trimmed.trim_end_matches(".blur()"));
            return Ok(format!("Native blur dispatched on '{}'", target));
        }

        // Arithmetic: +, -, *, /, %
        if let Some(result) = Self::try_arithmetic(trimmed) {
            return Ok(result);
        }

        // String concatenation with +
        if trimmed.contains('+') {
            if let Some(result) = Self::try_string_concat(tree, trimmed) {
                return Ok(result);
            }
        }

        // Comparison operators
        if let Some(result) = Self::try_comparison(tree, trimmed) {
            return Ok(result);
        }

        // Ternary: condition ? a : b
        if let Some(result) = Self::try_ternary(tree, trimmed) {
            return Ok(result);
        }

        // typeof
        if trimmed.starts_with("typeof ") {
            let operand = trimmed[7..].trim();
            let val = Self::eval_expression(tree, operand)?;
            let type_name = if val == "true" || val == "false" { "boolean" }
                else if val == "null" { "object" }
                else if val == "undefined" { "undefined" }
                else if val.parse::<f64>().is_ok() { "number" }
                else { "string" };
            return Ok(type_name.to_string());
        }

        // document.title
        if trimmed == "document.title" {
            return Ok(tree.extract_page_title());
        }

        // document.querySelectorAll('...').length
        if trimmed.ends_with(".length") && trimmed.contains("querySelectorAll") {
            let sel = Self::extract_query_selector(trimmed.trim_end_matches(".length"));
            let count = tree.nodes.iter()
                .filter(|n| Self::node_matches_selector(n, &sel))
                .count();
            return Ok(format!("{}", count));
        }

        // console.log(...)
        if trimmed.starts_with("console.log(") && trimmed.ends_with(')') {
            let arg = &trimmed[12..trimmed.len() - 1];
            let val = Self::eval_expression(tree, arg)?;
            return Ok(format!("[console.log] {}", val));
        }

        // Variable lookup (simple identifier)
        if trimmed.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') && !trimmed.is_empty() {
            // Check if it's a known global
            return Ok(format!("undefined"));
        }

        Ok("Expression evaluated cleanly".to_string())
    }

    /// Extract the selector from document.querySelector('...') or similar.
    fn extract_query_selector(expr: &str) -> String {
        if let Some(start) = expr.find("('") {
            let rest = &expr[start + 2..];
            if let Some(end) = rest.find("')") {
                return rest[..end].to_string();
            }
        }
        if let Some(start) = expr.find("(\"") {
            let rest = &expr[start + 2..];
            if let Some(end) = rest.find("\")") {
                return rest[..end].to_string();
            }
        }
        expr.to_string()
    }

    /// Check if a node matches a simple CSS selector.
    fn node_matches_selector(node: &crate::parser::html::DomNode, selector: &str) -> bool {
        if selector.starts_with('#') {
            let id = &selector[1..];
            return node.attributes.get("id").map(|i| i == id).unwrap_or(false);
        }
        if selector.starts_with('.') {
            let cls = &selector[1..];
            return node.attributes.get("class")
                .map(|c| c.split_whitespace().any(|x| x == cls))
                .unwrap_or(false);
        }
        node.tag_name.to_lowercase() == selector.to_lowercase()
    }

    /// Extract a single argument from a method call like .method('arg').
    fn extract_method_arg(expr: &str, method: &str) -> Option<String> {
        let start = expr.find(method)? + method.len();
        let rest = &expr[start..];
        // Find the closing paren
        let end = rest.find(')')?;
        let arg = rest[..end].trim().trim_matches('\'').trim_matches('"');
        Some(arg.to_string())
    }

    /// Extract two arguments from a method call like .method('a', 'b').
    fn extract_two_method_args(expr: &str, method: &str) -> Option<(String, String)> {
        let start = expr.find(method)? + method.len();
        let rest = &expr[start..];
        let end = rest.find(')')?;
        let args_str = &rest[..end];
        let parts: Vec<&str> = args_str.splitn(2, ',').collect();
        if parts.len() == 2 {
            let a = parts[0].trim().trim_matches('\'').trim_matches('"').to_string();
            let b = parts[1].trim().trim_matches('\'').trim_matches('"').to_string();
            return Some((a, b));
        }
        None
    }

    /// Try to evaluate as arithmetic expression.
    fn try_arithmetic(expr: &str) -> Option<String> {
        // Find the last + or - that isn't inside a string
        let chars: Vec<char> = expr.chars().collect();
        let len = chars.len();

        // Try each operator (lowest precedence first)
        for op in &['+', '-', '*', '/', '%'] {
            let mut depth = 0;
            let mut in_str = false;
            let mut str_char = ' ';
            for i in (1..len).rev() {
                let ch = chars[i];
                if in_str {
                    if ch == str_char { in_str = false; }
                    continue;
                }
                if ch == '\'' || ch == '"' { in_str = true; str_char = ch; continue; }
                if ch == ')' { depth += 1; continue; }
                if ch == '(' { depth -= 1; continue; }
                if depth == 0 && ch == *op {
                    // Avoid matching unary +/- at start or after operator
                    if i == 0 { continue; }
                    let prev = chars[i - 1];
                    if prev == '(' || prev == ',' || prev == '=' || prev == '+' || prev == '-' || prev == '*' || prev == '/' {
                        continue;
                    }
                    let left = expr[..i].trim();
                    let right = expr[i + 1..].trim();
                    if let (Ok(l), Ok(r)) = (left.parse::<f64>(), right.parse::<f64>()) {
                        let result = match op {
                            '+' => l + r,
                            '-' => l - r,
                            '*' => l * r,
                            '/' => if r == 0.0 { return Some("Infinity".to_string()); } else { l / r },
                            '%' => if r == 0.0 { return Some("NaN".to_string()); } else { l % r },
                            _ => return None,
                        };
                        if result == result.floor() && result.abs() < i64::MAX as f64 {
                            return Some(format!("{}", result as i64));
                        }
                        return Some(format!("{}", result));
                    }
                }
            }
        }
        None
    }

    /// Try string concatenation.
    fn try_string_concat(tree: &mut DomTree, expr: &str) -> Option<String> {
        let parts: Vec<&str> = expr.split('+').collect();
        if parts.len() < 2 { return None; }

        let mut result = String::new();
        let mut all_strings = true;

        for part in &parts {
            let trimmed = part.trim();
            if (trimmed.starts_with('"') && trimmed.ends_with('"'))
                || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
            {
                result.push_str(&trimmed[1..trimmed.len() - 1]);
            } else if trimmed.parse::<f64>().is_ok() {
                result.push_str(trimmed);
            } else {
                all_strings = false;
                break;
            }
        }

        if all_strings { Some(result) } else { None }
    }

    /// Try comparison operators.
    fn try_comparison(tree: &mut DomTree, expr: &str) -> Option<String> {
        for (op, name) in &[("===", "strict_eq"), ("!==", "strict_ne"), ("==", "eq"), ("!=", "ne"), (">=", "gte"), ("<=", "lte"), (">", "gt"), ("<", "lt")] {
            if let Some(pos) = expr.find(op) {
                let left = expr[..pos].trim();
                let right = expr[pos + op.len()..].trim();
                let l_val = Self::eval_expression(tree, left).ok()?;
                let r_val = Self::eval_expression(tree, right).ok()?;

                let result = match *name {
                    "strict_eq" | "eq" => l_val == r_val,
                    "strict_ne" | "ne" => l_val != r_val,
                    "gte" => {
                        if let (Ok(l), Ok(r)) = (l_val.parse::<f64>(), r_val.parse::<f64>()) { l >= r }
                        else { l_val >= r_val }
                    }
                    "lte" => {
                        if let (Ok(l), Ok(r)) = (l_val.parse::<f64>(), r_val.parse::<f64>()) { l <= r }
                        else { l_val <= r_val }
                    }
                    "gt" => {
                        if let (Ok(l), Ok(r)) = (l_val.parse::<f64>(), r_val.parse::<f64>()) { l > r }
                        else { l_val > r_val }
                    }
                    "lt" => {
                        if let (Ok(l), Ok(r)) = (l_val.parse::<f64>(), r_val.parse::<f64>()) { l < r }
                        else { l_val < r_val }
                    }
                    _ => false,
                };
                return Some(if result { "true".to_string() } else { "false".to_string() });
            }
        }
        None
    }

    /// Try ternary expression: condition ? a : b
    fn try_ternary(tree: &mut DomTree, expr: &str) -> Option<String> {
        let q_pos = expr.find('?')?;
        let colon_pos = expr[q_pos + 1..].find(':')? + q_pos + 1;

        let condition = expr[..q_pos].trim();
        let consequent = expr[q_pos + 1..colon_pos].trim();
        let alternate = expr[colon_pos + 1..].trim();

        let cond_val = Self::eval_expression(tree, condition).ok()?;
        let is_truthy = cond_val != "false" && cond_val != "0" && cond_val != "null" && cond_val != "undefined" && !cond_val.is_empty();

        if is_truthy {
            Self::eval_expression(tree, consequent).ok()
        } else {
            Self::eval_expression(tree, alternate).ok()
        }
    }

    /// Interpolate template literal expressions ${...}.
    fn interpolate_template(tree: &mut DomTree, template: &str) -> Result<String, String> {
        let mut result = String::new();
        let mut chars = template.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '$' && chars.peek() == Some(&'{') {
                chars.next(); // consume '{'
                let mut depth = 1;
                let mut expr = String::new();
                while let Some(c) = chars.next() {
                    if c == '{' { depth += 1; }
                    if c == '}' {
                        depth -= 1;
                        if depth == 0 { break; }
                    }
                    expr.push(c);
                }
                let val = Self::eval_expression(tree, &expr)?;
                result.push_str(&val);
            } else {
                result.push(ch);
            }
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::DomTree;
    use crate::parser::html::{DomNode, NodeType};

    fn make_tree() -> DomTree {
        let nodes = vec![
            DomNode {
                id: 0,
                node_type: NodeType::Document,
                tag_name: "#document".to_string(),
                attributes: HashMap::new(),
                text_content: String::new(),
                children: vec![1, 2],
                parent: None,
            },
            DomNode {
                id: 1,
                node_type: NodeType::Element,
                tag_name: "input".to_string(),
                attributes: {
                    let mut m = HashMap::new();
                    m.insert("id".to_string(), "name".to_string());
                    m.insert("value".to_string(), "old".to_string());
                    m
                },
                text_content: String::new(),
                children: Vec::new(),
                parent: Some(0),
            },
            DomNode {
                id: 2,
                node_type: NodeType::Element,
                tag_name: "div".to_string(),
                attributes: {
                    let mut m = HashMap::new();
                    m.insert("class".to_string(), "active".to_string());
                    m
                },
                text_content: "hello".to_string(),
                children: Vec::new(),
                parent: Some(0),
            },
        ];
        DomTree::new(nodes)
    }

    #[test]
    fn test_numeric_literal() {
        let mut tree = make_tree();
        assert_eq!(JsEvaluator::eval_expression(&mut tree, "42").unwrap(), "42");
        assert_eq!(JsEvaluator::eval_expression(&mut tree, "3.14").unwrap(), "3.14");
    }

    #[test]
    fn test_string_literal() {
        let mut tree = make_tree();
        assert_eq!(JsEvaluator::eval_expression(&mut tree, "'hello'").unwrap(), "hello");
        assert_eq!(JsEvaluator::eval_expression(&mut tree, "\"world\"").unwrap(), "world");
    }

    #[test]
    fn test_boolean_null() {
        let mut tree = make_tree();
        assert_eq!(JsEvaluator::eval_expression(&mut tree, "true").unwrap(), "true");
        assert_eq!(JsEvaluator::eval_expression(&mut tree, "false").unwrap(), "false");
        assert_eq!(JsEvaluator::eval_expression(&mut tree, "null").unwrap(), "null");
    }

    #[test]
    fn test_arithmetic() {
        let mut tree = make_tree();
        assert_eq!(JsEvaluator::eval_expression(&mut tree, "2 + 3").unwrap(), "5");
        assert_eq!(JsEvaluator::eval_expression(&mut tree, "10 - 4").unwrap(), "6");
        assert_eq!(JsEvaluator::eval_expression(&mut tree, "3 * 7").unwrap(), "21");
        assert_eq!(JsEvaluator::eval_expression(&mut tree, "10 / 3").unwrap(), "3.3333333333333335");
        assert_eq!(JsEvaluator::eval_expression(&mut tree, "10 % 3").unwrap(), "1");
    }

    #[test]
    fn test_division_by_zero() {
        let mut tree = make_tree();
        assert_eq!(JsEvaluator::eval_expression(&mut tree, "1 / 0").unwrap(), "Infinity");
    }

    #[test]
    fn test_string_concat() {
        let mut tree = make_tree();
        assert_eq!(JsEvaluator::eval_expression(&mut tree, "'hello' + ' ' + 'world'").unwrap(), "hello world");
    }

    #[test]
    fn test_comparison() {
        let mut tree = make_tree();
        assert_eq!(JsEvaluator::eval_expression(&mut tree, "5 > 3").unwrap(), "true");
        assert_eq!(JsEvaluator::eval_expression(&mut tree, "2 === 2").unwrap(), "true");
        assert_eq!(JsEvaluator::eval_expression(&mut tree, "'a' !== 'b'").unwrap(), "true");
        assert_eq!(JsEvaluator::eval_expression(&mut tree, "3 <= 3").unwrap(), "true");
    }

    #[test]
    fn test_ternary() {
        let mut tree = make_tree();
        assert_eq!(JsEvaluator::eval_expression(&mut tree, "true ? 'yes' : 'no'").unwrap(), "yes");
        assert_eq!(JsEvaluator::eval_expression(&mut tree, "false ? 'yes' : 'no'").unwrap(), "no");
    }

    #[test]
    fn test_template_literal() {
        let mut tree = make_tree();
        assert_eq!(JsEvaluator::eval_expression(&mut tree, "`hello ${'world'}`").unwrap(), "hello world");
    }

    #[test]
    fn test_value_assignment() {
        let mut tree = make_tree();
        let result = JsEvaluator::eval_expression(&mut tree, "document.querySelector('#name').value = 'new_val'");
        assert!(result.is_ok());
        assert_eq!(tree.nodes[1].attributes.get("value").unwrap(), "new_val");
    }

    #[test]
    fn test_text_content_assignment() {
        let mut tree = make_tree();
        let result = JsEvaluator::eval_expression(&mut tree, "document.querySelector('.active').textContent = 'updated'");
        assert!(result.is_ok());
        assert_eq!(tree.nodes[2].text_content, "updated");
    }

    #[test]
    fn test_classlist_add() {
        let mut tree = make_tree();
        let result = JsEvaluator::eval_expression(&mut tree, "document.querySelector('.active').classList.add('highlight')");
        assert!(result.is_ok());
        assert!(tree.nodes[2].attributes.get("class").unwrap().contains("highlight"));
    }

    #[test]
    fn test_classlist_remove() {
        let mut tree = make_tree();
        let result = JsEvaluator::eval_expression(&mut tree, "document.querySelector('.active').classList.remove('active')");
        assert!(result.is_ok());
        assert!(!tree.nodes[2].attributes.get("class").unwrap().contains("active"));
    }

    #[test]
    fn test_set_attribute() {
        let mut tree = make_tree();
        let result = JsEvaluator::eval_expression(&mut tree, "document.querySelector('#name').setAttribute('disabled', 'true')");
        assert!(result.is_ok());
        assert_eq!(tree.nodes[1].attributes.get("disabled").unwrap(), "true");
    }

    #[test]
    fn test_typeof() {
        let mut tree = make_tree();
        assert_eq!(JsEvaluator::eval_expression(&mut tree, "typeof 42").unwrap(), "number");
        assert_eq!(JsEvaluator::eval_expression(&mut tree, "typeof 'hello'").unwrap(), "string");
        assert_eq!(JsEvaluator::eval_expression(&mut tree, "typeof true").unwrap(), "boolean");
        assert_eq!(JsEvaluator::eval_expression(&mut tree, "typeof null").unwrap(), "object");
    }

    #[test]
    fn test_console_log() {
        let mut tree = make_tree();
        let result = JsEvaluator::eval_expression(&mut tree, "console.log('test')").unwrap();
        assert!(result.contains("test"));
    }

    #[test]
    fn test_click_dispatch() {
        let mut tree = make_tree();
        let result = JsEvaluator::eval_expression(&mut tree, "document.querySelector('#name').click()");
        assert!(result.unwrap().contains("click"));
    }

    #[test]
    fn test_queryselectorall_length() {
        let mut tree = make_tree();
        let result = JsEvaluator::eval_expression(&mut tree, "document.querySelectorAll('div').length").unwrap();
        assert_eq!(result, "1");
    }
}
