//! V.E.L.O.C.I.T.Y. IDE — Headless remote frame server
//!
//! Renders VelocityApp to PNG via software rasterization and serves frames
//! over HTTP. A browser thin-client long-polls `/frame` for new frames only
//! (frame coalescing) and posts input (pointer, wheel, keys, paste) to
//! `/input`. Server-side clipboard copies are relayed via `/clipboard`.

use clap::Parser;
use crossbeam_channel::{bounded, Receiver, Sender};
use egui::{
    Context, CursorIcon, Event, MouseWheelUnit, OutputCommand, Pos2, RawInput, Rect, TouchPhase,
    Vec2,
};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Condvar, Mutex};
use tiny_http::{Header, Method, Response, Server};

use velocity_mcp::agent::{AgentToUiMessage, UiToAgentMessage};

// ─── CLI ───────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "velocity_serve",
    about = "V.E.L.O.C.I.T.Y. IDE — Headless frame server"
)]
struct Cli {
    /// HTTP listen port
    #[arg(long, default_value = "7331")]
    port: u16,

    /// Authentication token required on all API endpoints
    #[arg(long)]
    token: String,

    /// Workspace root directory
    #[arg(long)]
    workspace: std::path::PathBuf,

    /// Framebuffer width in pixels
    #[arg(long, default_value = "1280")]
    width: u32,

    /// Framebuffer height in pixels
    #[arg(long, default_value = "768")]
    height: u32,
}

// ─── Input protocol (browser → server) ─────────────────────────────────────

#[derive(Serialize, Deserialize, Clone)]
pub struct InputEvent {
    /// "pointer_move" | "pointer_down" | "pointer_up" | "key" | "text" | "wheel"
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub button: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Wheel delta (pixels) — only for "wheel" events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dx: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dy: Option<f32>,
    #[serde(default)]
    pub repeat: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub alt: bool,
}

/// POST /resize body: tells the server the browser's desired egui viewport size.
#[derive(Deserialize)]
struct ResizeBody {
    width: u32,
    height: u32,
}

fn input_to_egui_events(events: Vec<InputEvent>) -> Vec<Event> {
    let mut out = Vec::with_capacity(events.len());
    for e in events {
        match e.kind.as_str() {
            "pointer_move" => {
                if let (Some(x), Some(y)) = (e.x, e.y) {
                    out.push(Event::PointerMoved(Pos2::new(x, y)));
                }
            }
            "pointer_down" | "pointer_up" => {
                if let (Some(x), Some(y)) = (e.x, e.y) {
                    let btn = match e.button.unwrap_or(0) {
                        1 => egui::PointerButton::Secondary,
                        2 => egui::PointerButton::Middle,
                        _ => egui::PointerButton::Primary,
                    };
                    out.push(Event::PointerButton {
                        pos: Pos2::new(x, y),
                        button: btn,
                        pressed: e.kind == "pointer_down",
                        modifiers: build_modifiers(&e),
                    });
                }
            }
            "key" => {
                if let Some(ref k) = e.key {
                    if let Some(key) = str_to_key(k) {
                        out.push(Event::Key {
                            key,
                            physical_key: None,
                            pressed: true,
                            repeat: e.repeat,
                            modifiers: build_modifiers(&e),
                        });
                    }
                }
            }
            "text" => {
                if let Some(ref t) = e.text {
                    for ch in t.chars() {
                        out.push(Event::Text(ch.to_string()));
                    }
                }
            }
            "wheel" => {
                out.push(Event::MouseWheel {
                    unit: MouseWheelUnit::Point,
                    delta: Vec2::new(e.dx.unwrap_or(0.0), e.dy.unwrap_or(0.0)),
                    phase: TouchPhase::Move,
                    modifiers: build_modifiers(&e),
                });
            }
            _ => {}
        }
    }
    out
}

fn build_modifiers(e: &InputEvent) -> egui::Modifiers {
    egui::Modifiers {
        alt: e.alt,
        ctrl: e.ctrl,
        shift: e.shift,
        ..Default::default()
    }
}

/// Map a browser `KeyboardEvent.key` value onto an egui key.
fn str_to_key(k: &str) -> Option<egui::Key> {
    Some(match k {
        "Enter" | "NumpadEnter" => egui::Key::Enter,
        "Escape" => egui::Key::Escape,
        "Backspace" => egui::Key::Backspace,
        "Tab" => egui::Key::Tab,
        " " => egui::Key::Space,
        "ArrowUp" => egui::Key::ArrowUp,
        "ArrowDown" => egui::Key::ArrowDown,
        "ArrowLeft" => egui::Key::ArrowLeft,
        "ArrowRight" => egui::Key::ArrowRight,
        "Delete" => egui::Key::Delete,
        "Insert" => egui::Key::Insert,
        "Home" => egui::Key::Home,
        "End" => egui::Key::End,
        "PageUp" => egui::Key::PageUp,
        "PageDown" => egui::Key::PageDown,
        "-" | "_" => egui::Key::Minus,
        "=" | "+" => egui::Key::Equals,
        "[" => egui::Key::OpenBracket,
        "]" => egui::Key::CloseBracket,
        "\\" | "|" => egui::Key::Backslash,
        ";" | ":" => egui::Key::Semicolon,
        "'" | "\"" => egui::Key::Quote,
        "," | "<" => egui::Key::Comma,
        "." | ">" => egui::Key::Period,
        "/" | "?" => egui::Key::Slash,
        "`" | "~" => egui::Key::Backtick,
        "F1" => egui::Key::F1,
        "F2" => egui::Key::F2,
        "F3" => egui::Key::F3,
        "F4" => egui::Key::F4,
        "F5" => egui::Key::F5,
        "F6" => egui::Key::F6,
        "F7" => egui::Key::F7,
        "F8" => egui::Key::F8,
        "F9" => egui::Key::F9,
        "F10" => egui::Key::F10,
        "F11" => egui::Key::F11,
        "F12" => egui::Key::F12,
        "a" => egui::Key::A,
        "b" => egui::Key::B,
        "c" => egui::Key::C,
        "d" => egui::Key::D,
        "e" => egui::Key::E,
        "f" => egui::Key::F,
        "g" => egui::Key::G,
        "h" => egui::Key::H,
        "i" => egui::Key::I,
        "j" => egui::Key::J,
        "k" => egui::Key::K,
        "l" => egui::Key::L,
        "m" => egui::Key::M,
        "n" => egui::Key::N,
        "o" => egui::Key::O,
        "p" => egui::Key::P,
        "q" => egui::Key::Q,
        "r" => egui::Key::R,
        "s" => egui::Key::S,
        "t" => egui::Key::T,
        "u" => egui::Key::U,
        "v" => egui::Key::V,
        "w" => egui::Key::W,
        "x" => egui::Key::X,
        "y" => egui::Key::Y,
        "z" => egui::Key::Z,
        "0" => egui::Key::Num0,
        "1" => egui::Key::Num1,
        "2" => egui::Key::Num2,
        "3" => egui::Key::Num3,
        "4" => egui::Key::Num4,
        "5" => egui::Key::Num5,
        "6" => egui::Key::Num6,
        "7" => egui::Key::Num7,
        "8" => egui::Key::Num8,
        "9" => egui::Key::Num9,
        _ => return None,
    })
}

// ─── Cursor + query helpers (pure, testable) ───────────────────────────────

/// Map an egui cursor icon onto its CSS `cursor` property name.
fn cursor_css(icon: CursorIcon) -> &'static str {
    match icon {
        CursorIcon::Default => "default",
        CursorIcon::None => "none",
        CursorIcon::ContextMenu => "context-menu",
        CursorIcon::Progress => "progress",
        CursorIcon::Wait => "wait",
        CursorIcon::Help => "help",
        CursorIcon::PointingHand => "pointer",
        CursorIcon::Grab => "grab",
        CursorIcon::Grabbing => "grabbing",
        CursorIcon::Crosshair => "crosshair",
        CursorIcon::Text => "text",
        CursorIcon::Move => "move",
        CursorIcon::ResizeVertical => "ns-resize",
        CursorIcon::ResizeHorizontal => "ew-resize",
        CursorIcon::ResizeColumn => "col-resize",
        CursorIcon::ResizeRow => "row-resize",
        CursorIcon::ResizeNorth => "n-resize",
        CursorIcon::ResizeNorthEast => "ne-resize",
        CursorIcon::ResizeEast => "e-resize",
        CursorIcon::ResizeSouthEast => "se-resize",
        CursorIcon::ResizeSouth => "s-resize",
        CursorIcon::ResizeSouthWest => "sw-resize",
        CursorIcon::ResizeWest => "w-resize",
        CursorIcon::ResizeNorthWest => "nw-resize",
        CursorIcon::ResizeNwSe => "nwse-resize",
        CursorIcon::ResizeNeSw => "nesw-resize",
        CursorIcon::AllScroll => "all-scroll",
        CursorIcon::ZoomIn => "zoom-in",
        CursorIcon::ZoomOut => "zoom-out",
        _ => "default",
    }
}

/// Extract the `since` frame-sequence number from a request URL like
/// `/frame?since=42`. Returns 0 when absent or unparseable (fetch anything).
fn parse_since(url: &str) -> u64 {
    let Some(query) = url.split_once('?').map(|(_, q)| q) else {
        return 0;
    };
    for pair in query.split('&') {
        if let Some(v) = pair.strip_prefix("since=") {
            if let Ok(n) = v.parse::<u64>() {
                return n;
            }
        }
    }
    0
}

// ─── Frame hub (shared latest frame + sequence + cursor + clipboard) ────────

struct FrameHub {
    /// Latest PNG, empty until the first frame is rendered.
    png: Vec<u8>,
    /// Monotonically increasing; incremented only when pixels changed.
    seq: u64,
    /// CSS cursor name for the latest frame.
    cursor: &'static str,
    /// Most recent server-side clipboard (copy) content.
    clipboard: String,
    /// Frame dimensions (updated on resize).
    width: u32,
    height: u32,
}

impl FrameHub {
    fn new() -> Self {
        Self {
            png: Vec::new(),
            seq: 0,
            cursor: "default",
            clipboard: String::new(),
            width: 0,
            height: 0,
        }
    }
}

type Hub = (Mutex<FrameHub>, Condvar);

// ─── Software rasterizer (proven in v64a spike) ────────────────────────────

fn rasterize_tri(
    buf: &mut [u8],
    w: u32,
    h: u32,
    v0: (f32, f32, [u8; 4]),
    v1: (f32, f32, [u8; 4]),
    v2: (f32, f32, [u8; 4]),
    clip: (f32, f32, f32, f32),
) {
    let (cx0, cy0, cx1, cy1) = clip;
    let min_x = (v0.0.min(v1.0).min(v2.0)).max(0.0).max(cx0) as i32;
    let max_x = (v0.0.max(v1.0).max(v2.0)).min(w as f32 - 1.0).min(cx1) as i32;
    let min_y = (v0.1.min(v1.1).min(v2.1)).max(0.0).max(cy0) as i32;
    let max_y = (v0.1.max(v1.1).max(v2.1)).min(h as f32 - 1.0).min(cy1) as i32;

    let area = (v1.0 - v0.0) * (v2.1 - v0.1) - (v2.0 - v0.0) * (v1.1 - v0.1);
    if area.abs() < 1e-6 {
        return;
    }
    let inv_area = 1.0 / area;

    for py in min_y..=max_y {
        for px in min_x..=max_x {
            let cx = px as f32 + 0.5;
            let cy = py as f32 + 0.5;
            let b0 = ((v1.0 - cx) * (v2.1 - cy) - (v2.0 - cx) * (v1.1 - cy)) * inv_area;
            let b1 = ((v2.0 - cx) * (v0.1 - cy) - (v0.0 - cx) * (v2.1 - cy)) * inv_area;
            let b2 = 1.0 - b0 - b1;
            if b0 < 0.0 || b1 < 0.0 || b2 < 0.0 {
                continue;
            }
            let r = (b0 * v0.2[0] as f32 + b1 * v1.2[0] as f32 + b2 * v2.2[0] as f32) as u8;
            let g = (b0 * v0.2[1] as f32 + b1 * v1.2[1] as f32 + b2 * v2.2[1] as f32) as u8;
            let b = (b0 * v0.2[2] as f32 + b1 * v1.2[2] as f32 + b2 * v2.2[2] as f32) as u8;
            let a = (b0 * v0.2[3] as f32 + b1 * v1.2[3] as f32 + b2 * v2.2[3] as f32) as u8;
            if a == 0 {
                continue;
            }
            let idx = (py as usize * w as usize + px as usize) * 4;
            let dst = &mut buf[idx..idx + 4];
            let sa = a as u16;
            let da = 255 - sa;
            dst[0] = ((r as u16 * sa + dst[0] as u16 * da) / 255) as u8;
            dst[1] = ((g as u16 * sa + dst[1] as u16 * da) / 255) as u8;
            dst[2] = ((b as u16 * sa + dst[2] as u16 * da) / 255) as u8;
            dst[3] = a.max(dst[3]);
        }
    }
}

/// Rasterize one egui frame into an RGBA buffer. Returns the buffer plus the
/// frame's cursor icon and any clipboard text the app copied this tick.
fn render_frame_to_rgba(
    ctx: &Context,
    app: &mut velocity_mcp::editor::app::VelocityApp,
    width: u32,
    height: u32,
    events: Vec<Event>,
    time: f64,
) -> (Vec<u8>, CursorIcon, String) {
    let raw = RawInput {
        time: Some(time),
        screen_rect: Some(Rect::from_min_size(
            Pos2::ZERO,
            Vec2::new(width as f32, height as f32),
        )),
        events,
        ..Default::default()
    };

    let full_output = ctx.run_ui(raw, |ui| {
        app.render_frame(ui);
    });

    let clipped_primitives = ctx.tessellate(full_output.shapes, full_output.pixels_per_point);

    // Allocate RGBA buffer with dark IDE background
    let mut buf = vec![0u8; (width as usize * height as usize) * 4];
    for chunk in buf.chunks_mut(4) {
        chunk[0] = 30;
        chunk[1] = 30;
        chunk[2] = 40;
        chunk[3] = 255;
    }

    for cp in &clipped_primitives {
        let clip = (
            cp.clip_rect.min.x,
            cp.clip_rect.min.y,
            cp.clip_rect.max.x,
            cp.clip_rect.max.y,
        );
        if let epaint::Primitive::Mesh(mesh) = &cp.primitive {
            for tri in mesh.indices.as_chunks::<3>().0 {
                let verts: Vec<(f32, f32, [u8; 4])> = tri
                    .iter()
                    .map(|&i| {
                        let v = &mesh.vertices[i as usize];
                        (v.pos.x, v.pos.y, v.color.to_array())
                    })
                    .collect();
                rasterize_tri(&mut buf, width, height, verts[0], verts[1], verts[2], clip);
            }
        }
    }

    let copied_text =
        full_output
            .platform_output
            .commands
            .iter()
            .fold(String::new(), |mut acc, cmd| {
                if let OutputCommand::CopyText(text) = cmd {
                    acc.push_str(text);
                }
                acc
            });
    (buf, full_output.platform_output.cursor_icon, copied_text)
}

fn encode_png(buf: &[u8], width: u32, height: u32) -> Vec<u8> {
    let img = RgbaImage::from_raw(width, height, buf.to_vec()).expect("buffer size");
    let mut png_bytes = Vec::new();
    {
        #[allow(deprecated)]
        let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
        #[allow(deprecated)]
        encoder
            .encode(img.as_raw(), width, height, image::ColorType::Rgba8)
            .expect("png encode");
    }
    png_bytes
}

// ─── Thin-client HTML ──────────────────────────────────────────────────────

fn client_html(width: u32, height: u32, token: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>Velocity IDE — Remote</title>
<meta name="viewport" content="width=device-width,initial-scale=1,user-scalable=no">
<style>*{{margin:0;padding:0;box-sizing:border-box}}
html,body{{width:100%;height:100%;overflow:hidden;background:#1e1e2e}}
canvas{{display:block;width:100%;height:100%;object-fit:contain;image-rendering:pixelated;outline:none}}
#status{{position:fixed;top:4px;right:8px;color:#888;font:11px monospace;z-index:9}}</style>
</head><body>
<canvas id="c" width="{width}" height="{height}" tabindex="0"></canvas>
<div id="status">connecting…</div>
<script>
const TOKEN = "{token}";
let W = {width}, H = {height};
const VIEWONLY = location.hash.includes('viewonly');
const canvas = document.getElementById('c');
const ctx2d = canvas.getContext('2d');
const status = document.getElementById('status');
let since = 0, frames = 0, lastStat = Date.now();

// Scale browser CSS coordinates → egui logical coordinates
function toEgui(cx, cy) {{
    const r = canvas.getBoundingClientRect();
    return [ (cx - r.left) * W / r.width, (cy - r.top) * H / r.height ];
}}

function authHeaders() {{
    return {{'Authorization': 'Bearer ' + TOKEN, 'Content-Type': 'application/json'}};
}}

// ── Frame long-poll: server holds until a NEW frame exists (coalescing) ──
async function pollLoop() {{
    let img = new Image();
    while (true) {{
        try {{
            const r = await fetch('/frame?since=' + since, {{headers: {{'Authorization': 'Bearer ' + TOKEN}}}});
            if (r.status === 304) continue;           // nothing new, poll again
            if (!r.ok) {{ status.textContent = 'HTTP ' + r.status; await new Promise(s => setTimeout(s, 1000)); continue; }}
            const seq = +(r.headers.get('X-Frame-Seq') || 0);
            const cur = r.headers.get('X-Cursor') || 'default';
            canvas.style.cursor = cur;
            // Adapt to server viewport resize
            const fw = +(r.headers.get('X-Frame-Width') || 0);
            const fh = +(r.headers.get('X-Frame-Height') || 0);
            if (fw > 0 && fh > 0 && (fw !== W || fh !== H)) {{
                W = fw; H = fh;
                canvas.width = W; canvas.height = H;
            }}
            const blob = await r.blob();
            const url = URL.createObjectURL(blob);
            await new Promise(res => {{ img.onload = () => {{ ctx2d.drawImage(img, 0, 0, W, H); URL.revokeObjectURL(url); res(); }}; img.onerror = res; img.src = url; }});
            if (seq > since) since = seq;
            frames++;
            const now = Date.now();
            if (now - lastStat > 1000) {{ status.textContent = (VIEWONLY?'view-only ':'') + 'fps ' + Math.round(frames*1000/(now-lastStat)) + ' frame#' + since; frames = 0; lastStat = now; }}
        }} catch(e) {{ status.textContent = 'disconnected'; await new Promise(s => setTimeout(s, 1000)); }}
    }}
}}
pollLoop();

// ── Input forwarding (batched per animation frame) ──
let pending = [];
async function flush() {{
    if (!pending.length || VIEWONLY) {{ pending = []; return; }}
    const batch = pending; pending = [];
    try {{ await fetch('/input', {{method:'POST', headers: authHeaders(), body: JSON.stringify(batch)}}); }}
    catch(e) {{ }}
}}
setInterval(flush, 16);
function sendInput(events) {{ pending = pending.concat(events); }}

// ── Mouse input (coordinate-scaled) ──
canvas.addEventListener('mousemove', e => {{
    const [x, y] = toEgui(e.clientX, e.clientY);
    sendInput([{{kind:'pointer_move', x, y}}]);
}});
canvas.addEventListener('mousedown', e => {{
    canvas.focus();
    const [x, y] = toEgui(e.clientX, e.clientY);
    sendInput([{{kind:'pointer_down', x, y, button:e.button, shift:e.shiftKey, ctrl:e.ctrlKey, alt:e.altKey}}]);
}});
canvas.addEventListener('mouseup', e => {{
    const [x, y] = toEgui(e.clientX, e.clientY);
    sendInput([{{kind:'pointer_up', x, y, button:e.button, shift:e.shiftKey, ctrl:e.ctrlKey, alt:e.altKey}}]);
}});
canvas.addEventListener('contextmenu', e => e.preventDefault());

// ── Wheel scrolling (deltaMode-normalized, coordinate-scaled) ──
canvas.addEventListener('wheel', e => {{
    e.preventDefault();
    let dx = e.deltaX, dy = e.deltaY;
    if (e.deltaMode === 1) {{ dx *= 20; dy *= 20; }}
    else if (e.deltaMode === 2) {{ dx *= W; dy *= H; }}
    const [x, y] = toEgui(e.clientX, e.clientY);
    sendInput([{{kind:'wheel', x, y, dx, dy, shift:e.shiftKey, ctrl:e.ctrlKey, alt:e.altKey}}]);
}}, {{passive:false}});

// ── Touch input (single-finger → pointer, two-finger → scroll) ──
let lastTouchDist = 0, lastTouchY = 0;
function touchDist(ts) {{
    if (ts.length < 2) return 0;
    const dx = ts[0].clientX - ts[1].clientX, dy = ts[0].clientY - ts[1].clientY;
    return Math.sqrt(dx*dx + dy*dy);
}}
canvas.addEventListener('touchstart', e => {{
    e.preventDefault(); canvas.focus();
    const ts = e.touches;
    if (ts.length === 1) {{
        const [x, y] = toEgui(ts[0].clientX, ts[0].clientY);
        sendInput([{{kind:'pointer_down', x, y, button:0}}]);
    }}
    lastTouchDist = touchDist(ts);
    lastTouchY = ts[0] ? ts[0].clientY : 0;
}}, {{passive:false}});
canvas.addEventListener('touchmove', e => {{
    e.preventDefault();
    const ts = e.touches;
    if (ts.length === 1) {{
        const [x, y] = toEgui(ts[0].clientX, ts[0].clientY);
        sendInput([{{kind:'pointer_move', x, y}}]);
    }} else if (ts.length === 2) {{
        // Two-finger vertical pinch → wheel
        const d = touchDist(ts);
        const dy = lastTouchY - ts[0].clientY;
        const [x, y] = toEgui(ts[0].clientX, ts[0].clientY);
        sendInput([{{kind:'wheel', x, y, dx:0, dy:dy}}]);
        lastTouchY = ts[0].clientY;
    }}
}}, {{passive:false}});
canvas.addEventListener('touchend', e => {{
    e.preventDefault();
    const ct = e.changedTouches;
    if (ct.length >= 1) {{
        const [x, y] = toEgui(ct[0].clientX, ct[0].clientY);
        sendInput([{{kind:'pointer_up', x, y, button:0}}]);
    }}
}}, {{passive:false}});

// ── Keyboard ──
document.addEventListener('keydown', e => {{
    if (e.key === 'F5' || e.key === 'F12' || (e.ctrlKey && ['r','R','l','L','j','J'].includes(e.key))) return;
    if (e.key.length === 1) sendInput([{{kind:'text', text:e.key}}]);
    else sendInput([{{kind:'key', key:e.key, repeat:e.repeat, shift:e.shiftKey, ctrl:e.ctrlKey, alt:e.altKey}}]);
    e.preventDefault();
}});
document.addEventListener('keyup', e => e.preventDefault());

// Paste → forwarded as text (works without clipboard permission)
document.addEventListener('paste', e => {{
    const t = (e.clipboardData || window.clipboardData).getData('text');
    if (t) sendInput([{{kind:'text', text:t}}]);
    e.preventDefault();
}});

// ── Clipboard relay: server-side copies → browser clipboard (best effort) ──
let lastClip = '';
setInterval(async () => {{
    try {{
        const r = await fetch('/clipboard', {{headers: {{'Authorization': 'Bearer ' + TOKEN}}}});
        if (!r.ok) return;
        const t = await r.text();
        if (t && t !== lastClip) {{
            lastClip = t;
            if (navigator.clipboard && navigator.clipboard.writeText) {{
                try {{ await navigator.clipboard.writeText(t); }} catch(e) {{}}
            }}
        }}
    }} catch(e) {{ }}
}}, 800);

// ── Viewport resize: debounced notify server of new canvas logical size ──
if (!VIEWONLY) {{
    let resizeTimer = null;
    function notifyResize() {{
        const rect = canvas.getBoundingClientRect();
        const dpr = window.devicePixelRatio || 1;
        const nw = Math.round(rect.width * dpr);
        const nh = Math.round(rect.height * dpr);
        if (nw === W && nh === H) return;
        W = nw; H = nh;
        canvas.width = W; canvas.height = H;
        fetch('/resize', {{method:'POST', headers: authHeaders(), body: JSON.stringify({{width:W,height:H}})}}).catch(()=>{{}});
    }}
    window.addEventListener('resize', () => {{
        clearTimeout(resizeTimer);
        resizeTimer = setTimeout(notifyResize, 200);
    }});
}}
</script></body></html>"#,
        token = token,
        width = width,
        height = height
    )
}

// ─── Main ──────────────────────────────────────────────────────────────────

fn main() {
    env_logger::init();
    let cli = Cli::parse();

    println!("[velocity-serve] Starting headless IDE frame server");
    println!("  Workspace : {}", cli.workspace.display());
    println!("  Port      : {}", cli.port);
    println!("  Size      : {}x{}", cli.width, cli.height);

    // ── Infrastructure (mirrors velocity-ide-gui setup) ──
    let (ui_tx, agent_rx): (Sender<AgentToUiMessage>, Receiver<AgentToUiMessage>) =
        crossbeam_channel::unbounded();
    let (agent_tx, ui_rx): (Sender<UiToAgentMessage>, Receiver<UiToAgentMessage>) =
        crossbeam_channel::unbounded();

    std::fs::create_dir_all(&cli.workspace).expect("create workspace");
    std::fs::create_dir_all(cli.workspace.join(".velocity")).expect("create .velocity");

    let mediator = Arc::new(velocity_mcp::automation::MediatorArena::new());

    // Spawn agent thread
    let ws_agent = cli.workspace.clone();
    std::thread::spawn(move || {
        velocity_mcp::agent::run_agent_thread(ws_agent, ui_rx, ui_tx);
    });

    // ── egui Context + VelocityApp ──
    let egui_ctx = Context::default();

    let mut app = velocity_mcp::editor::app::VelocityApp::new_headless(
        &egui_ctx,
        cli.workspace.clone(),
        agent_tx,
        agent_rx,
        "Headless".to_string(),
        mediator.clone(),
    );

    // ── Shared state ──
    let hub: Arc<Hub> = Arc::new((Mutex::new(FrameHub::new()), Condvar::new()));
    let (input_tx, input_rx): (Sender<Vec<InputEvent>>, Receiver<Vec<InputEvent>>) = bounded(256);
    let viewport: Arc<Mutex<(u32, u32)>> = Arc::new(Mutex::new((cli.width, cli.height)));

    // ── HTTP server thread ──
    let frame_for_http = hub.clone();
    let input_for_http = input_tx.clone();
    let viewport_for_http = viewport.clone();
    let token = cli.token.clone();
    let port = cli.port;
    let cli_width = cli.width;
    let cli_height = cli.height;

    std::thread::spawn(move || {
        let addr = format!("0.0.0.0:{}", port);
        let server = Server::http(&addr).expect("bind HTTP");
        println!("[velocity-serve] HTTP listening on {}", addr);

        for mut request in server.incoming_requests() {
            let url = request.url().to_string();
            let method = request.method().clone();

            // GET / — serve thin-client HTML (no auth)
            if method == Method::Get && (url == "/" || url == "/index.html") {
                let html = client_html(cli_width, cli_height, &token);
                let hdr = Header::from_bytes(&b"Content-Type"[..], &b"text/html"[..]).unwrap();
                let resp = Response::from_string(html).with_header(hdr);
                let _ = request.respond(resp);
                continue;
            }

            // All other endpoints require Bearer token
            let auth_ok = request.headers().iter().any(|h| {
                h.field.to_string().eq_ignore_ascii_case("authorization")
                    && h.value
                        .to_string()
                        .starts_with(&format!("Bearer {}", token))
            });
            if !auth_ok {
                let resp = Response::from_string("Unauthorized").with_status_code(401);
                let _ = request.respond(resp);
                continue;
            }

            match (method, url.split_once('?').map_or(url.as_str(), |(p, _)| p)) {
                (Method::Get, "/frame") => {
                    // Long-poll: hold until a frame newer than `since` exists,
                    // then serve it with sequence + cursor headers. Answers
                    // 304 after ~1.2s when nothing changed (frame coalescing).
                    let since = parse_since(&url);
                    let (lock, cvar) = &*frame_for_http;
                    let mut sf = lock.lock().unwrap();
                    let start = std::time::Instant::now();
                    while sf.seq <= since
                        && start.elapsed() < std::time::Duration::from_millis(1200)
                    {
                        let (guard, _timeout) = cvar
                            .wait_timeout(sf, std::time::Duration::from_millis(200))
                            .unwrap();
                        sf = guard;
                    }
                    if sf.seq <= since {
                        let _ = request.respond(Response::empty(304));
                    } else {
                        let png = sf.png.clone();
                        let seq = sf.seq;
                        let cursor = sf.cursor;
                        let fw = sf.width;
                        let fh = sf.height;
                        drop(sf);
                        let ct =
                            Header::from_bytes(&b"Content-Type"[..], &b"image/png"[..]).unwrap();
                        let seq_h =
                            Header::from_bytes(&b"X-Frame-Seq"[..], seq.to_string().as_bytes())
                                .unwrap();
                        let cur_h =
                            Header::from_bytes(&b"X-Cursor"[..], cursor.as_bytes()).unwrap();
                        let fw_h =
                            Header::from_bytes(&b"X-Frame-Width"[..], fw.to_string().as_bytes())
                                .unwrap();
                        let fh_h =
                            Header::from_bytes(&b"X-Frame-Height"[..], fh.to_string().as_bytes())
                                .unwrap();
                        let resp = Response::from_data(png)
                            .with_header(ct)
                            .with_header(seq_h)
                            .with_header(cur_h)
                            .with_header(fw_h)
                            .with_header(fh_h);
                        let _ = request.respond(resp);
                    }
                }
                (Method::Get, "/clipboard") => {
                    let text = frame_for_http.0.lock().unwrap().clipboard.clone();
                    let hdr =
                        Header::from_bytes(&b"Content-Type"[..], &b"text/plain; charset=utf-8"[..])
                            .unwrap();
                    let resp = Response::from_string(text).with_header(hdr);
                    let _ = request.respond(resp);
                }
                (Method::Post, "/input") => {
                    let mut body = String::new();
                    request.as_reader().read_to_string(&mut body).ok();
                    if let Ok(events) = serde_json::from_str::<Vec<InputEvent>>(&body) {
                        // Non-blocking: drop input if the frame loop is behind
                        // rather than stalling the HTTP thread.
                        let _ = input_for_http.try_send(events);
                    }
                    let _ = request.respond(Response::from_string("ok"));
                }
                (Method::Post, "/resize") => {
                    let mut body = String::new();
                    request.as_reader().read_to_string(&mut body).ok();
                    if let Ok(r) = serde_json::from_str::<ResizeBody>(&body) {
                        let w = r.width.clamp(320, 7680);
                        let h = r.height.clamp(200, 4320);
                        *viewport_for_http.lock().unwrap() = (w, h);
                    }
                    let _ = request.respond(Response::from_string("ok"));
                }
                _ => {
                    let _ =
                        request.respond(Response::from_string("Not Found").with_status_code(404));
                }
            }
        }
    });

    // ── Frame loop (main thread) ──
    let mut frame_time: f64 = 0.0;
    let frame_interval = 1.0 / 30.0; // 30 fps target
    let mut prev_rgba: Option<Vec<u8>> = None;
    let mut cur_w = cli_width;
    let mut cur_h = cli_height;

    println!("[velocity-serve] Frame loop running (30 fps, coalesced)");

    loop {
        // Collect pending input events (drain all, non-blocking)
        let mut events: Vec<Event> = Vec::new();
        for batch in input_rx.try_iter() {
            events.extend(input_to_egui_events(batch));
        }

        // Check for viewport resize from browser
        let (new_w, new_h) = *viewport.lock().unwrap();
        if new_w != cur_w || new_h != cur_h {
            cur_w = new_w;
            cur_h = new_h;
            prev_rgba = None; // force new frame on resize
        }

        frame_time += frame_interval;

        let (rgba, cursor_icon, copied) =
            render_frame_to_rgba(&egui_ctx, &mut app, cur_w, cur_h, events, frame_time);

        let changed = prev_rgba.as_ref().is_none_or(|p| *p != rgba);
        let (lock, cvar) = &*hub;
        {
            let mut sf = lock.lock().unwrap();
            sf.cursor = cursor_css(cursor_icon);
            if !copied.is_empty() {
                sf.clipboard = copied;
            }
            if changed {
                sf.png = encode_png(&rgba, cur_w, cur_h);
                sf.width = cur_w;
                sf.height = cur_h;
                sf.seq += 1;
                prev_rgba = Some(rgba);
                cvar.notify_all();
            }
        }

        // Sleep until next frame tick
        std::thread::sleep(std::time::Duration::from_millis(33));
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(kind: &str) -> InputEvent {
        InputEvent {
            kind: kind.to_string(),
            x: None,
            y: None,
            button: None,
            key: None,
            text: None,
            dx: None,
            dy: None,
            repeat: false,
            shift: false,
            ctrl: false,
            alt: false,
        }
    }

    #[test]
    fn wheel_event_becomes_mouse_pixel_scroll() {
        let mut e = ev("wheel");
        e.dx = Some(0.0);
        e.dy = Some(-120.0);
        e.shift = true;
        let out = input_to_egui_events(vec![e]);
        assert_eq!(out.len(), 1);
        match &out[0] {
            Event::MouseWheel {
                unit,
                delta,
                modifiers,
                ..
            } => {
                assert!(matches!(unit, MouseWheelUnit::Point));
                assert_eq!(delta.y, -120.0);
                assert!(modifiers.shift);
            }
            other => panic!("expected MouseWheel, got {:?}", other),
        }
    }

    #[test]
    fn key_mapping_covers_space_function_and_punct() {
        assert_eq!(str_to_key(" "), Some(egui::Key::Space));
        assert_eq!(str_to_key("F2"), Some(egui::Key::F2));
        assert_eq!(str_to_key("F12"), Some(egui::Key::F12));
        assert_eq!(str_to_key("-"), Some(egui::Key::Minus));
        assert_eq!(str_to_key("/"), Some(egui::Key::Slash));
        assert_eq!(str_to_key("Insert"), Some(egui::Key::Insert));
        assert_eq!(str_to_key("NonexistentKey"), None);
    }

    #[test]
    fn key_repeat_flag_forwarded() {
        let mut e = ev("key");
        e.key = Some("ArrowDown".to_string());
        e.repeat = true;
        let out = input_to_egui_events(vec![e]);
        match &out[0] {
            Event::Key { key, repeat, .. } => {
                assert_eq!(*key, egui::Key::ArrowDown);
                assert!(*repeat);
            }
            other => panic!("expected Key, got {:?}", other),
        }
    }

    #[test]
    fn pointer_up_down_share_shape() {
        let mut down = ev("pointer_down");
        down.x = Some(10.0);
        down.y = Some(20.0);
        let mut up = ev("pointer_up");
        up.x = Some(10.0);
        up.y = Some(20.0);
        let out = input_to_egui_events(vec![down, up]);
        let pressed: Vec<bool> = out
            .iter()
            .map(|e| match e {
                Event::PointerButton { pressed, .. } => *pressed,
                other => panic!("expected PointerButton, got {:?}", other),
            })
            .collect();
        assert_eq!(pressed, vec![true, false]);
    }

    #[test]
    fn parse_since_extracts_seq() {
        assert_eq!(parse_since("/frame?since=42"), 42);
        assert_eq!(parse_since("/frame?_=1&since=7&x=2"), 7);
        assert_eq!(parse_since("/frame"), 0);
        assert_eq!(parse_since("/frame?since=abc"), 0);
        assert_eq!(parse_since("/frame?other=9"), 0);
    }

    #[test]
    fn cursor_css_maps_icons() {
        assert_eq!(cursor_css(CursorIcon::Text), "text");
        assert_eq!(cursor_css(CursorIcon::PointingHand), "pointer");
        assert_eq!(cursor_css(CursorIcon::Default), "default");
        assert_eq!(cursor_css(CursorIcon::ResizeColumn), "col-resize");
    }

    #[test]
    fn frame_hub_starts_empty_and_unsequenced() {
        let hub = FrameHub::new();
        assert_eq!(hub.seq, 0);
        assert!(hub.png.is_empty());
        assert_eq!(hub.cursor, "default");
        assert!(hub.clipboard.is_empty());
    }

    #[test]
    fn text_event_splits_chars() {
        let mut e = ev("text");
        e.text = Some("ab".to_string());
        let out = input_to_egui_events(vec![e]);
        assert_eq!(out.len(), 2);
        assert!(matches!(&out[0], Event::Text(s) if s == "a"));
        assert!(matches!(&out[1], Event::Text(s) if s == "b"));
    }
}
