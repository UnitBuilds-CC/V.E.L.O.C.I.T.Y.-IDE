//! Zero-dependency browser viewer generator for portable NDA1 documents.
//!
//! Produces standalone HTML that parses the exact 48-byte-header NDA1 layout
//! (see [`velocity_browser::nda_portable`]) in vanilla JavaScript and renders
//! the canvas display commands, the semantic-triple graph, the provenance
//! history, and a hex byte-map — with no network access or dependencies.
//!
//! Two shapes are produced from the same viewer core:
//! * [`self_contained_html`] inlines a document as base64 so a single `.html`
//!   file is fully portable (double-click to view).
//! * [`pwa_viewer_html`] ships a file picker so one viewer opens any `.nda`.

/// Standard base64 (with padding) — avoids pulling an external crate.
pub fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((n >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// The shared vanilla-JS viewer core: `parseNda` + the four renderers. Kept as
/// a string constant so both output shapes embed byte-identical logic.
const VIEWER_JS: &str = r##"
const PRED_HISTORY = new Set(["rev:parent","rev:content_hash","rev:author_name","rev:author_email","rev:author_source","rev:timestamp","rev:message","nda:origin","nda:created"]);
function parseNda(bytes){
  const dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const magic = dv.getUint32(0, true);
  const flags = dv.getUint32(4, true);
  let merkle = "";
  for (let i = 8; i < 40; i++) merkle += bytes[i].toString(16).padStart(2, "0");
  const tripleCount = dv.getUint32(40, true);
  const commandCount = dv.getUint16(44, true);
  const poolOffset = dv.getUint16(46, true);
  const dec = new TextDecoder();
  const readStr = (rel) => {
    const p = poolOffset + rel;
    const len = dv.getUint16(p, true);
    return dec.decode(bytes.subarray(p + 2, p + 2 + len));
  };
  let off = 48;
  const triples = [];
  for (let i = 0; i < tripleCount; i++) {
    const s = dv.getUint32(off, true), p = dv.getUint32(off + 4, true), o = dv.getUint32(off + 8, true);
    triples.push([readStr(s), readStr(p), readStr(o)]);
    off += 12;
  }
  const commands = [];
  for (let i = 0; i < commandCount; i++) {
    commands.push({
      type: dv.getUint8(off),
      color: dv.getUint32(off + 1, true),
      x: dv.getUint16(off + 5, true),
      y: dv.getUint16(off + 7, true),
      w: dv.getUint16(off + 9, true),
      h: dv.getUint16(off + 11, true),
      content: readStr(dv.getUint32(off + 13, true)),
    });
    off += 18;
  }
  return { magic, flags, merkle, tripleCount, commandCount, triples, commands, bytes };
}
function rgba(c){
  const r = (c >> 24) & 255, g = (c >> 16) & 255, b = (c >> 8) & 255, a = c & 255;
  return "rgba(" + r + "," + g + "," + b + "," + (a / 255).toFixed(3) + ")";
}
const KIND = { 1: "DrawText", 2: "DrawVector", 3: "DrawRect", 4: "DrawImage" };
function esc(s){ return String(s).replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" }[c])); }

function renderCanvas(doc){
  const el = document.getElementById("canvas");
  el.innerHTML = "";
  const cv = document.createElement("canvas");
  cv.width = 800; cv.height = 480;
  const ctx = cv.getContext("2d");
  const paint = () => {
    ctx.fillStyle = "#0d1117"; ctx.fillRect(0, 0, cv.width, cv.height);
    for (const c of doc.commands) {
      ctx.fillStyle = rgba(c.color);
      ctx.strokeStyle = rgba(c.color);
      if (c.type === 3) { ctx.fillRect(c.x, c.y, c.w, c.h); }
      else if (c.type === 1) {
        ctx.font = "14px monospace";
        if (c.w > 0) { wrapText(ctx, c.content, c.x, c.y, c.w, 18); }
        else { ctx.fillText(c.content, c.x, c.y); }
      }
      else if (c.type === 4) {
        const w = c.w || 120, h = c.h || 80;
        const img = imageCache[c.content];
        if (img && img.complete && img.naturalWidth) { ctx.drawImage(img, c.x, c.y, w, h); }
        else {
          if (!img) { const ni = new Image(); ni.onload = () => renderCanvas(doc); ni.src = c.content; imageCache[c.content] = ni; }
          ctx.strokeRect(c.x, c.y, w, h); ctx.font = "11px monospace"; ctx.fillText("[image]", c.x + 4, c.y + 14);
        }
      }
      else if (c.type === 2) {
        const pts = c.content.split(";").map(p => p.split(",").map(Number)).filter(p => p.length === 2 && p.every(n => !isNaN(n)));
        if (pts.length >= 2) {
          ctx.lineWidth = c.h || 1; ctx.beginPath();
          ctx.moveTo(c.x + pts[0][0], c.y + pts[0][1]);
          for (let i = 1; i < pts.length; i++) ctx.lineTo(c.x + pts[i][0], c.y + pts[i][1]);
          ctx.stroke();
        } else if (pts.length === 1) { ctx.fillRect(c.x + pts[0][0] - 1, c.y + pts[0][1] - 1, 2, 2); }
      }
    }
  };
  paint();
  el.appendChild(cv);
}
const imageCache = {};
function wrapText(ctx, text, x, y, maxWidth, lineHeight){
  const words = String(text).split(/\s+/);
  let line = "", cy = y;
  for (const w of words) {
    const test = line ? line + " " + w : w;
    if (ctx.measureText(test).width > maxWidth && line) { ctx.fillText(line, x, cy); line = w; cy += lineHeight; }
    else { line = test; }
  }
  if (line) ctx.fillText(line, x, cy);
}
function renderGraph(doc){
  const el = document.getElementById("graph");
  const content = doc.triples.filter(t => !PRED_HISTORY.has(t[1]));
  const hist = doc.triples.filter(t => PRED_HISTORY.has(t[1]));
  let html = "<h3>Content triples (" + content.length + ")</h3><table>";
  for (const [s, p, o] of content) html += "<tr><td class='s'>" + esc(s) + "</td><td class='p'>" + esc(p) + "</td><td class='o'>" + esc(o) + "</td></tr>";
  html += "</table><h3>Provenance / meta (" + hist.length + ")</h3><table>";
  for (const [s, p, o] of hist) html += "<tr><td class='s'>" + esc(s) + "</td><td class='p'>" + esc(p) + "</td><td class='o'>" + esc(o) + "</td></tr>";
  html += "</table>";
  el.innerHTML = html;
}
function renderHistory(doc){
  const el = document.getElementById("history");
  const get = (subj, pred) => { const t = doc.triples.find(x => x[0] === subj && x[1] === pred); return t ? t[2] : ""; };
  const revs = [];
  const ids = new Set(doc.triples.filter(t => t[0].startsWith("rev:")).map(t => t[0]));
  [...ids].sort((a, b) => parseInt(a.slice(4)) - parseInt(b.slice(4))).forEach(id => {
    revs.push({ id, parent: get(id, "rev:parent"), hash: get(id, "rev:content_hash"), name: get(id, "rev:author_name"), email: get(id, "rev:author_email"), source: get(id, "rev:author_source"), ts: get(id, "rev:timestamp"), msg: get(id, "rev:message") });
  });
  const origin = get("nda:doc", "nda:origin");
  const created = get("nda:doc", "nda:created");
  let html = "<div class='origin'>Origin: <b>" + esc(origin || "unknown") + "</b>" + (created ? " · created " + esc(created) : "") + "</div>";
  if (revs.length === 0) { html += "<p>No revisions recorded.</p>"; }
  revs.forEach((r, i) => {
    let delta = "";
    if (i > 0) {
      delta = r.hash === revs[i - 1].hash
        ? " <span class='delta same'>same content as #" + (i - 1) + "</span>"
        : " <span class='delta changed'>content changed</span>";
    }
    html += "<div class='rev'><div class='revhead'>#" + i + " <span class='badge'>" + esc(r.source || "?") + "</span> <b>" + esc(r.name || "anonymous") + "</b> <span class='email'>" + esc(r.email) + "</span>" + delta + "</div>";
    html += "<div class='revmeta'>" + esc(r.ts) + (r.msg ? " — " + esc(r.msg) : "") + "</div>";
    html += "<div class='revhash'>content " + esc(r.hash.slice(0, 16)) + "… ← parent " + esc(r.parent === "genesis" ? "genesis" : r.parent.slice(0, 16) + "…") + "</div></div>";
  });
  el.innerHTML = html;
}
function renderHex(doc){
  const el = document.getElementById("hex");
  const bytes = doc.bytes;
  let html = "<pre>";
  for (let l = 0; l < bytes.length; l += 16) {
    let hexs = "", asc = "";
    for (let i = 0; i < 16; i++) {
      const idx = l + i;
      if (idx < bytes.length) { hexs += bytes[idx].toString(16).padStart(2, "0") + " "; const v = bytes[idx]; asc += (v >= 32 && v < 127) ? String.fromCharCode(v) : "."; }
      else { hexs += "   "; }
    }
    html += l.toString(16).toUpperCase().padStart(8, "0") + "  " + hexs + " |" + esc(asc) + "|\n";
  }
  html += "</pre>";
  el.innerHTML = html;
}
function showTab(name){
  ["canvas", "graph", "history", "hex"].forEach(t => {
    document.getElementById(t).style.display = (t === name) ? "block" : "none";
    const btn = document.getElementById("tab-" + t);
    if (btn) btn.classList.toggle("active", t === name);
  });
}
function loadBytes(bytes){
  try {
    const doc = parseNda(new Uint8Array(bytes));
    document.getElementById("status").textContent = "NDA1 · " + doc.tripleCount + " triples · " + doc.commandCount + " commands · merkle " + doc.merkle.slice(0, 16) + "…";
    renderCanvas(doc); renderGraph(doc); renderHistory(doc); renderHex(doc);
    showTab("canvas");
  } catch (e) {
    document.getElementById("status").textContent = "Failed to parse NDA: " + e;
  }
}
"##;

const VIEWER_CSS: &str = r##"
body { margin: 0; background: #0d1117; color: #c9d1d9; font-family: system-ui, sans-serif; }
header { padding: 12px 16px; border-bottom: 1px solid #21262d; }
header h1 { font-size: 15px; margin: 0 0 4px; color: #58a6ff; }
#status { font-size: 12px; color: #8b949e; }
nav { display: flex; gap: 4px; padding: 8px 16px; border-bottom: 1px solid #21262d; }
nav button { background: #161b22; color: #c9d1d9; border: 1px solid #30363d; padding: 6px 12px; border-radius: 6px; cursor: pointer; font-size: 12px; }
nav button.active { background: #1f6feb; border-color: #1f6feb; color: #fff; }
main { padding: 16px; }
table { border-collapse: collapse; width: 100%; font-size: 12px; margin-bottom: 16px; }
td { border: 1px solid #21262d; padding: 4px 8px; vertical-align: top; word-break: break-word; }
td.s { color: #79c0ff; } td.p { color: #d2a8ff; } td.o { color: #7ee787; }
h3 { font-size: 13px; color: #8b949e; }
pre { font-size: 11px; line-height: 1.4; overflow: auto; }
.origin { background: #161b22; border: 1px solid #30363d; border-radius: 6px; padding: 8px 12px; margin-bottom: 12px; font-size: 12px; }
.rev { border-left: 2px solid #1f6feb; padding: 6px 12px; margin: 8px 0; background: #161b22; border-radius: 0 6px 6px 0; }
.revhead { font-size: 13px; } .revmeta { font-size: 11px; color: #8b949e; margin-top: 2px; }
.revhash { font-size: 10px; color: #6e7681; margin-top: 2px; font-family: monospace; }
.badge { background: #1f6feb33; color: #58a6ff; border-radius: 4px; padding: 1px 6px; font-size: 10px; }
.delta { border-radius: 4px; padding: 1px 6px; font-size: 10px; }
.delta.same { background: #2ea04333; color: #7ee787; }
.delta.changed { background: #bb800933; color: #e3b341; }
.email { color: #6e7681; font-size: 11px; }
canvas { border: 1px solid #21262d; border-radius: 6px; max-width: 100%; }
#drop { border: 2px dashed #30363d; border-radius: 8px; padding: 40px; text-align: center; color: #8b949e; margin: 16px; }
"##;

fn shell(title: &str, body: &str, onload: &str) -> String {
    format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{title}</title>\n<style>{css}</style>\n</head>\n<body>\n<header><h1>{title}</h1><div id=\"status\">loading…</div></header>\n<nav>\n<button id=\"tab-canvas\" onclick=\"showTab('canvas')\">Canvas</button>\n<button id=\"tab-graph\" onclick=\"showTab('graph')\">Triples</button>\n<button id=\"tab-history\" onclick=\"showTab('history')\">History</button>\n<button id=\"tab-hex\" onclick=\"showTab('hex')\">Bytes</button>\n</nav>\n<main>\n{body}\n<div id=\"canvas\"></div>\n<div id=\"graph\" style=\"display:none\"></div>\n<div id=\"history\" style=\"display:none\"></div>\n<div id=\"hex\" style=\"display:none\"></div>\n</main>\n<script>{js}\n{onload}\n</script>\n</body>\n</html>\n",
        title = title,
        css = VIEWER_CSS,
        body = body,
        js = VIEWER_JS,
        onload = onload,
    )
}

/// Build a fully self-contained HTML viewer with `nda_bytes` inlined as base64.
/// The result needs no network or sibling files — double-click to view.
pub fn self_contained_html(nda_bytes: &[u8], title: &str) -> String {
    let b64 = base64_encode(nda_bytes);
    let onload = format!(
        "const EMBEDDED = \"{b64}\";\n\
         function b64dec(s){{ const bin = atob(s); const out = new Uint8Array(bin.length); for (let i=0;i<bin.length;i++) out[i]=bin.charCodeAt(i); return out; }}\n\
         loadBytes(b64dec(EMBEDDED).buffer);",
        b64 = b64,
    );
    let doc_title = if title.is_empty() {
        "NDA Document"
    } else {
        title
    };
    shell(&format!("NDA · {doc_title}"), "", &onload)
}

/// Build the standalone PWA-style viewer with a file picker (opens any `.nda`).
pub fn pwa_viewer_html() -> String {
    let body = "<div id=\"drop\">Drop a <code>.nda</code> file here, or <label style=\"color:#58a6ff;cursor:pointer;text-decoration:underline\">browse<input id=\"file\" type=\"file\" accept=\".nda\" style=\"display:none\"></label></div>";
    let onload = r#"
const drop = document.getElementById("drop");
const fileInput = document.getElementById("file");
function handleFile(f){ if(!f) return; const r = new FileReader(); r.onload = () => { drop.style.display="none"; loadBytes(r.result); }; r.readAsArrayBuffer(f); }
fileInput.addEventListener("change", e => handleFile(e.target.files[0]));
["dragover","drop"].forEach(ev => window.addEventListener(ev, e => e.preventDefault()));
window.addEventListener("drop", e => { if (e.dataTransfer.files.length) handleFile(e.dataTransfer.files[0]); });
document.getElementById("status").textContent = "Open a .nda file to view it.";
"#;
    shell("NDA Viewer", body, onload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn self_contained_html_embeds_bytes() {
        let bytes = velocity_browser::nda_portable::NdaPortableDoc::new().to_portable_bytes();
        let html = self_contained_html(&bytes, "Test");
        assert!(html.contains("NDA · Test"));
        assert!(html.contains(&base64_encode(&bytes)));
        assert!(html.contains("function parseNda"));
    }

    #[test]
    fn pwa_viewer_has_file_picker() {
        let html = pwa_viewer_html();
        assert!(html.contains("type=\"file\""));
        assert!(html.contains("function parseNda"));
    }

    #[test]
    fn viewer_renders_images_vectors_and_wrap() {
        let html = pwa_viewer_html();
        assert!(html.contains("drawImage"), "image rendering");
        assert!(html.contains("function wrapText"), "text wrapping");
        assert!(html.contains("lineTo"), "vector polylines");
    }

    #[test]
    fn viewer_shows_revision_content_delta_badge() {
        let html = pwa_viewer_html();
        assert!(html.contains("delta same"), "same-content badge class");
        assert!(
            html.contains("content changed"),
            "changed-content badge text"
        );
        assert!(html.contains(".delta.changed"), "delta CSS present");
    }
}
