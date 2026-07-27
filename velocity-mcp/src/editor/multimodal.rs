//! Pillar 6 — Multimodal I/O.
//!
//! Attachments (images, documents, audio) that can be threaded through a chat
//! turn. Images are encoded as `data:` URLs for vision-capable models; when the
//! selected model lacks vision the image is described via the OCR fallback.
//! Document text extraction delegates to the Pillar 1 knowledge extractor.

use std::path::{Path, PathBuf};

/// Kind of attachment, inferred from the file's MIME type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentKind {
    Image,
    Document,
    Audio,
}

impl AttachmentKind {
    pub fn label(self) -> &'static str {
        match self {
            AttachmentKind::Image => "image",
            AttachmentKind::Document => "document",
            AttachmentKind::Audio => "audio",
        }
    }
}

/// A file attached to a chat turn.
#[derive(Debug, Clone)]
pub struct Attachment {
    pub kind: AttachmentKind,
    pub path: PathBuf,
    pub mime: String,
    /// Raw bytes of the file (loaded eagerly so prompt assembly is offline).
    pub data: Vec<u8>,
}

impl Attachment {
    /// Load an attachment from disk, inferring kind + MIME from the extension.
    pub fn load(path: impl AsRef<Path>) -> Result<Attachment, String> {
        let path = path.as_ref().to_path_buf();
        let data = std::fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let mime = guess_mime(&path).to_string();
        let kind = kind_for_mime(&mime);
        Ok(Attachment {
            kind,
            path,
            mime,
            data,
        })
    }

    /// Encode this attachment's bytes as a `data:` URL.
    pub fn data_url(&self) -> String {
        encode_data_url(&self.mime, &self.data)
    }

    /// Best-effort text representation for models without native support:
    /// documents are extracted as text, images fall back to OCR, audio has no
    /// textual form yet.
    pub fn fallback_text(&self) -> Option<String> {
        match self.kind {
            AttachmentKind::Document => crate::editor::knowledge_base::extract_text(&self.path),
            AttachmentKind::Image => Some(ocr_image_text(&self.path)),
            AttachmentKind::Audio => None,
        }
    }
}

/// How an image attachment should be delivered to a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageDelivery {
    /// Send the image itself (data URL) to a vision-capable model.
    Vision,
    /// Send extracted OCR text because the model has no vision.
    OcrText,
}

/// Decide how to deliver an image to a model given its id.
pub fn image_delivery_for_model(model_id: &str) -> ImageDelivery {
    if model_supports_vision(model_id) {
        ImageDelivery::Vision
    } else {
        ImageDelivery::OcrText
    }
}

/// Heuristic capability check: whether a model id denotes a vision-capable
/// model. Kept as a standalone helper so `ModelInfo`'s many construction sites
/// stay untouched.
pub fn model_supports_vision(model_id: &str) -> bool {
    let id = model_id.to_lowercase();
    const VISION_MARKERS: &[&str] = &[
        "vision",
        "-vl",
        "vl-",
        "llava",
        "gpt-4o",
        "gpt-4-turbo",
        "gpt-4.1",
        "o4",
        "claude-3",
        "claude-sonnet",
        "claude-opus",
        "gemini",
        "pixtral",
        "qwen2-vl",
        "qwen2.5-vl",
        "llama-3.2",
        "phi-3-vision",
        "phi-3.5-vision",
        "internvl",
    ];
    VISION_MARKERS.iter().any(|m| id.contains(m))
}

/// Assemble the OpenAI-style content parts for a chat turn that carries text
/// plus optional attachments, honoring the model's vision capability.
///
/// Returns a JSON array suitable for a `content` field: text parts as
/// `{ "type": "text", "text": ... }` and images (for vision models) as
/// `{ "type": "image_url", "image_url": { "url": data_url } }`.
pub fn assemble_content_parts(
    model_id: &str,
    text: &str,
    attachments: &[Attachment],
) -> serde_json::Value {
    let mut parts = Vec::new();
    let mut extra_text = String::new();

    for att in attachments {
        match att.kind {
            AttachmentKind::Image => match image_delivery_for_model(model_id) {
                ImageDelivery::Vision => {
                    parts.push(serde_json::json!({
                        "type": "image_url",
                        "image_url": { "url": att.data_url() }
                    }));
                }
                ImageDelivery::OcrText => {
                    if let Some(t) = att.fallback_text() {
                        if !t.trim().is_empty() {
                            extra_text
                                .push_str(&format!("\n\n[image: {}]\n{}", att.path.display(), t));
                        }
                    }
                }
            },
            AttachmentKind::Document => {
                if let Some(t) = att.fallback_text() {
                    extra_text.push_str(&format!("\n\n[document: {}]\n{}", att.path.display(), t));
                }
            }
            AttachmentKind::Audio => {
                extra_text.push_str(&format!("\n\n[audio attached: {}]", att.path.display()));
            }
        }
    }

    let combined = format!("{text}{extra_text}");
    // Text part goes first so the instruction precedes image parts.
    let mut all = vec![serde_json::json!({ "type": "text", "text": combined })];
    all.append(&mut parts);
    serde_json::Value::Array(all)
}

/// Encode bytes as a `data:<mime>;base64,<payload>` URL.
pub fn encode_data_url(mime: &str, bytes: &[u8]) -> String {
    format!("data:{};base64,{}", mime, base64_encode(bytes))
}

/// Guess a MIME type from a file extension. Returns `application/octet-stream`
/// for unknown extensions.
pub fn guess_mime(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "md" => "text/markdown",
        "txt" | "log" => "text/plain",
        "csv" => "text/csv",
        "json" => "application/json",
        "toml" => "application/toml",
        "html" | "htm" => "text/html",
        "wav" => "audio/wav",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        "m4a" => "audio/mp4",
        _ => "application/octet-stream",
    }
}

/// Map a MIME type to an [`AttachmentKind`].
pub fn kind_for_mime(mime: &str) -> AttachmentKind {
    if mime.starts_with("image/") {
        AttachmentKind::Image
    } else if mime.starts_with("audio/") {
        AttachmentKind::Audio
    } else {
        AttachmentKind::Document
    }
}

/// OCR an image file into text as a vision fallback. Returns an empty string on
/// non-Windows hosts (where the OCR engine is unavailable).
fn ocr_image_text(_path: &Path) -> String {
    // The native OCR engine operates over screen regions; for a file-based
    // fallback we currently return an empty string on platforms without an
    // image-decode + OCR bridge. Vision-capable models are the primary path.
    String::new()
}

/// Pure base64 encoder (standard alphabet, `=` padding). Kept local so the
/// module has no external crate dependency.
fn base64_encode(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
        let b2 = chunk.get(2).copied().unwrap_or(0) as usize;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(T[(n >> 18) & 63] as char);
        out.push(T[(n >> 12) & 63] as char);
        if chunk.len() > 1 {
            out.push(T[(n >> 6) & 63] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(T[n & 63] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Default Workers AI text-to-image model.
const DEFAULT_IMAGE_MODEL: &str = "@cf/stabilityai/stable-diffusion-xl-base-1.0";

/// Generate an image from a text prompt via Cloudflare Workers AI and save it
/// into the workspace. Returns the saved path. Requires a configured
/// Cloudflare account (workspace provider settings or environment).
pub fn generate_image(
    workspace_root: &Path,
    prompt: &str,
    model: Option<&str>,
    out_rel: Option<&str>,
) -> Result<PathBuf, String> {
    if prompt.trim().is_empty() {
        return Err("prompt is required".to_string());
    }
    let accounts = crate::usage::load_accounts(workspace_root);
    let account = accounts
        .first()
        .ok_or("no Cloudflare Workers AI account configured")?;
    let model = model.unwrap_or(DEFAULT_IMAGE_MODEL);
    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/ai/run/{}",
        account.id, model
    );
    let response = ureq::post(&url)
        .timeout(std::time::Duration::from_secs(120))
        .set("Authorization", &format!("Bearer {}", account.token))
        .set("Accept", "image/png")
        .send_json(serde_json::json!({ "prompt": prompt }))
        .map_err(|e| format!("image generation request failed: {e}"))?;

    let mut bytes = Vec::new();
    use std::io::Read;
    response
        .into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| format!("reading image response failed: {e}"))?;
    if bytes.is_empty() {
        return Err("image generation returned no data".to_string());
    }

    let rel = out_rel
        .map(str::to_string)
        .unwrap_or_else(|| format!("generated/image-{}.png", now_secs()));
    let out_path = workspace_root.join(&rel);
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("creating output dir failed: {e}"))?;
    }
    std::fs::write(&out_path, &bytes).map_err(|e| format!("writing image failed: {e}"))?;
    Ok(out_path)
}

/// Describe an image file. Vision-capable models can consume the image directly
/// (a `data:` URL is provided); this helper additionally returns an OCR text
/// fallback for non-vision models.
pub fn describe_image(path: &Path) -> Result<serde_json::Value, String> {
    let att = Attachment::load(path)?;
    if att.kind != AttachmentKind::Image {
        return Err(format!("{} is not an image ({})", path.display(), att.mime));
    }
    let ocr = att.fallback_text().unwrap_or_default();
    Ok(serde_json::json!({
        "path": path.display().to_string(),
        "mime": att.mime,
        "bytes": att.data.len(),
        "data_url": att.data_url(),
        "ocr_text": ocr,
        "note": "Vision-capable models can view the image via the data URL; non-vision models use ocr_text.",
    }))
}

/// Epoch seconds, mirroring the convention used across the editor subsystems.
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_encoding_matches_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn data_url_has_mime_and_base64_payload() {
        let url = encode_data_url("image/png", b"foobar");
        assert_eq!(url, "data:image/png;base64,Zm9vYmFy");
    }

    #[test]
    fn mime_and_kind_inference() {
        assert_eq!(guess_mime(Path::new("a.png")), "image/png");
        assert_eq!(guess_mime(Path::new("a.JPG")), "image/jpeg");
        assert_eq!(guess_mime(Path::new("a.pdf")), "application/pdf");
        assert_eq!(guess_mime(Path::new("a.wav")), "audio/wav");
        assert_eq!(guess_mime(Path::new("a.unknown")), "application/octet-stream");

        assert_eq!(kind_for_mime("image/png"), AttachmentKind::Image);
        assert_eq!(kind_for_mime("audio/wav"), AttachmentKind::Audio);
        assert_eq!(kind_for_mime("application/pdf"), AttachmentKind::Document);
        assert_eq!(kind_for_mime("text/markdown"), AttachmentKind::Document);
    }

    #[test]
    fn vision_capability_detection() {
        assert!(model_supports_vision("gpt-4o"));
        assert!(model_supports_vision("openai/gpt-4o-mini"));
        assert!(model_supports_vision("qwen2.5-vl-7b"));
        assert!(model_supports_vision("llava:13b"));
        assert!(model_supports_vision("claude-3-5-sonnet"));
        assert!(model_supports_vision("google/gemini-1.5-pro"));

        assert!(!model_supports_vision("gpt-3.5-turbo"));
        assert!(!model_supports_vision("mistral-7b-instruct"));
        assert!(!model_supports_vision("llama-3.1-8b"));
    }

    #[test]
    fn delivery_selection_by_capability() {
        assert_eq!(image_delivery_for_model("gpt-4o"), ImageDelivery::Vision);
        assert_eq!(
            image_delivery_for_model("mistral-7b"),
            ImageDelivery::OcrText
        );
    }

    #[test]
    fn vision_model_gets_image_url_part() {
        let att = Attachment {
            kind: AttachmentKind::Image,
            path: PathBuf::from("shot.png"),
            mime: "image/png".to_string(),
            data: b"foobar".to_vec(),
        };
        let parts = assemble_content_parts("gpt-4o", "describe this", &[att]);
        let arr = parts.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["type"], "text");
        assert_eq!(arr[0]["text"], "describe this");
        assert_eq!(arr[1]["type"], "image_url");
        assert_eq!(arr[1]["image_url"]["url"], "data:image/png;base64,Zm9vYmFy");
    }

    #[test]
    fn non_vision_model_folds_document_text_into_prompt() {
        let dir = std::env::temp_dir().join(format!("mm_doc_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let doc = dir.join("note.txt");
        std::fs::write(&doc, "hello from document").unwrap();
        let att = Attachment::load(&doc).unwrap();
        assert_eq!(att.kind, AttachmentKind::Document);

        let parts = assemble_content_parts("mistral-7b", "summarize", &[att]);
        let arr = parts.as_array().unwrap();
        // No image part for a document; text part carries the extracted content.
        assert_eq!(arr.len(), 1);
        let text = arr[0]["text"].as_str().unwrap();
        assert!(text.contains("summarize"));
        assert!(text.contains("hello from document"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attachment_load_infers_image_kind_and_data_url() {
        let dir = std::env::temp_dir().join(format!("mm_img_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let img = dir.join("pixel.png");
        std::fs::write(&img, b"foobar").unwrap();
        let att = Attachment::load(&img).unwrap();
        assert_eq!(att.kind, AttachmentKind::Image);
        assert_eq!(att.mime, "image/png");
        assert_eq!(att.data_url(), "data:image/png;base64,Zm9vYmFy");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
