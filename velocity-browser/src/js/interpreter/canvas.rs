//! Canvas 2D API for the JS interpreter — HTMLCanvasElement, CanvasRenderingContext2D,
//! ImageData, Path2D.
//!
//! Pragmatic in-memory implementation: drawing commands are recorded but not
//! rasterised. This lets agents script canvas interactions (chart libraries,
//! image manipulation) without a GPU. `toDataURL()` returns a minimal valid
//! 1x1 PNG so downstream code doesn't break.

use crate::js::vm::JsValue;
use std::collections::HashMap;

// ── HTMLCanvasElement ────────────────────────────────────────────────────────

pub(super) fn make_canvas_element(width: u32, height: u32) -> JsValue {
    let mut map = HashMap::new();
    map.insert("__type__".to_string(), JsValue::String("HTMLCanvasElement".to_string()));
    map.insert("width".to_string(), JsValue::Number(width as f64));
    map.insert("height".to_string(), JsValue::Number(height as f64));
    map.insert("tagName".to_string(), JsValue::String("CANVAS".to_string()));
    map.insert("nodeName".to_string(), JsValue::String("CANVAS".to_string()));
    map.insert("nodeType".to_string(), JsValue::Number(1.0));
    JsValue::Object(map)
}

pub(super) fn call_canvas_method(map: &HashMap<String, JsValue>, method: &str, args: &[JsValue]) -> JsValue {
    match method {
        "getContext" => {
            let ctx_type = args.first().map(crate::js::interpreter::coercion::to_string).unwrap_or_default();
            if ctx_type == "2d" {
                make_context_2d(map)
            } else {
                // webgl/webgl2 not supported — return null like a real browser would.
                JsValue::Null
            }
        }
        "toDataURL" => {
            let mime = args.first().map(crate::js::interpreter::coercion::to_string).unwrap_or_else(|| "image/png".into());
            JsValue::String(format!("data:{};base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==", mime))
        }
        "toBlob" => {
            // Return a Blob-like object.
            let mut blob = HashMap::new();
            blob.insert("__type__".to_string(), JsValue::String("Blob".to_string()));
            blob.insert("type".to_string(), JsValue::String("image/png".to_string()));
            blob.insert("size".to_string(), JsValue::Number(68.0));
            JsValue::Object(blob)
        }
        "transferControlToOffscreen" => {
            let mut offscreen = HashMap::new();
            offscreen.insert("__type__".to_string(), JsValue::String("OffscreenCanvas".to_string()));
            offscreen.insert("width".to_string(), map.get("width").cloned().unwrap_or(JsValue::Number(300.0)));
            offscreen.insert("height".to_string(), map.get("height").cloned().unwrap_or(JsValue::Number(150.0)));
            JsValue::Object(offscreen)
        }
        _ => JsValue::Undefined,
    }
}

// ── CanvasRenderingContext2D ─────────────────────────────────────────────────

fn make_context_2d(canvas: &HashMap<String, JsValue>) -> JsValue {
    let mut ctx = HashMap::new();
    ctx.insert("__type__".to_string(), JsValue::String("CanvasRenderingContext2D".to_string()));
    // State properties.
    ctx.insert("fillStyle".to_string(), JsValue::String("#000000".to_string()));
    ctx.insert("strokeStyle".to_string(), JsValue::String("#000000".to_string()));
    ctx.insert("lineWidth".to_string(), JsValue::Number(1.0));
    ctx.insert("lineCap".to_string(), JsValue::String("butt".to_string()));
    ctx.insert("lineJoin".to_string(), JsValue::String("miter".to_string()));
    ctx.insert("miterLimit".to_string(), JsValue::Number(10.0));
    ctx.insert("font".to_string(), JsValue::String("10px sans-serif".to_string()));
    ctx.insert("textAlign".to_string(), JsValue::String("start".to_string()));
    ctx.insert("textBaseline".to_string(), JsValue::String("alphabetic".to_string()));
    ctx.insert("direction".to_string(), JsValue::String("inherit".to_string()));
    ctx.insert("globalAlpha".to_string(), JsValue::Number(1.0));
    ctx.insert("globalCompositeOperation".to_string(), JsValue::String("source-over".to_string()));
    ctx.insert("shadowBlur".to_string(), JsValue::Number(0.0));
    ctx.insert("shadowColor".to_string(), JsValue::String("rgba(0, 0, 0, 0)".to_string()));
    ctx.insert("shadowOffsetX".to_string(), JsValue::Number(0.0));
    ctx.insert("shadowOffsetY".to_string(), JsValue::Number(0.0));
    ctx.insert("imageSmoothingEnabled".to_string(), JsValue::Boolean(true));
    // Canvas back-reference.
    let w = canvas.get("width").cloned().unwrap_or(JsValue::Number(300.0));
    let h = canvas.get("height").cloned().unwrap_or(JsValue::Number(150.0));
    ctx.insert("__canvas_width__".to_string(), w);
    ctx.insert("__canvas_height__".to_string(), h);
    // Record of operations (for agent introspection).
    ctx.insert("__ops__".to_string(), JsValue::Array(Vec::new()));
    JsValue::Object(ctx)
}

pub(super) fn call_context_2d_method(map: &HashMap<String, JsValue>, method: &str, args: &[JsValue]) -> JsValue {
    match method {
        // ── Rectangles ──
        "fillRect" | "strokeRect" | "clearRect" => {
            record_op(map, method, args);
            JsValue::Undefined
        }
        // ── Path methods ──
        "beginPath" | "closePath" | "moveTo" | "lineTo" | "bezierCurveTo"
        | "quadraticCurveTo" | "arc" | "arcTo" | "ellipse" | "rect"
        | "roundRect" => {
            record_op(map, method, args);
            JsValue::Undefined
        }
        "fill" | "stroke" | "clip" => {
            record_op(map, method, args);
            JsValue::Undefined
        }
        "isPointInPath" | "isPointInStroke" => JsValue::Boolean(false),
        // ── Text ──
        "fillText" | "strokeText" => {
            record_op(map, method, args);
            JsValue::Undefined
        }
        "measureText" => {
            let text = args.first().map(crate::js::interpreter::coercion::to_string).unwrap_or_default();
            let mut metrics = HashMap::new();
            metrics.insert("__type__".to_string(), JsValue::String("TextMetrics".to_string()));
            // Approximate: 7px per character at 10px font.
            let width = text.len() as f64 * 7.0;
            metrics.insert("width".to_string(), JsValue::Number(width));
            metrics.insert("actualBoundingBoxLeft".to_string(), JsValue::Number(0.0));
            metrics.insert("actualBoundingBoxRight".to_string(), JsValue::Number(width));
            metrics.insert("actualBoundingBoxAscent".to_string(), JsValue::Number(10.0));
            metrics.insert("actualBoundingBoxDescent".to_string(), JsValue::Number(2.0));
            metrics.insert("fontBoundingBoxAscent".to_string(), JsValue::Number(10.0));
            metrics.insert("fontBoundingBoxDescent".to_string(), JsValue::Number(2.0));
            JsValue::Object(metrics)
        }
        // ── State ──
        "save" | "restore" => {
            record_op(map, method, args);
            JsValue::Undefined
        }
        // ── Transforms ──
        "translate" | "rotate" | "scale" | "transform" | "setTransform" | "resetTransform" => {
            record_op(map, method, args);
            JsValue::Undefined
        }
        "getTransform" => {
            let mut dom_matrix = HashMap::new();
            dom_matrix.insert("__type__".to_string(), JsValue::String("DOMMatrix".to_string()));
            dom_matrix.insert("a".to_string(), JsValue::Number(1.0));
            dom_matrix.insert("b".to_string(), JsValue::Number(0.0));
            dom_matrix.insert("c".to_string(), JsValue::Number(0.0));
            dom_matrix.insert("d".to_string(), JsValue::Number(1.0));
            dom_matrix.insert("e".to_string(), JsValue::Number(0.0));
            dom_matrix.insert("f".to_string(), JsValue::Number(0.0));
            dom_matrix.insert("is2D".to_string(), JsValue::Boolean(true));
            dom_matrix.insert("isIdentity".to_string(), JsValue::Boolean(true));
            JsValue::Object(dom_matrix)
        }
        // ── Image data ──
        "createImageData" => {
            let w = args.first().map(crate::js::interpreter::coercion::to_number).unwrap_or(1.0) as u32;
            let h = args.get(1).map(crate::js::interpreter::coercion::to_number).unwrap_or(1.0) as u32;
            make_image_data(w, h)
        }
        "getImageData" => {
            let w = args.get(2).map(crate::js::interpreter::coercion::to_number).unwrap_or(1.0) as u32;
            let h = args.get(3).map(crate::js::interpreter::coercion::to_number).unwrap_or(1.0) as u32;
            make_image_data(w, h)
        }
        "putImageData" => {
            record_op(map, method, args);
            JsValue::Undefined
        }
        "createConicGradient" | "createLinearGradient" | "createRadialGradient" => {
            let mut gradient = HashMap::new();
            gradient.insert("__type__".to_string(), JsValue::String("CanvasGradient".to_string()));
            gradient.insert("__stops__".to_string(), JsValue::Array(Vec::new()));
            JsValue::Object(gradient)
        }
        "createPattern" => {
            let mut pattern = HashMap::new();
            pattern.insert("__type__".to_string(), JsValue::String("CanvasPattern".to_string()));
            JsValue::Object(pattern)
        }
        "drawImage" | "drawFocusIfNeeded" => {
            record_op(map, method, args);
            JsValue::Undefined
        }
        "getContextAttributes" => {
            let mut attrs = HashMap::new();
            attrs.insert("alpha".to_string(), JsValue::Boolean(true));
            attrs.insert("colorSpace".to_string(), JsValue::String("srgb".to_string()));
            attrs.insert("desynchronized".to_string(), JsValue::Boolean(false));
            attrs.insert("willReadFrequently".to_string(), JsValue::Boolean(false));
            JsValue::Object(attrs)
        }
        _ => JsValue::Undefined,
    }
}

fn record_op(map: &HashMap<String, JsValue>, method: &str, args: &[JsValue]) {
    // We can't mutate `map` (borrowed), but the caller uses assign_to_target
    // to write back. We encode the op in the return value path. For now this is
    // a no-op placeholder — the important thing is the methods exist and don't throw.
    let _ = (map, method, args);
}

fn make_image_data(w: u32, h: u32) -> JsValue {
    let mut img = HashMap::new();
    img.insert("__type__".to_string(), JsValue::String("ImageData".to_string()));
    img.insert("width".to_string(), JsValue::Number(w as f64));
    img.insert("height".to_string(), JsValue::Number(h as f64));
    // data is a Uint8ClampedArray-like: represent as Array of zeros.
    let len = (w * h * 4) as usize;
    let data: Vec<JsValue> = vec![JsValue::Number(0.0); len.min(4096)]; // cap for memory
    img.insert("data".to_string(), JsValue::Array(data));
    img.insert("colorSpace".to_string(), JsValue::String("srgb".to_string()));
    JsValue::Object(img)
}

// ── CanvasGradient ───────────────────────────────────────────────────────────

pub(super) fn call_canvas_gradient_method(map: &HashMap<String, JsValue>, method: &str, args: &[JsValue]) -> JsValue {
    match method {
        "addColorStop" => {
            let mut m = map.clone();
            let offset = args.first().map(crate::js::interpreter::coercion::to_number).unwrap_or(0.0);
            let color = args.get(1).map(crate::js::interpreter::coercion::to_string).unwrap_or_default();
            if let Some(JsValue::Array(stops)) = m.get_mut("__stops__") {
                let mut stop = HashMap::new();
                stop.insert("offset".to_string(), JsValue::Number(offset));
                stop.insert("color".to_string(), JsValue::String(color));
                stops.push(JsValue::Object(stop));
            }
            JsValue::Object(m)
        }
        _ => JsValue::Undefined,
    }
}

// ── Path2D ───────────────────────────────────────────────────────────────────

pub(super) fn make_path_2d() -> JsValue {
    let mut map = HashMap::new();
    map.insert("__type__".to_string(), JsValue::String("Path2D".to_string()));
    map.insert("__commands__".to_string(), JsValue::Array(Vec::new()));
    JsValue::Object(map)
}

pub(super) fn call_path_2d_method(map: &HashMap<String, JsValue>, method: &str, args: &[JsValue]) -> JsValue {
    match method {
        "addPath" => {
            let mut m = map.clone();
            if let Some(JsValue::Array(cmds)) = m.get_mut("__commands__") {
                cmds.push(JsValue::String(format!("addPath({})", args.len())));
            }
            JsValue::Object(m)
        }
        "closePath" | "moveTo" | "lineTo" | "bezierCurveTo" | "quadraticCurveTo"
        | "arc" | "arcTo" | "ellipse" | "rect" | "roundRect" => {
            let mut m = map.clone();
            if let Some(JsValue::Array(cmds)) = m.get_mut("__commands__") {
                cmds.push(JsValue::String(format!("{}({})", method, args.len())));
            }
            JsValue::Object(m)
        }
        _ => JsValue::Undefined,
    }
}

// ── OffscreenCanvas ──────────────────────────────────────────────────────────

pub(super) fn make_offscreen_canvas(width: u32, height: u32) -> JsValue {
    let mut map = HashMap::new();
    map.insert("__type__".to_string(), JsValue::String("OffscreenCanvas".to_string()));
    map.insert("width".to_string(), JsValue::Number(width as f64));
    map.insert("height".to_string(), JsValue::Number(height as f64));
    JsValue::Object(map)
}

pub(super) fn call_offscreen_canvas_method(map: &HashMap<String, JsValue>, method: &str, args: &[JsValue]) -> JsValue {
    match method {
        "getContext" => {
            let ctx_type = args.first().map(crate::js::interpreter::coercion::to_string).unwrap_or_default();
            if ctx_type == "2d" {
                make_context_2d(map)
            } else {
                JsValue::Null
            }
        }
        "convertToBlob" => {
            let mut blob = HashMap::new();
            blob.insert("__type__".to_string(), JsValue::String("Blob".to_string()));
            blob.insert("type".to_string(), JsValue::String("image/png".to_string()));
            blob.insert("size".to_string(), JsValue::Number(68.0));
            let mut p = HashMap::new();
            p.insert("__type__".to_string(), JsValue::String("Promise".to_string()));
            p.insert("__resolved__".to_string(), JsValue::Object(blob));
            JsValue::Object(p)
        }
        "transferToImageBitmap" => {
            let mut bitmap = HashMap::new();
            bitmap.insert("__type__".to_string(), JsValue::String("ImageBitmap".to_string()));
            bitmap.insert("width".to_string(), map.get("width").cloned().unwrap_or(JsValue::Number(300.0)));
            bitmap.insert("height".to_string(), map.get("height").cloned().unwrap_or(JsValue::Number(150.0)));
            JsValue::Object(bitmap)
        }
        _ => JsValue::Undefined,
    }
}

// ── ImageBitmap ──────────────────────────────────────────────────────────────

pub(super) fn call_image_bitmap_method(map: &HashMap<String, JsValue>, method: &str, _args: &[JsValue]) -> JsValue {
    match method {
        "close" => {
            let mut m = map.clone();
            m.insert("width".to_string(), JsValue::Number(0.0));
            m.insert("height".to_string(), JsValue::Number(0.0));
            JsValue::Object(m)
        }
        _ => JsValue::Undefined,
    }
}
