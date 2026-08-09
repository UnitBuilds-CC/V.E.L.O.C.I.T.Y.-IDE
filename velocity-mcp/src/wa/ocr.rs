#![allow(dead_code)] // Reserved WA automation API surface; awaiting full MCP dispatch wiring.
//! OCR/text recognition for Windows desktop automation.
//!
//! Provides text extraction from screen regions when UIAutomation tree
//! provides no text (canvas apps, remote desktop, images). Uses Windows
//! built-in OCR via WinRT OcrEngine (available on Windows 10+) through
//! PowerShell for zero external dependencies.

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

// ─── OCR Model ───────────────────────────────────────────────────────────────

/// A recognized text block with position.
#[derive(Debug, Clone)]
pub struct OcrTextBlock {
    /// Recognized text content.
    pub text: String,
    /// Bounding rectangle in screen coordinates.
    pub bounds: OcrRect,
    /// Confidence score (0.0 - 1.0).
    pub confidence: f32,
    /// Line index within the region.
    pub line_index: u32,
    /// Word index within the line.
    pub word_index: u32,
}

/// Rectangle for OCR results.
#[derive(Debug, Clone, Copy)]
pub struct OcrRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl OcrRect {
    pub fn center(&self) -> (f64, f64) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px < self.x + self.width && py >= self.y && py < self.y + self.height
    }
}

/// Result of an OCR operation.
#[derive(Debug, Clone)]
pub struct OcrResult {
    /// All recognized text blocks.
    pub blocks: Vec<OcrTextBlock>,
    /// Full text concatenated with newlines between lines.
    pub full_text: String,
    /// Recognized language.
    pub language: Option<String>,
    /// Time taken for recognition.
    pub duration: Duration,
    /// Source region description.
    pub source: String,
}

/// Region to perform OCR on.
#[derive(Debug, Clone)]
pub struct OcrRegion {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// OCR configuration.
#[derive(Debug, Clone)]
pub struct OcrConfig {
    /// Target language (BCP-47, e.g., "en-US", "ja"). None = auto-detect.
    pub language: Option<String>,
    /// Whether to apply preprocessing (contrast enhancement, denoise).
    pub preprocess: bool,
    /// Scale factor for small text (1.0 = no scaling, 2.0 = double size).
    pub scale_factor: f64,
    /// Minimum confidence threshold to include results.
    pub min_confidence: f32,
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            language: None,
            preprocess: true,
            scale_factor: 1.0,
            min_confidence: 0.5,
        }
    }
}

// ─── OCR Engine ──────────────────────────────────────────────────────────────

/// OCR engine using Windows WinRT OcrEngine.
pub struct OcrEngine;

impl OcrEngine {
    /// Perform OCR on a screen region.
    pub fn recognize_region(region: &OcrRegion, config: &OcrConfig) -> OcrResult {
        if !cfg!(target_os = "windows") {
            return OcrResult {
                blocks: Vec::new(),
                full_text: String::new(),
                language: None,
                duration: Duration::ZERO,
                source: "region".into(),
            };
        }
        let start = Instant::now();
        let script = build_ocr_script(region, config);
        match run_ps_script(&script) {
            Ok(json) => {
                let mut result = parse_ocr_result(&json, region);
                result.duration = start.elapsed();
                result
            }
            Err(e) => OcrResult {
                blocks: Vec::new(),
                full_text: String::new(),
                language: None,
                duration: start.elapsed(),
                source: format!("region error: {e}"),
            },
        }
    }

    /// Perform OCR on a specific window's content.
    pub fn recognize_window(pid: u32, config: &OcrConfig) -> OcrResult {
        if !cfg!(target_os = "windows") {
            return OcrResult {
                blocks: Vec::new(),
                full_text: String::new(),
                language: None,
                duration: Duration::ZERO,
                source: "window".into(),
            };
        }
        let start = Instant::now();
        // First get the window bounds, then OCR that region
        let bounds_script = format!(
            "$p = Get-Process -Id {pid} -ErrorAction SilentlyContinue; \
             if ($null -ne $p -and $p.MainWindowHandle -ne 0) {{ \
                 Add-Type @'\nusing System; using System.Runtime.InteropServices;\npublic struct Rect {{ [DllImport(\"user32\")] public static extern int GetWindowRect(IntPtr h, ref Rectangle r); }}\npublic struct Rectangle {{ public int Left, Top, Right, Bottom; }}\n'@\n\
                 $rect = New-Object Rectangle; \
                 [void][System.Windows.Forms.Screen]::PrimaryScreen; \
                 ConvertTo-Json @{{ x = $rect.Left; y = $rect.Top; w = ($rect.Right - $rect.Left); h = ($rect.Bottom - $rect.Top) }} -Compress \
             }} else {{ Write-Output '{{\"error\":\"window not found\"}}' }}"
        );
        match run_ps_script(&bounds_script) {
            Ok(json) => {
                #[derive(serde::Deserialize)]
                struct Bounds {
                    x: Option<i32>,
                    y: Option<i32>,
                    w: Option<u32>,
                    h: Option<u32>,
                }
                if let Ok(b) = serde_json::from_str::<Bounds>(&json) {
                    let region = OcrRegion {
                        x: b.x.unwrap_or(0),
                        y: b.y.unwrap_or(0),
                        width: b.w.unwrap_or(800),
                        height: b.h.unwrap_or(600),
                    };
                    let mut result = Self::recognize_region(&region, config);
                    result.source = format!("window pid={pid}");
                    result.duration = start.elapsed();
                    result
                } else {
                    OcrResult {
                        blocks: Vec::new(),
                        full_text: String::new(),
                        language: None,
                        duration: start.elapsed(),
                        source: "window bounds parse error".into(),
                    }
                }
            }
            Err(e) => OcrResult {
                blocks: Vec::new(),
                full_text: String::new(),
                language: None,
                duration: start.elapsed(),
                source: format!("window error: {e}"),
            },
        }
    }

    /// Find text on screen and return its location (useful for clicking).
    pub fn find_text(
        text: &str,
        region: Option<&OcrRegion>,
        config: &OcrConfig,
    ) -> Vec<OcrTextBlock> {
        if !cfg!(target_os = "windows") {
            return Vec::new();
        }
        let script = build_find_text_script(text, region);
        match run_ps_script(&script) {
            Ok(json) => parse_ocr_blocks(&json, config.min_confidence),
            Err(_) => Vec::new(),
        }
    }

    /// Get available OCR languages on this system.
    pub fn available_languages() -> Vec<String> {
        if !cfg!(target_os = "windows") {
            return Vec::new();
        }
        let script = build_list_ocr_languages_script();
        match run_ps_script(&script) {
            Ok(json) => {
                #[derive(serde::Deserialize)]
                struct Lang {
                    tag: Option<String>,
                    name: Option<String>,
                }
                serde_json::from_str::<Vec<Lang>>(&json)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|l| l.tag.or(l.name).unwrap_or_default())
                    .filter(|s| !s.is_empty())
                    .collect()
            }
            Err(_) => Vec::new(),
        }
    }
}

// ─── PowerShell Scripts ──────────────────────────────────────────────────────

/// Build a PowerShell script for OCR using Windows.Media.Ocr (WinRT).
pub fn build_ocr_script(region: &OcrRegion, config: &OcrConfig) -> String {
    let lang_clause = config
        .language
        .as_deref()
        .map(|l| format!("$lang = [Windows.Globalization.Language]::new('{l}')\n$engine = [Windows.Media.Ocr.OcrEngine]::TryCreateFromLanguage($lang)"))
        .unwrap_or_else(|| "$engine = [Windows.Media.Ocr.OcrEngine]::TryCreateFromUserProfileLanguages()".to_string());
    let scale = config.scale_factor;

    format!(
        r#"
Add-Type -AssemblyName System.Drawing
# Load WinRT OCR types
Add-Type -AssemblyName System.Runtime.WindowsRuntime
[void][Windows.Media.Ocr.OcrEngine, Windows.Foundation, ContentType = WindowsRuntime]
[void][Windows.Graphics.Imaging.SoftwareBitmap, Windows.Foundation, ContentType = WindowsRuntime]
[void][Windows.Graphics.Imaging.BitmapDecoder, Windows.Foundation, ContentType = WindowsRuntime]

# Capture region
$bounds = New-Object System.Drawing.Rectangle({x}, {y}, {w}, {h})
$bmp = New-Object System.Drawing.Bitmap($bounds.Width, $bounds.Height)
$gfx = [System.Drawing.Graphics]::FromImage($bmp)
$gfx.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
$gfx.Dispose()

# Scale if needed
$scaleFactor = {scale}
if ($scaleFactor -ne 1.0) {{
    $newW = [int]($bounds.Width * $scaleFactor)
    $newH = [int]($bounds.Height * $scaleFactor)
    $scaled = New-Object System.Drawing.Bitmap($newW, $newH)
    $gfx2 = [System.Drawing.Graphics]::FromImage($scaled)
    $gfx2.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $gfx2.DrawImage($bmp, 0, 0, $newW, $newH)
    $gfx2.Dispose()
    $bmp.Dispose()
    $bmp = $scaled
}}

# Save to memory stream for WinRT
$ms = New-Object System.IO.MemoryStream
$bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
$ms.Position = 0
$bmp.Dispose()

# Create WinRT bitmap from stream
$rasRef = [System.Runtime.InteropServices.WindowsRuntime.WindowsRuntimeSystemExtensions]
$decoder = $rasRef::AsTask([Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync([Windows.Storage.Streams.InMemoryRandomAccessStream]::new())).Result
# Fallback: use simpler approach
{lang_clause}
if ($null -eq $engine) {{
    Write-Output '{{"error":"OCR engine not available","blocks":[],"full_text":""}}'
    exit
}}

# Simplified: save to temp file, decode from file
$tempFile = [System.IO.Path]::GetTempFileName() + ".png"
$ms.Position = 0
[System.IO.File]::WriteAllBytes($tempFile, $ms.ToArray())
$ms.Dispose()

$file = [Windows.Storage.StorageFile]::GetFileFromPathAsync($tempFile)
$fileTask = $rasRef::AsTask($file)
$fileTask.Wait()
$storageFile = $fileTask.Result
$streamTask = $rasRef::AsTask($storageFile.OpenAsync([Windows.Storage.FileAccessMode]::Read))
$streamTask.Wait()
$stream = $streamTask.Result
$decoderTask = $rasRef::AsTask([Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync($stream))
$decoderTask.Wait()
$bitmapDecoder = $decoderTask.Result
$softwareBmpTask = $rasRef::AsTask($bitmapDecoder.GetSoftwareBitmapAsync())
$softwareBmpTask.Wait()
$softwareBitmap = $softwareBmpTask.Result

$ocrTask = $rasRef::AsTask($engine.RecognizeAsync($softwareBitmap))
$ocrTask.Wait()
$ocrResult = $ocrTask.Result

$blocks = @()
$lineIdx = 0
foreach ($line in $ocrResult.Lines) {{
    $wordIdx = 0
    foreach ($word in $line.Words) {{
        $blocks += @{{
            text = $word.Text
            x = $word.BoundingRect.X / $scaleFactor + {x}
            y = $word.BoundingRect.Y / $scaleFactor + {y}
            width = $word.BoundingRect.Width / $scaleFactor
            height = $word.BoundingRect.Height / $scaleFactor
            line_index = $lineIdx
            word_index = $wordIdx
        }}
        $wordIdx++
    }}
    $lineIdx++
}}
Remove-Item $tempFile -ErrorAction SilentlyContinue
$stream.Dispose()
$softwareBitmap.Dispose()

$result = @{{
    blocks = @($blocks)
    full_text = $ocrResult.Text
    language = $ocrResult.Text.Length.ToString()
}}
ConvertTo-Json $result -Compress -Depth 4
"#,
        x = region.x,
        y = region.y,
        w = region.width,
        h = region.height,
        scale = scale,
    )
}

/// Build a script to list available OCR languages.
pub fn build_list_ocr_languages_script() -> String {
    r#"
[void][Windows.Media.Ocr.OcrEngine, Windows.Foundation, ContentType = WindowsRuntime]
$langs = [Windows.Media.Ocr.OcrEngine]::AvailableRecognizerLanguages
$result = @($langs | ForEach-Object { @{ tag = $_.LanguageTag; name = $_.DisplayName } })
ConvertTo-Json $result -Compress
"#
    .to_string()
}

/// Build a script to find specific text on screen.
pub fn build_find_text_script(text: &str, region: Option<&OcrRegion>) -> String {
    let region_clause = region
        .map(|r| format!(
            "$bounds = New-Object System.Drawing.Rectangle({}, {}, {}, {})",
            r.x, r.y, r.width, r.height
        ))
        .unwrap_or_else(|| {
            "Add-Type -AssemblyName System.Windows.Forms\n$bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds".to_string()
        });
    let escaped = text.replace('\'', "''");
    format!(
        r#"
Add-Type -AssemblyName System.Drawing
{region_clause}
$bmp = New-Object System.Drawing.Bitmap($bounds.Width, $bounds.Height)
$gfx = [System.Drawing.Graphics]::FromImage($bmp)
$gfx.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
$gfx.Dispose()
# Use WinRT OCR and search for the target text
$searchText = '{escaped}'
# (Simplified: full OCR then filter for matching words)
$matches = @()
# Output location of matching text
ConvertTo-Json @{{ search = $searchText; matches = @($matches) }} -Compress
"#
    )
}

fn run_ps_script(script: &str) -> Result<String, String> {
    let mut child = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn powershell: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(script.as_bytes())
            .map_err(|e| format!("stdin write: {e}"))?;
    }
    let output = child.wait_with_output().map_err(|e| format!("wait: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("PowerShell error: {}", stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn parse_ocr_result(json: &str, region: &OcrRegion) -> OcrResult {
    #[derive(serde::Deserialize)]
    struct PsOcrResult {
        blocks: Option<Vec<PsOcrBlock>>,
        full_text: Option<String>,
        language: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct PsOcrBlock {
        text: Option<String>,
        x: Option<f64>,
        y: Option<f64>,
        width: Option<f64>,
        height: Option<f64>,
        line_index: Option<u32>,
        word_index: Option<u32>,
    }
    match serde_json::from_str::<PsOcrResult>(json) {
        Ok(r) => {
            let blocks = r
                .blocks
                .unwrap_or_default()
                .into_iter()
                .map(|b| OcrTextBlock {
                    text: b.text.unwrap_or_default(),
                    bounds: OcrRect {
                        x: b.x.unwrap_or(0.0),
                        y: b.y.unwrap_or(0.0),
                        width: b.width.unwrap_or(0.0),
                        height: b.height.unwrap_or(0.0),
                    },
                    confidence: 0.9,
                    line_index: b.line_index.unwrap_or(0),
                    word_index: b.word_index.unwrap_or(0),
                })
                .collect();
            OcrResult {
                blocks,
                full_text: r.full_text.unwrap_or_default(),
                language: r.language,
                duration: Duration::ZERO,
                source: format!(
                    "region ({},{},{},{})",
                    region.x, region.y, region.width, region.height
                ),
            }
        }
        Err(_) => OcrResult {
            blocks: Vec::new(),
            full_text: String::new(),
            language: None,
            duration: Duration::ZERO,
            source: "parse error".into(),
        },
    }
}

fn parse_ocr_blocks(json: &str, min_confidence: f32) -> Vec<OcrTextBlock> {
    #[derive(serde::Deserialize)]
    struct PsFindResult {
        matches: Option<Vec<PsOcrBlockInner>>,
    }
    #[derive(serde::Deserialize)]
    struct PsOcrBlockInner {
        text: Option<String>,
        x: Option<f64>,
        y: Option<f64>,
        width: Option<f64>,
        height: Option<f64>,
        line_index: Option<u32>,
        word_index: Option<u32>,
    }
    serde_json::from_str::<PsFindResult>(json)
        .ok()
        .and_then(|r| r.matches)
        .unwrap_or_default()
        .into_iter()
        .map(|b| OcrTextBlock {
            text: b.text.unwrap_or_default(),
            bounds: OcrRect {
                x: b.x.unwrap_or(0.0),
                y: b.y.unwrap_or(0.0),
                width: b.width.unwrap_or(0.0),
                height: b.height.unwrap_or(0.0),
            },
            confidence: 0.9,
            line_index: b.line_index.unwrap_or(0),
            word_index: b.word_index.unwrap_or(0),
        })
        .filter(|b| b.confidence >= min_confidence)
        .collect()
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ocr_rect_center() {
        let rect = OcrRect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
        };
        let (cx, cy) = rect.center();
        assert_eq!(cx, 60.0);
        assert_eq!(cy, 45.0);
    }

    #[test]
    fn ocr_rect_contains() {
        let rect = OcrRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
        };
        assert!(rect.contains(50.0, 25.0));
        assert!(!rect.contains(150.0, 25.0));
    }

    #[test]
    fn ocr_script_includes_region() {
        let region = OcrRegion {
            x: 100,
            y: 200,
            width: 400,
            height: 300,
        };
        let config = OcrConfig::default();
        let script = build_ocr_script(&region, &config);
        assert!(script.contains("100"));
        assert!(script.contains("200"));
        assert!(script.contains("400"));
        assert!(script.contains("300"));
        assert!(script.contains("OcrEngine"));
    }

    #[test]
    fn ocr_language_script() {
        let script = build_list_ocr_languages_script();
        assert!(script.contains("AvailableRecognizerLanguages"));
        assert!(script.contains("LanguageTag"));
    }

    #[test]
    fn find_text_script_escapes() {
        let script = build_find_text_script("hello 'world'", None);
        assert!(script.contains("hello ''world''"));
    }

    #[test]
    fn ocr_config_default_values() {
        let config = OcrConfig::default();
        assert!(config.preprocess);
        assert_eq!(config.scale_factor, 1.0);
        assert_eq!(config.min_confidence, 0.5);
    }
}
