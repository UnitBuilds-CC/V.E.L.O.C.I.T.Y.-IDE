//! Tests for Canvas 2D API: HTMLCanvasElement, CanvasRenderingContext2D,
//! Path2D, OffscreenCanvas, ImageData.

use super::*;

// ── HTMLCanvasElement ────────────────────────────────────────────────────────

#[test]
fn canvas_create_element() {
    let result = eval_full(
        r#"
        var c = document.createElement('canvas');
        c.__type__
    "#,
    );
    assert_eq!(result, JsValue::String("HTMLCanvasElement".to_string()));
}

#[test]
fn canvas_default_dimensions() {
    let result = eval_full(
        r#"
        var c = document.createElement('canvas');
        c.width
    "#,
    );
    assert_eq!(result, JsValue::Number(300.0));
}

#[test]
fn canvas_get_context_2d() {
    let result = eval_full(
        r#"
        var c = document.createElement('canvas');
        var ctx = c.getContext('2d');
        ctx.__type__
    "#,
    );
    assert_eq!(
        result,
        JsValue::String("CanvasRenderingContext2D".to_string())
    );
}

#[test]
fn canvas_get_context_webgl_null() {
    let result = eval_full(
        r#"
        var c = document.createElement('canvas');
        c.getContext('webgl')
    "#,
    );
    assert_eq!(result, JsValue::Null);
}

#[test]
fn canvas_to_data_url() {
    let result = eval_full(
        r#"
        var c = document.createElement('canvas');
        c.toDataURL().startsWith('data:image/png')
    "#,
    );
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn canvas_to_blob() {
    let result = eval_full(
        r#"
        var c = document.createElement('canvas');
        var blob = c.toBlob();
        blob.type
    "#,
    );
    assert_eq!(result, JsValue::String("image/png".to_string()));
}

// ── CanvasRenderingContext2D ─────────────────────────────────────────────────

#[test]
fn ctx_default_fill_style() {
    let result = eval_full(
        r#"
        var c = document.createElement('canvas');
        var ctx = c.getContext('2d');
        ctx.fillStyle
    "#,
    );
    assert_eq!(result, JsValue::String("#000000".to_string()));
}

#[test]
fn ctx_fill_rect() {
    let result = eval_full(
        r#"
        var c = document.createElement('canvas');
        var ctx = c.getContext('2d');
        ctx.fillRect(10, 20, 100, 50);
        'ok'
    "#,
    );
    assert_eq!(result, JsValue::String("ok".to_string()));
}

#[test]
fn ctx_path_methods() {
    let result = eval_full(
        r#"
        var c = document.createElement('canvas');
        var ctx = c.getContext('2d');
        ctx.beginPath();
        ctx.moveTo(0, 0);
        ctx.lineTo(100, 100);
        ctx.arc(50, 50, 40, 0, 6.28);
        ctx.closePath();
        ctx.stroke();
        'done'
    "#,
    );
    assert_eq!(result, JsValue::String("done".to_string()));
}

#[test]
fn ctx_measure_text() {
    let result = eval_full(
        r#"
        var c = document.createElement('canvas');
        var ctx = c.getContext('2d');
        var m = ctx.measureText('Hello');
        m.width > 0
    "#,
    );
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn ctx_save_restore() {
    let result = eval_full(
        r#"
        var c = document.createElement('canvas');
        var ctx = c.getContext('2d');
        ctx.save();
        ctx.restore();
        'ok'
    "#,
    );
    assert_eq!(result, JsValue::String("ok".to_string()));
}

#[test]
fn ctx_transforms() {
    let result = eval_full(
        r#"
        var c = document.createElement('canvas');
        var ctx = c.getContext('2d');
        ctx.translate(10, 20);
        ctx.rotate(0.5);
        ctx.scale(2, 2);
        var m = ctx.getTransform();
        m.is2D
    "#,
    );
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn ctx_create_image_data() {
    let result = eval_full(
        r#"
        var c = document.createElement('canvas');
        var ctx = c.getContext('2d');
        var img = ctx.createImageData(2, 2);
        img.width
    "#,
    );
    assert_eq!(result, JsValue::Number(2.0));
}

#[test]
fn ctx_get_image_data() {
    let result = eval_full(
        r#"
        var c = document.createElement('canvas');
        var ctx = c.getContext('2d');
        var img = ctx.getImageData(0, 0, 4, 4);
        img.data.length
    "#,
    );
    assert_eq!(result, JsValue::Number(64.0)); // 4*4*4
}

#[test]
fn ctx_gradient() {
    let result = eval_full(
        r#"
        var c = document.createElement('canvas');
        var ctx = c.getContext('2d');
        var g = ctx.createLinearGradient(0, 0, 200, 0);
        g.__type__
    "#,
    );
    assert_eq!(result, JsValue::String("CanvasGradient".to_string()));
}

#[test]
fn ctx_fill_text() {
    let result = eval_full(
        r#"
        var c = document.createElement('canvas');
        var ctx = c.getContext('2d');
        ctx.font = '20px Arial';
        ctx.fillText('Hello', 10, 50);
        ctx.font
    "#,
    );
    assert_eq!(result, JsValue::String("20px Arial".to_string()));
}

// ── Path2D ───────────────────────────────────────────────────────────────────

#[test]
fn path2d_construct() {
    let result = eval_full(
        r#"
        var p = new Path2D();
        p.__type__
    "#,
    );
    assert_eq!(result, JsValue::String("Path2D".to_string()));
}

#[test]
fn path2d_add_commands() {
    let result = eval_full(
        r#"
        var p = new Path2D();
        p.moveTo(0, 0);
        p.lineTo(100, 100);
        p.__commands__.length
    "#,
    );
    assert_eq!(result, JsValue::Number(2.0));
}

// ── OffscreenCanvas ──────────────────────────────────────────────────────────

#[test]
fn offscreen_canvas_construct() {
    let result = eval_full(
        r#"
        var oc = new OffscreenCanvas(800, 600);
        oc.width
    "#,
    );
    assert_eq!(result, JsValue::Number(800.0));
}

#[test]
fn offscreen_canvas_get_context() {
    let result = eval_full(
        r#"
        var oc = new OffscreenCanvas(100, 100);
        var ctx = oc.getContext('2d');
        ctx.__type__
    "#,
    );
    assert_eq!(
        result,
        JsValue::String("CanvasRenderingContext2D".to_string())
    );
}

#[test]
fn offscreen_canvas_convert_to_blob() {
    let result = eval_full(
        r#"
        var oc = new OffscreenCanvas(10, 10);
        var blob = oc.convertToBlob();
        blob.__type__
    "#,
    );
    assert_eq!(result, JsValue::String("Promise".to_string()));
}

// ── createImageBitmap ────────────────────────────────────────────────────────

#[test]
fn create_image_bitmap_global() {
    let result = eval_full(
        r#"
        var bmp = createImageBitmap({});
        bmp.__type__
    "#,
    );
    assert_eq!(result, JsValue::String("Promise".to_string()));
}
