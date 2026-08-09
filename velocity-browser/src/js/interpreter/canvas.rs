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
    map.insert(
        "__type__".to_string(),
        JsValue::String("HTMLCanvasElement".to_string()),
    );
    map.insert("width".to_string(), JsValue::Number(width as f64));
    map.insert("height".to_string(), JsValue::Number(height as f64));
    map.insert("tagName".to_string(), JsValue::String("CANVAS".to_string()));
    map.insert(
        "nodeName".to_string(),
        JsValue::String("CANVAS".to_string()),
    );
    map.insert("nodeType".to_string(), JsValue::Number(1.0));
    JsValue::Object(map)
}

pub(super) fn call_canvas_method(
    map: &HashMap<String, JsValue>,
    method: &str,
    args: &[JsValue],
) -> JsValue {
    match method {
        "getContext" => {
            let ctx_type = args
                .first()
                .map(crate::js::interpreter::coercion::to_string)
                .unwrap_or_default();
            if ctx_type == "2d" {
                make_context_2d(map)
            } else {
                // webgl/webgl2 not supported — return null like a real browser would.
                JsValue::Null
            }
        }
        "toDataURL" => {
            let mime = args
                .first()
                .map(crate::js::interpreter::coercion::to_string)
                .unwrap_or_else(|| "image/png".into());
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
            offscreen.insert(
                "__type__".to_string(),
                JsValue::String("OffscreenCanvas".to_string()),
            );
            offscreen.insert(
                "width".to_string(),
                map.get("width").cloned().unwrap_or(JsValue::Number(300.0)),
            );
            offscreen.insert(
                "height".to_string(),
                map.get("height").cloned().unwrap_or(JsValue::Number(150.0)),
            );
            JsValue::Object(offscreen)
        }
        _ => JsValue::Undefined,
    }
}

// ── CanvasRenderingContext2D ─────────────────────────────────────────────────

fn make_context_2d(canvas: &HashMap<String, JsValue>) -> JsValue {
    let mut ctx = HashMap::new();
    ctx.insert(
        "__type__".to_string(),
        JsValue::String("CanvasRenderingContext2D".to_string()),
    );
    // State properties.
    ctx.insert(
        "fillStyle".to_string(),
        JsValue::String("#000000".to_string()),
    );
    ctx.insert(
        "strokeStyle".to_string(),
        JsValue::String("#000000".to_string()),
    );
    ctx.insert("lineWidth".to_string(), JsValue::Number(1.0));
    ctx.insert("lineCap".to_string(), JsValue::String("butt".to_string()));
    ctx.insert("lineJoin".to_string(), JsValue::String("miter".to_string()));
    ctx.insert("miterLimit".to_string(), JsValue::Number(10.0));
    ctx.insert(
        "font".to_string(),
        JsValue::String("10px sans-serif".to_string()),
    );
    ctx.insert(
        "textAlign".to_string(),
        JsValue::String("start".to_string()),
    );
    ctx.insert(
        "textBaseline".to_string(),
        JsValue::String("alphabetic".to_string()),
    );
    ctx.insert(
        "direction".to_string(),
        JsValue::String("inherit".to_string()),
    );
    ctx.insert("globalAlpha".to_string(), JsValue::Number(1.0));
    ctx.insert(
        "globalCompositeOperation".to_string(),
        JsValue::String("source-over".to_string()),
    );
    ctx.insert("shadowBlur".to_string(), JsValue::Number(0.0));
    ctx.insert(
        "shadowColor".to_string(),
        JsValue::String("rgba(0, 0, 0, 0)".to_string()),
    );
    ctx.insert("shadowOffsetX".to_string(), JsValue::Number(0.0));
    ctx.insert("shadowOffsetY".to_string(), JsValue::Number(0.0));
    ctx.insert("imageSmoothingEnabled".to_string(), JsValue::Boolean(true));
    // Canvas back-reference.
    let w = canvas
        .get("width")
        .cloned()
        .unwrap_or(JsValue::Number(300.0));
    let h = canvas
        .get("height")
        .cloned()
        .unwrap_or(JsValue::Number(150.0));
    ctx.insert("__canvas_width__".to_string(), w);
    ctx.insert("__canvas_height__".to_string(), h);
    // Record of operations (for agent introspection).
    ctx.insert("__ops__".to_string(), JsValue::Array(Vec::new()));
    JsValue::Object(ctx)
}

pub(super) fn call_context_2d_method(
    map: &HashMap<String, JsValue>,
    method: &str,
    args: &[JsValue],
) -> JsValue {
    match method {
        // ── Rectangles ──
        "fillRect" | "strokeRect" | "clearRect" => JsValue::Object(record_op(map, method, args)),
        // ── Path methods ──
        "beginPath" | "closePath" | "moveTo" | "lineTo" | "bezierCurveTo" | "quadraticCurveTo"
        | "arc" | "arcTo" | "ellipse" | "rect" | "roundRect" => {
            JsValue::Object(record_op(map, method, args))
        }
        "fill" | "stroke" | "clip" => JsValue::Object(record_op(map, method, args)),
        "isPointInPath" | "isPointInStroke" => JsValue::Boolean(false),
        // ── Text ──
        "fillText" | "strokeText" => JsValue::Object(record_op(map, method, args)),
        "measureText" => {
            let text = args
                .first()
                .map(crate::js::interpreter::coercion::to_string)
                .unwrap_or_default();
            let mut metrics = HashMap::new();
            metrics.insert(
                "__type__".to_string(),
                JsValue::String("TextMetrics".to_string()),
            );
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
        "save" | "restore" => JsValue::Object(record_op(map, method, args)),
        // ── Transforms ──
        "translate" | "rotate" | "scale" | "transform" | "setTransform" | "resetTransform" => {
            JsValue::Object(record_op(map, method, args))
        }
        "getTransform" => {
            let mut dom_matrix = HashMap::new();
            dom_matrix.insert(
                "__type__".to_string(),
                JsValue::String("DOMMatrix".to_string()),
            );
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
            let w = args
                .first()
                .map(crate::js::interpreter::coercion::to_number)
                .unwrap_or(1.0) as u32;
            let h = args
                .get(1)
                .map(crate::js::interpreter::coercion::to_number)
                .unwrap_or(1.0) as u32;
            make_image_data(w, h)
        }
        "getImageData" => {
            let w = args
                .get(2)
                .map(crate::js::interpreter::coercion::to_number)
                .unwrap_or(1.0) as u32;
            let h = args
                .get(3)
                .map(crate::js::interpreter::coercion::to_number)
                .unwrap_or(1.0) as u32;
            make_image_data(w, h)
        }
        "putImageData" => JsValue::Object(record_op(map, method, args)),
        "createConicGradient" | "createLinearGradient" | "createRadialGradient" => {
            let mut gradient = HashMap::new();
            gradient.insert(
                "__type__".to_string(),
                JsValue::String("CanvasGradient".to_string()),
            );
            gradient.insert("__stops__".to_string(), JsValue::Array(Vec::new()));
            JsValue::Object(gradient)
        }
        "createPattern" => {
            let mut pattern = HashMap::new();
            pattern.insert(
                "__type__".to_string(),
                JsValue::String("CanvasPattern".to_string()),
            );
            JsValue::Object(pattern)
        }
        "drawImage" | "drawFocusIfNeeded" => JsValue::Object(record_op(map, method, args)),
        "getContextAttributes" => {
            let mut attrs = HashMap::new();
            attrs.insert("alpha".to_string(), JsValue::Boolean(true));
            attrs.insert(
                "colorSpace".to_string(),
                JsValue::String("srgb".to_string()),
            );
            attrs.insert("desynchronized".to_string(), JsValue::Boolean(false));
            attrs.insert("willReadFrequently".to_string(), JsValue::Boolean(false));
            JsValue::Object(attrs)
        }
        _ => JsValue::Undefined,
    }
}

fn record_op(
    map: &HashMap<String, JsValue>,
    method: &str,
    args: &[JsValue],
) -> HashMap<String, JsValue> {
    // Record the operation in the __ops__ array for agent introspection
    let mut m = map.clone();
    if let Some(JsValue::Array(ops)) = m.get_mut("__ops__") {
        let mut op = HashMap::new();
        op.insert("method".to_string(), JsValue::String(method.to_string()));
        // Serialize arguments as strings for introspection
        let arg_strs: Vec<JsValue> = args
            .iter()
            .map(|a| match a {
                JsValue::Number(n) => JsValue::String(format!("{}", n)),
                JsValue::String(s) => JsValue::String(format!("\"{}\"", s)),
                JsValue::Boolean(b) => JsValue::String(format!("{}", b)),
                _ => JsValue::String("[object]".to_string()),
            })
            .collect();
        op.insert("args".to_string(), JsValue::Array(arg_strs));
        op.insert(
            "timestamp".to_string(),
            JsValue::Number(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs_f64() * 1000.0)
                    .unwrap_or(0.0),
            ),
        );
        ops.push(JsValue::Object(op));
    }
    m
}

fn make_image_data(w: u32, h: u32) -> JsValue {
    let mut img = HashMap::new();
    img.insert(
        "__type__".to_string(),
        JsValue::String("ImageData".to_string()),
    );
    img.insert("width".to_string(), JsValue::Number(w as f64));
    img.insert("height".to_string(), JsValue::Number(h as f64));
    // data is a Uint8ClampedArray-like: represent as Array of zeros.
    let len = (w * h * 4) as usize;
    let data: Vec<JsValue> = vec![JsValue::Number(0.0); len.min(4096)]; // cap for memory
    img.insert("data".to_string(), JsValue::Array(data));
    img.insert(
        "colorSpace".to_string(),
        JsValue::String("srgb".to_string()),
    );
    JsValue::Object(img)
}

// ── CanvasGradient ───────────────────────────────────────────────────────────

pub(super) fn call_canvas_gradient_method(
    map: &HashMap<String, JsValue>,
    method: &str,
    args: &[JsValue],
) -> JsValue {
    match method {
        "addColorStop" => {
            let mut m = map.clone();
            let offset = args
                .first()
                .map(crate::js::interpreter::coercion::to_number)
                .unwrap_or(0.0);
            let color = args
                .get(1)
                .map(crate::js::interpreter::coercion::to_string)
                .unwrap_or_default();
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
    map.insert(
        "__type__".to_string(),
        JsValue::String("Path2D".to_string()),
    );
    map.insert("__commands__".to_string(), JsValue::Array(Vec::new()));
    JsValue::Object(map)
}

pub(super) fn call_path_2d_method(
    map: &HashMap<String, JsValue>,
    method: &str,
    args: &[JsValue],
) -> JsValue {
    match method {
        "addPath" => {
            let mut m = map.clone();
            if let Some(JsValue::Array(cmds)) = m.get_mut("__commands__") {
                cmds.push(JsValue::String(format!("addPath({})", args.len())));
            }
            JsValue::Object(m)
        }
        "closePath" | "moveTo" | "lineTo" | "bezierCurveTo" | "quadraticCurveTo" | "arc"
        | "arcTo" | "ellipse" | "rect" | "roundRect" => {
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
    map.insert(
        "__type__".to_string(),
        JsValue::String("OffscreenCanvas".to_string()),
    );
    map.insert("width".to_string(), JsValue::Number(width as f64));
    map.insert("height".to_string(), JsValue::Number(height as f64));
    JsValue::Object(map)
}

pub(super) fn call_offscreen_canvas_method(
    map: &HashMap<String, JsValue>,
    method: &str,
    args: &[JsValue],
) -> JsValue {
    match method {
        "getContext" => {
            let ctx_type = args
                .first()
                .map(crate::js::interpreter::coercion::to_string)
                .unwrap_or_default();
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
            p.insert(
                "__type__".to_string(),
                JsValue::String("Promise".to_string()),
            );
            p.insert("__resolved__".to_string(), JsValue::Object(blob));
            JsValue::Object(p)
        }
        "transferToImageBitmap" => {
            let mut bitmap = HashMap::new();
            bitmap.insert(
                "__type__".to_string(),
                JsValue::String("ImageBitmap".to_string()),
            );
            bitmap.insert(
                "width".to_string(),
                map.get("width").cloned().unwrap_or(JsValue::Number(300.0)),
            );
            bitmap.insert(
                "height".to_string(),
                map.get("height").cloned().unwrap_or(JsValue::Number(150.0)),
            );
            JsValue::Object(bitmap)
        }
        _ => JsValue::Undefined,
    }
}

// ── ImageBitmap ──────────────────────────────────────────────────────────────

pub(super) fn call_image_bitmap_method(
    map: &HashMap<String, JsValue>,
    method: &str,
    _args: &[JsValue],
) -> JsValue {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn get_obj(v: &JsValue) -> &HashMap<String, JsValue> {
        match v {
            JsValue::Object(m) => m,
            _ => panic!("expected Object"),
        }
    }

    fn get_str(v: &JsValue) -> &str {
        match v {
            JsValue::String(s) => s.as_str(),
            _ => panic!("expected String"),
        }
    }

    fn get_num(v: &JsValue) -> f64 {
        match v {
            JsValue::Number(n) => *n,
            _ => panic!("expected Number"),
        }
    }

    // ── make_canvas_element ──────────────────────────────────────────────

    #[test]
    fn canvas_element_structure() {
        let c = make_canvas_element(800, 600);
        let m = get_obj(&c);
        assert_eq!(get_str(m.get("__type__").unwrap()), "HTMLCanvasElement");
        assert_eq!(get_num(m.get("width").unwrap()), 800.0);
        assert_eq!(get_num(m.get("height").unwrap()), 600.0);
        assert_eq!(get_str(m.get("tagName").unwrap()), "CANVAS");
        assert_eq!(get_num(m.get("nodeType").unwrap()), 1.0);
    }

    #[test]
    fn canvas_element_zero_size() {
        let c = make_canvas_element(0, 0);
        let m = get_obj(&c);
        assert_eq!(get_num(m.get("width").unwrap()), 0.0);
        assert_eq!(get_num(m.get("height").unwrap()), 0.0);
    }

    // ── call_canvas_method ───────────────────────────────────────────────

    #[test]
    fn canvas_get_context_2d() {
        let c = make_canvas_element(300, 150);
        let m = get_obj(&c);
        let ctx = call_canvas_method(m, "getContext", &[JsValue::String("2d".into())]);
        let ctx_map = get_obj(&ctx);
        assert_eq!(
            get_str(ctx_map.get("__type__").unwrap()),
            "CanvasRenderingContext2D"
        );
        assert_eq!(get_str(ctx_map.get("fillStyle").unwrap()), "#000000");
        assert_eq!(get_num(ctx_map.get("lineWidth").unwrap()), 1.0);
    }

    #[test]
    fn canvas_get_context_webgl_returns_null() {
        let c = make_canvas_element(300, 150);
        let m = get_obj(&c);
        let ctx = call_canvas_method(m, "getContext", &[JsValue::String("webgl".into())]);
        assert!(matches!(ctx, JsValue::Null));
    }

    #[test]
    fn canvas_to_data_url() {
        let c = make_canvas_element(300, 150);
        let m = get_obj(&c);
        let url = call_canvas_method(m, "toDataURL", &[]);
        let s = get_str(&url);
        assert!(s.starts_with("data:image/png;base64,"));
    }

    #[test]
    fn canvas_to_data_url_custom_mime() {
        let c = make_canvas_element(300, 150);
        let m = get_obj(&c);
        let url = call_canvas_method(m, "toDataURL", &[JsValue::String("image/jpeg".into())]);
        let s = get_str(&url);
        assert!(s.starts_with("data:image/jpeg;base64,"));
    }

    #[test]
    fn canvas_to_blob() {
        let c = make_canvas_element(300, 150);
        let m = get_obj(&c);
        let blob = call_canvas_method(m, "toBlob", &[]);
        let b = get_obj(&blob);
        assert_eq!(get_str(b.get("__type__").unwrap()), "Blob");
        assert_eq!(get_str(b.get("type").unwrap()), "image/png");
        assert_eq!(get_num(b.get("size").unwrap()), 68.0);
    }

    #[test]
    fn canvas_transfer_control_to_offscreen() {
        let c = make_canvas_element(400, 200);
        let m = get_obj(&c);
        let offscreen = call_canvas_method(m, "transferControlToOffscreen", &[]);
        let o = get_obj(&offscreen);
        assert_eq!(get_str(o.get("__type__").unwrap()), "OffscreenCanvas");
        assert_eq!(get_num(o.get("width").unwrap()), 400.0);
        assert_eq!(get_num(o.get("height").unwrap()), 200.0);
    }

    #[test]
    fn canvas_unknown_method() {
        let c = make_canvas_element(300, 150);
        let m = get_obj(&c);
        let result = call_canvas_method(m, "unknownMethod", &[]);
        assert!(matches!(result, JsValue::Undefined));
    }

    // ── call_context_2d_method ───────────────────────────────────────────

    #[test]
    fn context_2d_fill_rect() {
        let c = make_canvas_element(300, 150);
        let ctx = call_canvas_method(get_obj(&c), "getContext", &[JsValue::String("2d".into())]);
        let ctx_map = get_obj(&ctx);
        let result = call_context_2d_method(
            ctx_map,
            "fillRect",
            &[
                JsValue::Number(10.0),
                JsValue::Number(20.0),
                JsValue::Number(100.0),
                JsValue::Number(50.0),
            ],
        );
        // fillRect now returns an Object with recorded operations
        let result_map = get_obj(&result);
        assert_eq!(
            get_str(result_map.get("__type__").unwrap()),
            "CanvasRenderingContext2D"
        );
        // Verify the operation was recorded
        if let Some(JsValue::Array(ops)) = result_map.get("__ops__") {
            assert!(!ops.is_empty(), "fillRect should be recorded in __ops__");
        }
    }

    #[test]
    fn context_2d_measure_text() {
        let c = make_canvas_element(300, 150);
        let ctx = call_canvas_method(get_obj(&c), "getContext", &[JsValue::String("2d".into())]);
        let ctx_map = get_obj(&ctx);
        let metrics =
            call_context_2d_method(ctx_map, "measureText", &[JsValue::String("hello".into())]);
        let m = get_obj(&metrics);
        assert_eq!(get_str(m.get("__type__").unwrap()), "TextMetrics");
        assert_eq!(get_num(m.get("width").unwrap()), 35.0); // 5 chars * 7px
        assert_eq!(get_num(m.get("actualBoundingBoxAscent").unwrap()), 10.0);
    }

    #[test]
    fn context_2d_measure_text_empty() {
        let c = make_canvas_element(300, 150);
        let ctx = call_canvas_method(get_obj(&c), "getContext", &[JsValue::String("2d".into())]);
        let ctx_map = get_obj(&ctx);
        let metrics = call_context_2d_method(ctx_map, "measureText", &[JsValue::String("".into())]);
        let m = get_obj(&metrics);
        assert_eq!(get_num(m.get("width").unwrap()), 0.0);
    }

    #[test]
    fn context_2d_get_transform() {
        let c = make_canvas_element(300, 150);
        let ctx = call_canvas_method(get_obj(&c), "getContext", &[JsValue::String("2d".into())]);
        let ctx_map = get_obj(&ctx);
        let transform = call_context_2d_method(ctx_map, "getTransform", &[]);
        let t = get_obj(&transform);
        assert_eq!(get_str(t.get("__type__").unwrap()), "DOMMatrix");
        assert_eq!(get_num(t.get("a").unwrap()), 1.0);
        assert_eq!(get_num(t.get("d").unwrap()), 1.0);
        assert_eq!(get_num(t.get("e").unwrap()), 0.0);
        assert_eq!(get_num(t.get("f").unwrap()), 0.0);
        assert!(matches!(
            t.get("isIdentity").unwrap(),
            JsValue::Boolean(true)
        ));
    }

    #[test]
    fn context_2d_create_image_data() {
        let c = make_canvas_element(300, 150);
        let ctx = call_canvas_method(get_obj(&c), "getContext", &[JsValue::String("2d".into())]);
        let ctx_map = get_obj(&ctx);
        let img = call_context_2d_method(
            ctx_map,
            "createImageData",
            &[JsValue::Number(10.0), JsValue::Number(5.0)],
        );
        let i = get_obj(&img);
        assert_eq!(get_str(i.get("__type__").unwrap()), "ImageData");
        assert_eq!(get_num(i.get("width").unwrap()), 10.0);
        assert_eq!(get_num(i.get("height").unwrap()), 5.0);
        assert_eq!(get_str(i.get("colorSpace").unwrap()), "srgb");
        if let JsValue::Array(data) = i.get("data").unwrap() {
            assert_eq!(data.len(), 200); // 10 * 5 * 4
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn context_2d_create_linear_gradient() {
        let c = make_canvas_element(300, 150);
        let ctx = call_canvas_method(get_obj(&c), "getContext", &[JsValue::String("2d".into())]);
        let ctx_map = get_obj(&ctx);
        let gradient = call_context_2d_method(
            ctx_map,
            "createLinearGradient",
            &[
                JsValue::Number(0.0),
                JsValue::Number(0.0),
                JsValue::Number(100.0),
                JsValue::Number(0.0),
            ],
        );
        let g = get_obj(&gradient);
        assert_eq!(get_str(g.get("__type__").unwrap()), "CanvasGradient");
        if let JsValue::Array(stops) = g.get("__stops__").unwrap() {
            assert_eq!(stops.len(), 0);
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn context_2d_is_point_in_path() {
        let c = make_canvas_element(300, 150);
        let ctx = call_canvas_method(get_obj(&c), "getContext", &[JsValue::String("2d".into())]);
        let ctx_map = get_obj(&ctx);
        let result = call_context_2d_method(
            ctx_map,
            "isPointInPath",
            &[JsValue::Number(10.0), JsValue::Number(20.0)],
        );
        assert!(matches!(result, JsValue::Boolean(false)));
    }

    #[test]
    fn context_2d_get_context_attributes() {
        let c = make_canvas_element(300, 150);
        let ctx = call_canvas_method(get_obj(&c), "getContext", &[JsValue::String("2d".into())]);
        let ctx_map = get_obj(&ctx);
        let attrs = call_context_2d_method(ctx_map, "getContextAttributes", &[]);
        let a = get_obj(&attrs);
        assert!(matches!(a.get("alpha").unwrap(), JsValue::Boolean(true)));
        assert_eq!(get_str(a.get("colorSpace").unwrap()), "srgb");
    }

    #[test]
    fn context_2d_unknown_method() {
        let c = make_canvas_element(300, 150);
        let ctx = call_canvas_method(get_obj(&c), "getContext", &[JsValue::String("2d".into())]);
        let ctx_map = get_obj(&ctx);
        let result = call_context_2d_method(ctx_map, "unknownMethod", &[]);
        assert!(matches!(result, JsValue::Undefined));
    }

    // ── call_canvas_gradient_method ──────────────────────────────────────

    #[test]
    fn gradient_add_color_stop() {
        let mut gradient = HashMap::new();
        gradient.insert(
            "__type__".to_string(),
            JsValue::String("CanvasGradient".into()),
        );
        gradient.insert("__stops__".to_string(), JsValue::Array(Vec::new()));

        let result = call_canvas_gradient_method(
            &gradient,
            "addColorStop",
            &[JsValue::Number(0.0), JsValue::String("red".into())],
        );
        let g = get_obj(&result);
        if let JsValue::Array(stops) = g.get("__stops__").unwrap() {
            assert_eq!(stops.len(), 1);
            if let JsValue::Object(stop) = &stops[0] {
                assert_eq!(get_num(stop.get("offset").unwrap()), 0.0);
                assert_eq!(get_str(stop.get("color").unwrap()), "red");
            } else {
                panic!("expected Object");
            }
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn gradient_add_multiple_color_stops() {
        let mut gradient = HashMap::new();
        gradient.insert(
            "__type__".to_string(),
            JsValue::String("CanvasGradient".into()),
        );
        gradient.insert("__stops__".to_string(), JsValue::Array(Vec::new()));

        let g1 = call_canvas_gradient_method(
            &gradient,
            "addColorStop",
            &[JsValue::Number(0.0), JsValue::String("red".into())],
        );
        let g2 = call_canvas_gradient_method(
            get_obj(&g1),
            "addColorStop",
            &[JsValue::Number(1.0), JsValue::String("blue".into())],
        );
        let g = get_obj(&g2);
        if let JsValue::Array(stops) = g.get("__stops__").unwrap() {
            assert_eq!(stops.len(), 2);
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn gradient_unknown_method() {
        let mut gradient = HashMap::new();
        gradient.insert(
            "__type__".to_string(),
            JsValue::String("CanvasGradient".into()),
        );
        gradient.insert("__stops__".to_string(), JsValue::Array(Vec::new()));

        let result = call_canvas_gradient_method(&gradient, "unknownMethod", &[]);
        assert!(matches!(result, JsValue::Undefined));
    }

    // ── make_path_2d and call_path_2d_method ─────────────────────────────

    #[test]
    fn path_2d_structure() {
        let p = make_path_2d();
        let m = get_obj(&p);
        assert_eq!(get_str(m.get("__type__").unwrap()), "Path2D");
        if let JsValue::Array(cmds) = m.get("__commands__").unwrap() {
            assert_eq!(cmds.len(), 0);
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn path_2d_move_to() {
        let p = make_path_2d();
        let result = call_path_2d_method(
            get_obj(&p),
            "moveTo",
            &[JsValue::Number(10.0), JsValue::Number(20.0)],
        );
        let m = get_obj(&result);
        if let JsValue::Array(cmds) = m.get("__commands__").unwrap() {
            assert_eq!(cmds.len(), 1);
            assert_eq!(get_str(&cmds[0]), "moveTo(2)");
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn path_2d_multiple_commands() {
        let p = make_path_2d();
        let p1 = call_path_2d_method(
            get_obj(&p),
            "moveTo",
            &[JsValue::Number(10.0), JsValue::Number(20.0)],
        );
        let p2 = call_path_2d_method(
            get_obj(&p1),
            "lineTo",
            &[JsValue::Number(100.0), JsValue::Number(100.0)],
        );
        let p3 = call_path_2d_method(get_obj(&p2), "closePath", &[]);
        let m = get_obj(&p3);
        if let JsValue::Array(cmds) = m.get("__commands__").unwrap() {
            assert_eq!(cmds.len(), 3);
            assert_eq!(get_str(&cmds[0]), "moveTo(2)");
            assert_eq!(get_str(&cmds[1]), "lineTo(2)");
            assert_eq!(get_str(&cmds[2]), "closePath(0)");
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn path_2d_unknown_method() {
        let p = make_path_2d();
        let result = call_path_2d_method(get_obj(&p), "unknownMethod", &[]);
        assert!(matches!(result, JsValue::Undefined));
    }

    // ── make_offscreen_canvas and call_offscreen_canvas_method ───────────

    #[test]
    fn offscreen_canvas_structure() {
        let c = make_offscreen_canvas(500, 400);
        let m = get_obj(&c);
        assert_eq!(get_str(m.get("__type__").unwrap()), "OffscreenCanvas");
        assert_eq!(get_num(m.get("width").unwrap()), 500.0);
        assert_eq!(get_num(m.get("height").unwrap()), 400.0);
    }

    #[test]
    fn offscreen_canvas_get_context_2d() {
        let c = make_offscreen_canvas(500, 400);
        let m = get_obj(&c);
        let ctx = call_offscreen_canvas_method(m, "getContext", &[JsValue::String("2d".into())]);
        let ctx_map = get_obj(&ctx);
        assert_eq!(
            get_str(ctx_map.get("__type__").unwrap()),
            "CanvasRenderingContext2D"
        );
    }

    #[test]
    fn offscreen_canvas_get_context_webgl_returns_null() {
        let c = make_offscreen_canvas(500, 400);
        let m = get_obj(&c);
        let ctx = call_offscreen_canvas_method(m, "getContext", &[JsValue::String("webgl".into())]);
        assert!(matches!(ctx, JsValue::Null));
    }

    #[test]
    fn offscreen_canvas_convert_to_blob() {
        let c = make_offscreen_canvas(500, 400);
        let m = get_obj(&c);
        let blob = call_offscreen_canvas_method(m, "convertToBlob", &[]);
        let b = get_obj(&blob);
        assert_eq!(get_str(b.get("__type__").unwrap()), "Promise");
        if let JsValue::Object(resolved) = b.get("__resolved__").unwrap() {
            assert_eq!(get_str(resolved.get("__type__").unwrap()), "Blob");
            assert_eq!(get_str(resolved.get("type").unwrap()), "image/png");
        } else {
            panic!("expected Object");
        }
    }

    #[test]
    fn offscreen_canvas_transfer_to_image_bitmap() {
        let c = make_offscreen_canvas(500, 400);
        let m = get_obj(&c);
        let bitmap = call_offscreen_canvas_method(m, "transferToImageBitmap", &[]);
        let b = get_obj(&bitmap);
        assert_eq!(get_str(b.get("__type__").unwrap()), "ImageBitmap");
        assert_eq!(get_num(b.get("width").unwrap()), 500.0);
        assert_eq!(get_num(b.get("height").unwrap()), 400.0);
    }

    #[test]
    fn offscreen_canvas_unknown_method() {
        let c = make_offscreen_canvas(500, 400);
        let m = get_obj(&c);
        let result = call_offscreen_canvas_method(m, "unknownMethod", &[]);
        assert!(matches!(result, JsValue::Undefined));
    }

    // ── call_image_bitmap_method ─────────────────────────────────────────

    #[test]
    fn image_bitmap_close() {
        let mut bitmap = HashMap::new();
        bitmap.insert(
            "__type__".to_string(),
            JsValue::String("ImageBitmap".into()),
        );
        bitmap.insert("width".to_string(), JsValue::Number(100.0));
        bitmap.insert("height".to_string(), JsValue::Number(50.0));

        let result = call_image_bitmap_method(&bitmap, "close", &[]);
        let m = get_obj(&result);
        assert_eq!(get_num(m.get("width").unwrap()), 0.0);
        assert_eq!(get_num(m.get("height").unwrap()), 0.0);
    }

    #[test]
    fn image_bitmap_unknown_method() {
        let mut bitmap = HashMap::new();
        bitmap.insert(
            "__type__".to_string(),
            JsValue::String("ImageBitmap".into()),
        );
        bitmap.insert("width".to_string(), JsValue::Number(100.0));
        bitmap.insert("height".to_string(), JsValue::Number(50.0));

        let result = call_image_bitmap_method(&bitmap, "unknownMethod", &[]);
        assert!(matches!(result, JsValue::Undefined));
    }
}
