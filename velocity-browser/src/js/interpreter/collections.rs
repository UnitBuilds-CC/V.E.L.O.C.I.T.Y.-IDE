use super::signal::*;
use super::coercion::*;
use super::function::call_function;
use crate::js::scope::ScopeRef;
use crate::js::vm::JsValue;

pub(super) fn call_array_method(a: &mut Vec<JsValue>, method: &str, args: &[JsValue], scope: &ScopeRef) -> EvalResult {
    Ok(match method {
        "push" => { a.extend(args.iter().cloned()); JsValue::Number(a.len() as f64) }
        "pop" => a.pop().unwrap_or(JsValue::Undefined),
        "shift" => { if a.is_empty() { JsValue::Undefined } else { a.remove(0) } }
        "unshift" => {
            let tail = std::mem::take(a);
            let mut new = args.to_vec();
            new.extend(tail);
            *a = new;
            JsValue::Number(a.len() as f64)
        }
        "length" => JsValue::Number(a.len() as f64),
        "indexOf" => {
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            let len = a.len() as i64;
            let mut from = args.get(1).map(|v| to_number(v) as i64).unwrap_or(0);
            if from < 0 { from += len; }
            let start = from.max(0) as usize;
            let found = a.iter().enumerate().skip(start)
                .find(|(_, x)| strict_eq(x, &target))
                .map(|(i, _)| i as f64).unwrap_or(-1.0);
            JsValue::Number(found)
        }
        "lastIndexOf" => {
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            let len = a.len() as i64;
            let mut from = args.get(1).map(|v| to_number(v) as i64).unwrap_or(len - 1);
            if from < 0 { from += len; }
            let end = from.min(len - 1);
            let mut result = -1.0;
            if end >= 0 {
                for i in (0..=end as usize).rev() {
                    if strict_eq(&a[i], &target) { result = i as f64; break; }
                }
            }
            JsValue::Number(result)
        }
        "includes" => {
            let target = args.first().cloned().unwrap_or(JsValue::Undefined);
            let len = a.len() as i64;
            let mut from = args.get(1).map(|v| to_number(v) as i64).unwrap_or(0);
            if from < 0 { from += len; }
            let start = from.max(0) as usize;
            let found = a.iter().skip(start).any(|x| match (x, &target) {
                (JsValue::Number(p), JsValue::Number(q)) if p.is_nan() && q.is_nan() => true,
                _ => strict_eq(x, &target),
            });
            JsValue::Boolean(found)
        }
        "at" => {
            let i = args.first().map(to_number).unwrap_or(0.0) as i64;
            let len = a.len() as i64;
            let idx = if i < 0 { len + i } else { i };
            if (0..len).contains(&idx) { a[idx as usize].clone() } else { JsValue::Undefined }
        }
        "join" => {
            let sep = match args.first() {
                None | Some(JsValue::Undefined) => ",".to_string(),
                Some(v) => to_string(v),
            };
            let parts: Vec<String> = a.iter().map(|x| match x {
                JsValue::Null | JsValue::Undefined => String::new(),
                other => to_string(other),
            }).collect();
            JsValue::String(parts.join(&sep))
        }
        "toString" | "toLocaleString" => {
            let parts: Vec<String> = a.iter().map(|x| match x {
                JsValue::Null | JsValue::Undefined => String::new(),
                other => to_string(other),
            }).collect();
            JsValue::String(parts.join(","))
        }
        "slice" => {
            let start = args.first().map(|v| to_number(v) as i64).unwrap_or(0);
            let end = args.get(1).map(|v| to_number(v) as i64).unwrap_or(a.len() as i64);
            let s = if start < 0 { (a.len() as i64 + start).max(0) as usize } else { start as usize };
            let e = if end < 0 { (a.len() as i64 + end).max(0) as usize } else { (end as usize).min(a.len()) };
            JsValue::Array(a.get(s..e).unwrap_or(&[]).to_vec())
        }
        "concat" => {
            let mut new_arr = a.clone();
            for x in args { if let JsValue::Array(other) = x { new_arr.extend(other.iter().cloned()); } else { new_arr.push(x.clone()); } }
            JsValue::Array(new_arr)
        }
        "reverse" => { a.reverse(); JsValue::Array(a.clone()) }
        "sort" => {
            match args.first() {
                Some(cb) if !matches!(cb, JsValue::Undefined | JsValue::Null) => {
                    let mut sort_err: Option<Signal> = None;
                    a.sort_by(|x, y| {
                        if sort_err.is_some() { return std::cmp::Ordering::Equal; }
                        match call_function(cb, &[x.clone(), y.clone()], scope) {
                            Ok(v) => {
                                let n = to_number(&v);
                                if n < 0.0 { std::cmp::Ordering::Less }
                                else if n > 0.0 { std::cmp::Ordering::Greater }
                                else { std::cmp::Ordering::Equal }
                            }
                            Err(e) => { sort_err = Some(e); std::cmp::Ordering::Equal }
                        }
                    });
                    if let Some(e) = sort_err { return Err(e); }
                }
                _ => { a.sort_by_key(to_string); }
            }
            JsValue::Array(a.clone())
        }
        "splice" => {
            let len = a.len() as i64;
            let start_raw = args.first().map(to_number).unwrap_or(0.0) as i64;
            let start = if start_raw < 0 { (len + start_raw).max(0) as usize } else { (start_raw as usize).min(a.len()) };
            let delete_count = args.get(1).map(|v| to_number(v) as i64).unwrap_or(len).max(0) as usize;
            let end = (start + delete_count).min(a.len());
            let removed: Vec<JsValue> = a.drain(start..end).collect();
            for (i, item) in args.iter().skip(2).enumerate() { a.insert(start + i, item.clone()); }
            JsValue::Array(removed)
        }
        "map" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            let arr_val = JsValue::Array(a.clone());
            let mut result = Vec::new();
            for (i, item) in a.iter().enumerate() {
                let r = call_function(&callback, &[item.clone(), JsValue::Number(i as f64), arr_val.clone()], scope)?;
                result.push(r);
            }
            JsValue::Array(result)
        }
        "filter" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            let arr_val = JsValue::Array(a.clone());
            let mut result = Vec::new();
            for (i, item) in a.iter().enumerate() {
                let r = call_function(&callback, &[item.clone(), JsValue::Number(i as f64), arr_val.clone()], scope)?;
                if to_boolean(&r) { result.push(item.clone()); }
            }
            JsValue::Array(result)
        }
        "forEach" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            let arr_val = JsValue::Array(a.clone());
            for (i, item) in a.iter().enumerate() {
                call_function(&callback, &[item.clone(), JsValue::Number(i as f64), arr_val.clone()], scope)?;
            }
            JsValue::Undefined
        }
        "find" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            let arr_val = JsValue::Array(a.clone());
            for (i, item) in a.iter().enumerate() {
                let r = call_function(&callback, &[item.clone(), JsValue::Number(i as f64), arr_val.clone()], scope)?;
                if to_boolean(&r) { return Ok(item.clone()); }
            }
            JsValue::Undefined
        }
        "findIndex" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            let arr_val = JsValue::Array(a.clone());
            for (i, item) in a.iter().enumerate() {
                let r = call_function(&callback, &[item.clone(), JsValue::Number(i as f64), arr_val.clone()], scope)?;
                if to_boolean(&r) { return Ok(JsValue::Number(i as f64)); }
            }
            JsValue::Number(-1.0)
        }
        "findLast" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            let arr_val = JsValue::Array(a.clone());
            for (i, item) in a.iter().enumerate().rev() {
                let r = call_function(&callback, &[item.clone(), JsValue::Number(i as f64), arr_val.clone()], scope)?;
                if to_boolean(&r) { return Ok(item.clone()); }
            }
            JsValue::Undefined
        }
        "findLastIndex" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            let arr_val = JsValue::Array(a.clone());
            for (i, item) in a.iter().enumerate().rev() {
                let r = call_function(&callback, &[item.clone(), JsValue::Number(i as f64), arr_val.clone()], scope)?;
                if to_boolean(&r) { return Ok(JsValue::Number(i as f64)); }
            }
            JsValue::Number(-1.0)
        }
        "reduce" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            let arr_val = JsValue::Array(a.clone());
            let mut acc = args.get(1).cloned().unwrap_or_else(|| a.first().cloned().unwrap_or(JsValue::Undefined));
            let start = if args.len() > 1 { 0 } else { 1 };
            for (i, item) in a.iter().enumerate().skip(start) {
                acc = call_function(&callback, &[acc, item.clone(), JsValue::Number(i as f64), arr_val.clone()], scope)?;
            }
            acc
        }
        "reduceRight" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            let arr_val = JsValue::Array(a.clone());
            let has_initial = args.len() > 1;
            let mut acc = if has_initial { args[1].clone() } else { a.last().cloned().unwrap_or(JsValue::Undefined) };
            let upper = if has_initial { a.len() } else { a.len().saturating_sub(1) };
            for i in (0..upper).rev() {
                let item = a[i].clone();
                acc = call_function(&callback, &[acc, item, JsValue::Number(i as f64), arr_val.clone()], scope)?;
            }
            acc
        }
        "some" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            let arr_val = JsValue::Array(a.clone());
            for (i, item) in a.iter().enumerate() { if to_boolean(&call_function(&callback, &[item.clone(), JsValue::Number(i as f64), arr_val.clone()], scope)?) { return Ok(JsValue::Boolean(true)); } }
            JsValue::Boolean(false)
        }
        "every" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            let arr_val = JsValue::Array(a.clone());
            for (i, item) in a.iter().enumerate() { if !to_boolean(&call_function(&callback, &[item.clone(), JsValue::Number(i as f64), arr_val.clone()], scope)?) { return Ok(JsValue::Boolean(false)); } }
            JsValue::Boolean(true)
        }
        "flat" => {
            let depth = match args.first() {
                Some(v) if !matches!(v, JsValue::Undefined) => {
                    let n = to_number(v);
                    if n.is_finite() { n.max(0.0) as usize } else { usize::MAX }
                }
                _ => 1,
            };
            JsValue::Array(super::method_dispatch::flatten_array(a, depth))
        }
        "flatMap" => {
            let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
            let mut result = Vec::new();
            for (i, item) in a.iter().enumerate() {
                let r = call_function(&callback, &[item.clone(), JsValue::Number(i as f64)], scope)?;
                if let JsValue::Array(inner) = r { result.extend(inner); } else { result.push(r); }
            }
            JsValue::Array(result)
        }
        "fill" => {
            let val = args.first().cloned().unwrap_or(JsValue::Undefined);
            let len = a.len() as i64;
            let start_raw = args.get(1).map(to_number).unwrap_or(0.0) as i64;
            let end_raw = args.get(2).map(to_number).unwrap_or(len as f64) as i64;
            let start = if start_raw < 0 { (len + start_raw).max(0) as usize } else { (start_raw as usize).min(a.len()) };
            let end = if end_raw < 0 { (len + end_raw).max(0) as usize } else { (end_raw as usize).min(a.len()) };
            for item in a.iter_mut().take(end).skip(start) { *item = val.clone(); }
            JsValue::Array(a.clone())
        }
        "toReversed" => {
            let mut out = a.clone();
            out.reverse();
            JsValue::Array(out)
        }
        "toSorted" => {
            let mut out = a.clone();
            match args.first() {
                Some(cb) if !matches!(cb, JsValue::Undefined | JsValue::Null) => {
                    let mut sort_err: Option<Signal> = None;
                    out.sort_by(|x, y| {
                        if sort_err.is_some() { return std::cmp::Ordering::Equal; }
                        match call_function(cb, &[x.clone(), y.clone()], scope) {
                            Ok(v) => {
                                let n = to_number(&v);
                                if n < 0.0 { std::cmp::Ordering::Less }
                                else if n > 0.0 { std::cmp::Ordering::Greater }
                                else { std::cmp::Ordering::Equal }
                            }
                            Err(e) => { sort_err = Some(e); std::cmp::Ordering::Equal }
                        }
                    });
                    if let Some(e) = sort_err { return Err(e); }
                }
                _ => { out.sort_by_key(to_string); }
            }
            JsValue::Array(out)
        }
        "with" => {
            let len = a.len() as i64;
            let raw = args.first().map(to_number).unwrap_or(0.0) as i64;
            let idx = if raw < 0 { len + raw } else { raw };
            let mut out = a.clone();
            if (0..len).contains(&idx) {
                out[idx as usize] = args.get(1).cloned().unwrap_or(JsValue::Undefined);
            }
            JsValue::Array(out)
        }
        "toSpliced" => {
            let len = a.len() as i64;
            let start_raw = args.first().map(to_number).unwrap_or(0.0) as i64;
            let start = if start_raw < 0 { (len + start_raw).max(0) as usize } else { (start_raw as usize).min(a.len()) };
            let delete_count = args.get(1).map(|v| to_number(v) as i64).unwrap_or(len).max(0) as usize;
            let end = (start + delete_count).min(a.len());
            let mut out = a.clone();
            out.drain(start..end);
            for (i, item) in args.iter().skip(2).enumerate() { out.insert(start + i, item.clone()); }
            JsValue::Array(out)
        }
        "copyWithin" => {
            let len = a.len() as i64;
            let norm = |raw: i64| -> usize {
                if raw < 0 { (len + raw).max(0) as usize } else { (raw as usize).min(a.len()) }
            };
            let target = norm(args.first().map(to_number).unwrap_or(0.0) as i64);
            let start = norm(args.get(1).map(to_number).unwrap_or(0.0) as i64);
            let end = norm(args.get(2).map(to_number).unwrap_or(len as f64) as i64);
            if start < end {
                let slice: Vec<JsValue> = a[start..end].to_vec();
                for (i, v) in slice.into_iter().enumerate() {
                    let pos = target + i;
                    if pos >= a.len() { break; }
                    a[pos] = v;
                }
            }
            JsValue::Array(a.clone())
        }
        _ => JsValue::Undefined,
    })
}

/// Handle `String.prototype.replace` / `replaceAll` when the replacement is a
/// function.
pub(super) fn string_replace_with_fn(s: &str, method: &str, args: &[JsValue], scope: &ScopeRef) -> EvalResult {
    let pattern = args.first().map(to_string).unwrap_or_default();
    let func = args.get(1).cloned().unwrap_or(JsValue::Undefined);
    let replace_all = method == "replaceAll";
    if pattern.is_empty() {
        return Ok(JsValue::String(s.to_string()));
    }
    let mut out = String::new();
    let mut search_start = 0;
    let mut replaced = false;
    while let Some(rel) = s[search_start..].find(pattern.as_str()) {
        let idx = search_start + rel;
        out.push_str(&s[search_start..idx]);
        let matched = &s[idx..idx + pattern.len()];
        let result = call_function(&func, &[
            JsValue::String(matched.to_string()),
            JsValue::Number(idx as f64),
            JsValue::String(s.to_string()),
        ], scope)?;
        out.push_str(&to_string(&result));
        search_start = idx + pattern.len();
        replaced = true;
        if !replace_all { break; }
    }
    if !replaced {
        return Ok(JsValue::String(s.to_string()));
    }
    out.push_str(&s[search_start..]);
    Ok(JsValue::String(out))
}

pub(super) fn call_string_method(s: &str, method: &str, args: &[JsValue]) -> JsValue {
    match method {
        "length" => JsValue::Number(s.chars().count() as f64),
        "charAt" => { let i = args.first().map(to_number).unwrap_or(0.0) as usize; s.chars().nth(i).map(|c| JsValue::String(c.to_string())).unwrap_or(JsValue::String(String::new())) }
        "charCodeAt" => { let i = args.first().map(to_number).unwrap_or(0.0) as usize; s.chars().nth(i).map(|c| JsValue::Number(c as u32 as f64)).unwrap_or(JsValue::Number(f64::NAN)) }
        "codePointAt" => { let i = args.first().map(to_number).unwrap_or(0.0) as usize; s.chars().nth(i).map(|c| JsValue::Number(c as u32 as f64)).unwrap_or(JsValue::Undefined) }
        "at" => {
            let chars: Vec<char> = s.chars().collect();
            let len = chars.len() as i64;
            let raw = args.first().map(to_number).unwrap_or(0.0) as i64;
            let idx = if raw < 0 { len + raw } else { raw };
            if (0..len).contains(&idx) { JsValue::String(chars[idx as usize].to_string()) } else { JsValue::Undefined }
        }
        "indexOf" => {
            let needle: Vec<char> = args.first().map(to_string).unwrap_or_default().chars().collect();
            let chars: Vec<char> = s.chars().collect();
            let from = args.get(1).map(|v| to_number(v) as i64).unwrap_or(0).max(0) as usize;
            let start = from.min(chars.len());
            let mut result = -1.0;
            if needle.len() <= chars.len() {
                for i in start..=(chars.len() - needle.len()) {
                    if chars[i..i + needle.len()] == needle[..] { result = i as f64; break; }
                }
            }
            JsValue::Number(result)
        }
        "lastIndexOf" => {
            let needle: Vec<char> = args.first().map(to_string).unwrap_or_default().chars().collect();
            let chars: Vec<char> = s.chars().collect();
            let mut result = -1.0;
            if needle.len() <= chars.len() {
                let max_start = chars.len() - needle.len();
                let from = args.get(1).map(to_number).unwrap_or(f64::INFINITY);
                let cap = if from.is_nan() || from >= max_start as f64 { max_start } else if from < 0.0 { 0 } else { from as usize };
                for i in (0..=cap).rev() {
                    if chars[i..i + needle.len()] == needle[..] { result = i as f64; break; }
                }
            }
            JsValue::Number(result)
        }
        "includes" => {
            let needle: Vec<char> = args.first().map(to_string).unwrap_or_default().chars().collect();
            let chars: Vec<char> = s.chars().collect();
            let pos = args.get(1).map(|v| to_number(v) as i64).unwrap_or(0).max(0) as usize;
            let start = pos.min(chars.len());
            let mut found = false;
            if needle.len() <= chars.len() {
                for i in start..=(chars.len() - needle.len()) {
                    if chars[i..i + needle.len()] == needle[..] { found = true; break; }
                }
            }
            JsValue::Boolean(found)
        }
        "startsWith" => {
            let needle: Vec<char> = args.first().map(to_string).unwrap_or_default().chars().collect();
            let chars: Vec<char> = s.chars().collect();
            let pos = args.get(1).map(|v| to_number(v) as i64).unwrap_or(0).max(0) as usize;
            let ok = pos + needle.len() <= chars.len() && chars[pos..pos + needle.len()] == needle[..];
            JsValue::Boolean(ok)
        }
        "endsWith" => {
            let needle: Vec<char> = args.first().map(to_string).unwrap_or_default().chars().collect();
            let chars: Vec<char> = s.chars().collect();
            let end = args.get(1).map(|v| (to_number(v) as i64).max(0) as usize).unwrap_or(chars.len()).min(chars.len());
            let ok = needle.len() <= end && chars[end - needle.len()..end] == needle[..];
            JsValue::Boolean(ok)
        }
        "slice" => {
            let chars: Vec<char> = s.chars().collect();
            let len = chars.len() as i64;
            let start = args.first().map(|v| to_number(v) as i64).unwrap_or(0);
            let end = args.get(1).map(|v| to_number(v) as i64).unwrap_or(len);
            let s_idx = if start < 0 { (len + start).max(0) as usize } else { (start as usize).min(len as usize) };
            let e_idx = if end < 0 { (len + end).max(0) as usize } else { (end as usize).min(len as usize) };
            JsValue::String(chars.get(s_idx..e_idx).unwrap_or(&[]).iter().collect())
        }
        "substring" => {
            let chars: Vec<char> = s.chars().collect();
            let start = args.first().map(|v| to_number(v) as usize).unwrap_or(0).min(chars.len());
            let end = args.get(1).map(|v| to_number(v) as usize).unwrap_or(chars.len()).min(chars.len());
            let (s_idx, e_idx) = if start <= end { (start, end) } else { (end, start) };
            JsValue::String(chars.get(s_idx..e_idx).unwrap_or(&[]).iter().collect())
        }
        "toLowerCase" | "toLocaleLowerCase" => JsValue::String(s.to_lowercase()),
        "toUpperCase" | "toLocaleUpperCase" => JsValue::String(s.to_uppercase()),
        "trim" => JsValue::String(s.trim().to_string()),
        "trimStart" | "trimLeft" => JsValue::String(s.trim_start().to_string()),
        "trimEnd" | "trimRight" => JsValue::String(s.trim_end().to_string()),
        "split" => {
            let limit = args.get(1).and_then(|v| if matches!(v, JsValue::Undefined) { None } else { Some(to_number(v) as usize) });
            let mut parts: Vec<JsValue> = match args.first() {
                None | Some(JsValue::Undefined) => vec![JsValue::String(s.to_string())],
                Some(sep_val) => {
                    let sep = to_string(sep_val);
                    if sep.is_empty() {
                        s.chars().map(|c| JsValue::String(c.to_string())).collect()
                    } else {
                        s.split(&sep).map(|p| JsValue::String(p.to_string())).collect()
                    }
                }
            };
            if let Some(n) = limit { parts.truncate(n); }
            JsValue::Array(parts)
        }
        "substr" => {
            let chars: Vec<char> = s.chars().collect();
            let len = chars.len() as i64;
            let raw = args.first().map(to_number).unwrap_or(0.0) as i64;
            let start = if raw < 0 { (len + raw).max(0) as usize } else { (raw as usize).min(chars.len()) };
            let count = args.get(1).map(|v| to_number(v) as i64).unwrap_or(len).max(0) as usize;
            let end = (start + count).min(chars.len());
            JsValue::String(chars.get(start..end).unwrap_or(&[]).iter().collect())
        }
        "concat" => {
            let mut out = s.to_string();
            for a in args { out.push_str(&to_string(a)); }
            JsValue::String(out)
        }
        "replace" => {
            let pattern = args.first().map(to_string).unwrap_or_default();
            let replacement = args.get(1).map(to_string).unwrap_or_default();
            match s.find(&pattern) {
                Some(idx) => {
                    let before = &s[..idx];
                    let after = &s[idx + pattern.len()..];
                    let expanded = expand_replacement(&replacement, &pattern, before, after);
                    JsValue::String(format!("{}{}{}", before, expanded, after))
                }
                None => JsValue::String(s.to_string()),
            }
        }
        "replaceAll" => {
            let pattern = args.first().map(to_string).unwrap_or_default();
            let replacement = args.get(1).map(to_string).unwrap_or_default();
            if pattern.is_empty() {
                JsValue::String(s.to_string())
            } else {
                let mut out = String::new();
                let mut search_start = 0;
                while let Some(rel) = s[search_start..].find(&pattern) {
                    let idx = search_start + rel;
                    let before = &s[..idx];
                    let after = &s[idx + pattern.len()..];
                    out.push_str(&s[search_start..idx]);
                    out.push_str(&expand_replacement(&replacement, &pattern, before, after));
                    search_start = idx + pattern.len();
                }
                out.push_str(&s[search_start..]);
                JsValue::String(out)
            }
        }
        "repeat" => {
            let n = args.first().map(to_number).unwrap_or(0.0) as usize;
            JsValue::String(s.repeat(n.min(10000)))
        }
        "padStart" => {
            let target = args.first().map(to_number).unwrap_or(0.0) as usize;
            let pad = args.get(1).map(to_string).unwrap_or_else(|| " ".into());
            JsValue::String(pad_string(s, target, &pad, true))
        }
        "padEnd" => {
            let target = args.first().map(to_number).unwrap_or(0.0) as usize;
            let pad = args.get(1).map(to_string).unwrap_or_else(|| " ".into());
            JsValue::String(pad_string(s, target, &pad, false))
        }
        "localeCompare" => {
            let other = args.first().map(to_string).unwrap_or_default();
            let cmp = match s.cmp(other.as_str()) {
                std::cmp::Ordering::Less => -1.0,
                std::cmp::Ordering::Greater => 1.0,
                std::cmp::Ordering::Equal => 0.0,
            };
            JsValue::Number(cmp)
        }
        "normalize" => JsValue::String(s.to_string()),
        // ES2024: Rust strings are always valid UTF-8 (well-formed), so
        // isWellFormed is always true and toWellFormed is the identity.
        "isWellFormed" => JsValue::Boolean(true),
        "toWellFormed" => JsValue::String(s.to_string()),
        "match" => {
            let pattern = args.first().map(to_string).unwrap_or_default();
            if pattern.is_empty() {
                JsValue::Array(vec![JsValue::String(String::new())])
            } else {
                match s.find(pattern.as_str()) {
                    Some(_) => JsValue::Array(vec![JsValue::String(pattern)]),
                    None => JsValue::Null,
                }
            }
        }
        "search" => {
            let pattern = args.first().map(to_string).unwrap_or_default();
            let idx = if pattern.is_empty() { 0 } else { s.find(pattern.as_str()).map(|i| i as i64).unwrap_or(-1) };
            JsValue::Number(idx as f64)
        }
        "matchAll" => JsValue::Null,
        "toString" | "valueOf" => JsValue::String(s.to_string()),
        _ => JsValue::Undefined,
    }
}

pub(super) fn pad_string(s: &str, target: usize, pad: &str, at_start: bool) -> String {
    let cur = s.chars().count();
    if cur >= target || pad.is_empty() { return s.to_string(); }
    let needed = target - cur;
    let pad_chars: Vec<char> = pad.chars().collect();
    let fill: String = (0..needed).map(|i| pad_chars[i % pad_chars.len()]).collect();
    if at_start { format!("{}{}", fill, s) } else { format!("{}{}", s, fill) }
}

pub(super) fn call_number_method(n: f64, method: &str, args: &[JsValue]) -> JsValue {
    match method {
        "toString" => {
            let radix = args.first().map(to_number).unwrap_or(10.0) as u32;
            if radix == 10 || !(2..=36).contains(&radix) {
                JsValue::String(format_number(n))
            } else {
                JsValue::String(number_to_radix(n, radix))
            }
        }
        "toFixed" => {
            let digits = args.first().map(to_number).unwrap_or(0.0);
            let digits = if digits.is_finite() { (digits as i64).clamp(0, 100) as usize } else { 0 };
            JsValue::String(to_fixed_js(n, digits))
        }
        "toPrecision" => {
            match args.first() {
                Some(v) if !matches!(v, JsValue::Undefined) => {
                    let p = (to_number(v) as usize).clamp(1, 100);
                    JsValue::String(to_precision_js(n, p))
                }
                _ => JsValue::String(format_number(n)),
            }
        }
        "toExponential" => {
            let frac = match args.first() {
                Some(v) if !matches!(v, JsValue::Undefined) => {
                    let d = to_number(v);
                    if d.is_finite() { Some((d as i64).clamp(0, 100) as usize) } else { None }
                }
                _ => None,
            };
            JsValue::String(to_exponential_js(n, frac))
        }
        "valueOf" => JsValue::Number(n),
        _ => JsValue::Undefined,
    }
}

pub(super) fn number_to_radix(n: f64, radix: u32) -> String {
    if !n.is_finite() { return format_number(n); }
    let negative = n < 0.0;
    let abs = n.abs();
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut int_part = abs.trunc() as u64;
    let mut out = Vec::new();
    if int_part == 0 {
        out.push(b'0');
    } else {
        let mut tmp = Vec::new();
        while int_part > 0 {
            tmp.push(digits[(int_part % radix as u64) as usize]);
            int_part /= radix as u64;
        }
        tmp.reverse();
        out.extend(tmp);
    }
    let mut frac = abs.fract();
    if frac > 0.0 {
        out.push(b'.');
        let mut count = 0;
        while frac > 0.0 && count < 20 {
            frac *= radix as f64;
            let digit = (frac.trunc() as usize).min(radix as usize - 1);
            out.push(digits[digit]);
            frac -= frac.trunc();
            count += 1;
        }
    }
    let mut result = String::new();
    if negative { result.push('-'); }
    result.push_str(&String::from_utf8(out).unwrap_or_default());
    result
}

pub(super) fn expand_replacement(replacement: &str, matched: &str, before: &str, after: &str) -> String {
    let chars: Vec<char> = replacement.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '$' && i + 1 < chars.len() {
            match chars[i + 1] {
                '$' => { out.push('$'); i += 2; continue; }
                '&' => { out.push_str(matched); i += 2; continue; }
                '`' => { out.push_str(before); i += 2; continue; }
                '\'' => { out.push_str(after); i += 2; continue; }
                _ => {}
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

pub(super) fn to_fixed_js(n: f64, digits: usize) -> String {
    if n.is_nan() { return "NaN".to_string(); }
    if n.is_infinite() { return if n < 0.0 { "-Infinity".to_string() } else { "Infinity".to_string() }; }
    let neg = n.is_sign_negative() && n != 0.0;
    let scale = 10f64.powi(digits as i32);
    let rounded = (n.abs() * scale).round();
    let scaled_str = format!("{:.0}", rounded);
    let body = if digits == 0 {
        scaled_str
    } else {
        let padded = if scaled_str.len() <= digits {
            format!("{:0>width$}", scaled_str, width = digits + 1)
        } else {
            scaled_str
        };
        let split = padded.len() - digits;
        format!("{}.{}", &padded[..split], &padded[split..])
    };
    if neg && rounded != 0.0 { format!("-{}", body) } else { body }
}

pub(super) fn to_precision_js(n: f64, p: usize) -> String {
    if n.is_nan() { return "NaN".to_string(); }
    if n.is_infinite() { return if n > 0.0 { "Infinity" } else { "-Infinity" }.to_string(); }
    let negative = n < 0.0 || (n == 0.0 && n.is_sign_negative());
    let a = n.abs();
    let prefix = if negative { "-" } else { "" };
    if a == 0.0 {
        return if p <= 1 { format!("{}0", prefix) } else { format!("{}0.{}", prefix, "0".repeat(p - 1)) };
    }
    let sci = format!("{:e}", a);
    let e_pos = sci.find('e').unwrap();
    let sig = &sci[..e_pos];
    let rust_exp: i64 = sci[e_pos + 1..].parse().unwrap_or(0);
    let mut digits: Vec<u8> = sig.chars().filter(|c| *c != '.').map(|c| c as u8 - b'0').collect();
    let point = sig.find('.').unwrap_or(sig.len());
    let mut exp10 = rust_exp + point as i64;
    if digits.len() > p {
        let mut carry = digits[p] >= 5;
        digits.truncate(p);
        let mut i = p as isize - 1;
        while carry && i >= 0 {
            digits[i as usize] += 1;
            if digits[i as usize] >= 10 { digits[i as usize] = 0; } else { carry = false; }
            i -= 1;
        }
        if carry { digits = vec![1]; exp10 += 1; }
    }
    while digits.len() < p { digits.push(0); }
    let k = digits.len() as i64;
    let e = exp10 - 1;
    let chars: Vec<char> = digits.iter().map(|d| (b'0' + d) as char).collect();
    if e >= -6 && e < p as i64 {
        let body = if exp10 >= k {
            format!("{}{}", chars.iter().collect::<String>(), "0".repeat((exp10 - k) as usize))
        } else if exp10 > 0 {
            format!("{}.{}", chars[..exp10 as usize].iter().collect::<String>(), chars[exp10 as usize..].iter().collect::<String>())
        } else {
            format!("0.{}{}", "0".repeat((-exp10) as usize), chars.iter().collect::<String>())
        };
        format!("{}{}", prefix, body)
    } else {
        let mantissa = if p == 1 {
            format!("{}", chars[0])
        } else {
            format!("{}.{}", chars[0], chars[1..].iter().collect::<String>())
        };
        format!("{}{}e{}{}", prefix, mantissa, if e >= 0 { "+" } else { "-" }, e.abs())
    }
}

pub(super) fn to_exponential_js(n: f64, frac: Option<usize>) -> String {
    if n.is_nan() { return "NaN".to_string(); }
    if n.is_infinite() { return if n > 0.0 { "Infinity" } else { "-Infinity" }.to_string(); }
    let negative = n < 0.0;
    let a = n.abs();
    if a == 0.0 {
        let mantissa = match frac { Some(f) if f > 0 => format!("0.{}", "0".repeat(f)), _ => "0".to_string() };
        return format!("{}e+0", mantissa);
    }
    let sci = format!("{:e}", a);
    let e_pos = sci.find('e').unwrap();
    let sig = &sci[..e_pos];
    let exp: i64 = sci[e_pos + 1..].parse().unwrap_or(0);
    let mut digits: String = sig.chars().filter(|c| *c != '.').collect();
    let point = sig.find('.').unwrap_or(sig.len());
    let mut exp10 = exp + point as i64 - 1;
    match frac {
        Some(f) => {
            let keep = f + 1;
            while digits.len() < keep { digits.push('0'); }
            if digits.len() > keep {
                let mut d: Vec<u8> = digits.bytes().map(|b| b - b'0').collect();
                let mut carry = d[keep] >= 5;
                d.truncate(keep);
                let mut i = keep as isize - 1;
                while carry && i >= 0 {
                    d[i as usize] += 1;
                    if d[i as usize] >= 10 { d[i as usize] = 0; carry = true; } else { carry = false; }
                    i -= 1;
                }
                if carry { d.insert(0, 1); exp10 += 1; }
                digits = d.iter().map(|x| (b'0' + x) as char).collect();
            }
            let mantissa = if f == 0 { digits[..1].to_string() } else { format!("{}.{}", &digits[..1], &digits[1..1 + f]) };
            let prefix = if negative { "-" } else { "" };
            format!("{}{}e{}{}", prefix, mantissa, if exp10 >= 0 { "+" } else { "-" }, exp10.abs())
        }
        None => {
            while digits.len() > 1 && digits.ends_with('0') { digits.pop(); }
            let mantissa = if digits.len() == 1 { digits.clone() } else { format!("{}.{}", &digits[..1], &digits[1..]) };
            let prefix = if negative { "-" } else { "" };
            format!("{}{}e{}{}", prefix, mantissa, if exp10 >= 0 { "+" } else { "-" }, exp10.abs())
        }
    }
}
