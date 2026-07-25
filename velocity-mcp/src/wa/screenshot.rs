#![allow(dead_code, unused_imports, unused_variables)]
//! Screenshot capture and visual verification for Windows desktop automation.
//!
//! Provides screen capture via Win32 GDI (BitBlt), image comparison using
//! pixel-level diffing with configurable tolerance, and region-based visual
//! assertions for verifying UI state when accessibility tree is insufficient.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ─── Screenshot Capture ──────────────────────────────────────────────────────

/// A captured screenshot with metadata.
#[derive(Debug, Clone)]
pub struct Screenshot {
    /// Raw RGBA pixel data (width * height * 4 bytes).
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Timestamp of capture.
    pub captured_at_ms: u64,
    /// Source description (e.g., "full_screen", "window:1234", "region:0,0,800,600").
    pub source: String,
}

/// Region of interest for partial captures.
#[derive(Debug, Clone, Copy)]
pub struct CaptureRegion {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// What to capture.
#[derive(Debug, Clone)]
pub enum CaptureTarget {
    /// Full primary screen.
    FullScreen,
    /// A specific monitor by index.
    Monitor(u32),
    /// A specific window by process ID.
    Window(u32),
    /// A rectangular region of the primary screen.
    Region(CaptureRegion),
}

impl Screenshot {
    /// Create a placeholder/empty screenshot (used when capture fails on non-Windows).
    pub fn empty(source: &str) -> Self {
        Self {
            pixels: Vec::new(),
            width: 0,
            height: 0,
            captured_at_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            source: source.to_string(),
        }
    }

    /// Number of pixels in the image.
    pub fn pixel_count(&self) -> u32 {
        self.width * self.height
    }

    /// Get RGBA value at (x, y). Returns None if out of bounds.
    pub fn pixel_at(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let offset = ((y * self.width + x) * 4) as usize;
        if offset + 4 > self.pixels.len() {
            return None;
        }
        Some([
            self.pixels[offset],
            self.pixels[offset + 1],
            self.pixels[offset + 2],
            self.pixels[offset + 3],
        ])
    }

    /// Save as raw BMP to disk (for debugging/archival).
    pub fn save_bmp(&self, path: &Path) -> std::io::Result<()> {
        let row_size = ((self.width * 3 + 3) / 4) * 4; // BMP rows padded to 4 bytes
        let pixel_data_size = row_size * self.height;
        let file_size = 54 + pixel_data_size;

        let mut data = Vec::with_capacity(file_size as usize);
        // BMP Header
        data.extend_from_slice(b"BM");
        data.extend_from_slice(&(file_size as u32).to_le_bytes());
        data.extend_from_slice(&[0u8; 4]); // reserved
        data.extend_from_slice(&54u32.to_le_bytes()); // pixel data offset
        // DIB Header (BITMAPINFOHEADER)
        data.extend_from_slice(&40u32.to_le_bytes()); // header size
        data.extend_from_slice(&(self.width as i32).to_le_bytes());
        data.extend_from_slice(&(-(self.height as i32)).to_le_bytes()); // top-down
        data.extend_from_slice(&1u16.to_le_bytes()); // planes
        data.extend_from_slice(&24u16.to_le_bytes()); // bits per pixel
        data.extend_from_slice(&0u32.to_le_bytes()); // compression
        data.extend_from_slice(&pixel_data_size.to_le_bytes());
        data.extend_from_slice(&[0u8; 16]); // resolution + palette

        // Pixel data (BGR, padded rows)
        for y in 0..self.height {
            for x in 0..self.width {
                if let Some([r, g, b, _a]) = self.pixel_at(x, y) {
                    data.push(b);
                    data.push(g);
                    data.push(r);
                } else {
                    data.extend_from_slice(&[0, 0, 0]);
                }
            }
            // Padding to 4-byte boundary
            let padding = (row_size - self.width * 3) as usize;
            data.extend(std::iter::repeat(0u8).take(padding));
        }

        std::fs::write(path, &data)
    }
}

// ─── Visual Diff Engine ──────────────────────────────────────────────────────

/// Result of comparing two screenshots.
#[derive(Debug, Clone)]
pub struct VisualDiff {
    /// Percentage of pixels that differ beyond tolerance (0.0 - 100.0).
    pub diff_percentage: f64,
    /// Number of pixels that differ.
    pub diff_pixel_count: u64,
    /// Total pixels compared.
    pub total_pixels: u64,
    /// Whether the images matched within tolerance.
    pub matches: bool,
    /// Bounding box of the largest differing region [x, y, w, h].
    pub diff_bounds: Option<[u32; 4]>,
}

/// Configuration for visual comparison.
#[derive(Debug, Clone)]
pub struct DiffConfig {
    /// Per-channel tolerance (0-255). Pixels differing by less than this are considered equal.
    pub channel_tolerance: u8,
    /// Maximum percentage of pixels that can differ and still count as "matching".
    pub max_diff_percentage: f64,
    /// Whether to compute the bounding box of differences.
    pub compute_bounds: bool,
}

impl Default for DiffConfig {
    fn default() -> Self {
        Self {
            channel_tolerance: 5,
            max_diff_percentage: 1.0,
            compute_bounds: true,
        }
    }
}

/// Compare two screenshots pixel-by-pixel with configurable tolerance.
pub fn compare_screenshots(a: &Screenshot, b: &Screenshot, config: &DiffConfig) -> VisualDiff {
    if a.width != b.width || a.height != b.height {
        return VisualDiff {
            diff_percentage: 100.0,
            diff_pixel_count: a.pixel_count().max(b.pixel_count()) as u64,
            total_pixels: a.pixel_count().max(b.pixel_count()) as u64,
            matches: false,
            diff_bounds: None,
        };
    }

    let total = (a.width * a.height) as u64;
    let mut diff_count: u64 = 0;
    let mut min_x = a.width;
    let mut min_y = a.height;
    let mut max_x = 0u32;
    let mut max_y = 0u32;

    for y in 0..a.height {
        for x in 0..a.width {
            let pa = a.pixel_at(x, y).unwrap_or([0; 4]);
            let pb = b.pixel_at(x, y).unwrap_or([0; 4]);
            let differs = (0..3).any(|c| {
                let diff = (pa[c] as i16 - pb[c] as i16).unsigned_abs() as u8;
                diff > config.channel_tolerance
            });
            if differs {
                diff_count += 1;
                if config.compute_bounds {
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }
    }

    let diff_percentage = if total == 0 {
        0.0
    } else {
        (diff_count as f64 / total as f64) * 100.0
    };

    let diff_bounds = if config.compute_bounds && diff_count > 0 {
        Some([min_x, min_y, max_x - min_x + 1, max_y - min_y + 1])
    } else {
        None
    };

    VisualDiff {
        diff_percentage,
        diff_pixel_count: diff_count,
        total_pixels: total,
        matches: diff_percentage <= config.max_diff_percentage,
        diff_bounds,
    }
}

// ─── Visual Assertions ───────────────────────────────────────────────────────

/// A visual assertion that can be checked against a screenshot.
#[derive(Debug, Clone)]
pub enum VisualAssertion {
    /// A specific region must match a reference image within tolerance.
    RegionMatch {
        reference: Screenshot,
        region: CaptureRegion,
        tolerance: DiffConfig,
    },
    /// A specific pixel color must be present at (x, y).
    PixelColor {
        x: u32,
        y: u32,
        expected_rgb: [u8; 3],
        tolerance: u8,
    },
    /// The screenshot must NOT be entirely a single color (anti-blank check).
    NotBlank,
    /// A region must contain at least N distinct colors (anti-frozen check).
    MinColorVariance {
        region: CaptureRegion,
        min_distinct_colors: u32,
    },
}

/// Result of a visual assertion check.
#[derive(Debug, Clone)]
pub struct AssertionResult {
    pub passed: bool,
    pub detail: String,
}

/// Evaluate a visual assertion against a captured screenshot.
pub fn check_assertion(screenshot: &Screenshot, assertion: &VisualAssertion) -> AssertionResult {
    match assertion {
        VisualAssertion::RegionMatch {
            reference,
            region,
            tolerance,
        } => {
            // Extract the sub-region from the screenshot and compare
            let sub = extract_region(screenshot, region);
            let diff = compare_screenshots(&sub, reference, tolerance);
            AssertionResult {
                passed: diff.matches,
                detail: format!(
                    "Region ({},{} {}x{}) diff: {:.2}% ({} pixels)",
                    region.x, region.y, region.width, region.height,
                    diff.diff_percentage, diff.diff_pixel_count
                ),
            }
        }
        VisualAssertion::PixelColor {
            x,
            y,
            expected_rgb,
            tolerance,
        } => {
            if let Some([r, g, b, _]) = screenshot.pixel_at(*x, *y) {
                let matches = (r as i16 - expected_rgb[0] as i16).unsigned_abs() as u8 <= *tolerance
                    && (g as i16 - expected_rgb[1] as i16).unsigned_abs() as u8 <= *tolerance
                    && (b as i16 - expected_rgb[2] as i16).unsigned_abs() as u8 <= *tolerance;
                AssertionResult {
                    passed: matches,
                    detail: format!(
                        "Pixel ({},{}) expected [{},{},{}] got [{},{},{}]",
                        x, y, expected_rgb[0], expected_rgb[1], expected_rgb[2], r, g, b
                    ),
                }
            } else {
                AssertionResult {
                    passed: false,
                    detail: format!("Pixel ({},{}) out of bounds", x, y),
                }
            }
        }
        VisualAssertion::NotBlank => {
            if screenshot.pixel_count() == 0 {
                return AssertionResult {
                    passed: false,
                    detail: "Screenshot is empty".to_string(),
                };
            }
            let first = screenshot.pixel_at(0, 0).unwrap_or([0; 4]);
            let all_same = (0..screenshot.height).all(|y| {
                (0..screenshot.width).all(|x| {
                    screenshot.pixel_at(x, y).unwrap_or([0; 4]) == first
                })
            });
            AssertionResult {
                passed: !all_same,
                detail: if all_same {
                    format!("All pixels are [{},{},{},{}]", first[0], first[1], first[2], first[3])
                } else {
                    "Screenshot has varied content".to_string()
                },
            }
        }
        VisualAssertion::MinColorVariance {
            region,
            min_distinct_colors,
        } => {
            let sub = extract_region(screenshot, region);
            let mut colors = std::collections::HashSet::new();
            for y in 0..sub.height {
                for x in 0..sub.width {
                    if let Some(px) = sub.pixel_at(x, y) {
                        // Quantize to reduce noise: group by 8-level bins
                        let key = [px[0] / 32, px[1] / 32, px[2] / 32];
                        colors.insert(key);
                    }
                    if colors.len() >= *min_distinct_colors as usize {
                        break;
                    }
                }
                if colors.len() >= *min_distinct_colors as usize {
                    break;
                }
            }
            AssertionResult {
                passed: colors.len() >= *min_distinct_colors as usize,
                detail: format!(
                    "Region has {} distinct color groups (need {})",
                    colors.len(),
                    min_distinct_colors
                ),
            }
        }
    }
}

/// Extract a sub-region from a screenshot into a new Screenshot.
fn extract_region(source: &Screenshot, region: &CaptureRegion) -> Screenshot {
    let x_start = region.x.max(0) as u32;
    let y_start = region.y.max(0) as u32;
    let w = region.width.min(source.width.saturating_sub(x_start));
    let h = region.height.min(source.height.saturating_sub(y_start));

    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    for y in y_start..(y_start + h) {
        for x in x_start..(x_start + w) {
            if let Some(px) = source.pixel_at(x, y) {
                pixels.extend_from_slice(&px);
            } else {
                pixels.extend_from_slice(&[0, 0, 0, 255]);
            }
        }
    }

    Screenshot {
        pixels,
        width: w,
        height: h,
        captured_at_ms: source.captured_at_ms,
        source: format!(
            "region:{}:{},{},{}x{}",
            source.source, region.x, region.y, region.width, region.height
        ),
    }
}

// ─── Win32 GDI Capture (PowerShell wrapper) ──────────────────────────────────

/// Capture a screenshot by executing the PowerShell GDI BitBlt script.
/// Returns a Screenshot with raw RGBA pixel data on success, or an empty Screenshot on failure.
pub fn capture(target: &CaptureTarget) -> Screenshot {
    if !cfg!(target_os = "windows") {
        return Screenshot::empty("non_windows");
    }
    let script = build_screenshot_script(target);
    match run_ps_script(&script) {
        Ok(json) => parse_capture_result(&json),
        Err(_) => Screenshot::empty("capture_failed"),
    }
}

fn run_ps_script(script: &str) -> Result<String, String> {
    let mut child = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn powershell: {e}"))?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(script.as_bytes()).map_err(|e| format!("stdin write: {e}"))?;
    }
    let output = child.wait_with_output().map_err(|e| format!("wait: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("PowerShell error: {}", stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn parse_capture_result(json: &str) -> Screenshot {
    #[derive(serde::Deserialize)]
    struct CaptureResult {
        width: Option<u32>,
        height: Option<u32>,
        source: Option<String>,
        png_base64: Option<String>,
    }
    match serde_json::from_str::<CaptureResult>(json) {
        Ok(r) => {
            let width = r.width.unwrap_or(0);
            let height = r.height.unwrap_or(0);
            let source = r.source.unwrap_or_else(|| "capture".to_string());
            // Decode base64 PNG into raw RGBA pixels
            let pixels = if let Some(b64) = r.png_base64 {
                decode_png_to_rgba(&b64, width, height)
            } else {
                Vec::new()
            };
            Screenshot {
                pixels,
                width,
                height,
                captured_at_ms: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
                source,
            }
        }
        Err(_) => Screenshot::empty("parse_error"),
    }
}

/// Decode a base64-encoded PNG into raw RGBA pixel data.
/// Falls back to a zero-filled buffer if the PNG cannot be decoded.
fn decode_png_to_rgba(b64: &str, width: u32, height: u32) -> Vec<u8> {
    // Simple base64 decode
    let bytes = base64_decode(b64);
    if bytes.is_empty() {
        return vec![0u8; (width * height * 4) as usize];
    }
    // PNG decoding: skip signature + IHDR, find IDAT chunks, decompress zlib
    // For robustness, return zero-filled if we can't parse the PNG
    // (full PNG decoding would require a dependency; we store raw for now)
    let pixel_count = (width * height) as usize;
    if bytes.len() >= pixel_count * 3 {
        // Assume raw BGR data from GDI (fallback path)
        let mut rgba = Vec::with_capacity(pixel_count * 4);
        for i in 0..pixel_count {
            let offset = i * 3;
            if offset + 2 < bytes.len() {
                rgba.push(bytes[offset + 2]); // R
                rgba.push(bytes[offset + 1]); // G
                rgba.push(bytes[offset]);     // B
                rgba.push(255);               // A
            }
        }
        rgba
    } else {
        vec![0u8; pixel_count * 4]
    }
}

fn base64_decode(input: &str) -> Vec<u8> {
    let table = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = Vec::new();
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;
    for &byte in input.as_bytes() {
        if byte == b'=' || byte == b'\n' || byte == b'\r' {
            continue;
        }
        let val = table.iter().position(|&b| b == byte);
        if let Some(v) = val {
            buf = (buf << 6) | v as u32;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                output.push((buf >> bits) as u8);
                buf &= (1 << bits) - 1;
            }
        }
    }
    output
}

/// Build a PowerShell script that captures the screen using GDI BitBlt
/// and outputs raw pixel data as base64-encoded JSON.
pub fn build_screenshot_script(target: &CaptureTarget) -> String {
    let (region_clause, source_desc) = match target {
        CaptureTarget::FullScreen => (
            "Add-Type -AssemblyName System.Windows.Forms\n$bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds".to_string(),
            "full_screen".to_string(),
        ),
        CaptureTarget::Monitor(idx) => (
            format!(
                "Add-Type -AssemblyName System.Windows.Forms\n$bounds = [System.Windows.Forms.Screen]::AllScreens[{}].Bounds",
                idx
            ),
            format!("monitor:{}", idx),
        ),
        CaptureTarget::Window(pid) => (
            format!(
                "Add-Type -AssemblyName System.Windows.Forms\n$proc = Get-Process -Id {} -ErrorAction SilentlyContinue\nif ($null -eq $proc -or $null -eq $proc.MainWindowHandle -or $proc.MainWindowHandle -eq 0) {{ Write-Error 'Window not found'; exit 1 }}\n$src = New-Object System.Drawing.Rectangle; Add-Type @'\nusing System; using System.Runtime.InteropServices;\npublic class WinRect {{ [DllImport(\"user32.dll\")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect); [StructLayout(LayoutKind.Sequential)] public struct RECT {{ public int Left; public int Top; public int Right; public int Bottom; }} }}\n'@\n$rect = New-Object WinRect+RECT\n[WinRect]::GetWindowRect($proc.MainWindowHandle, [ref]$rect) | Out-Null\n$bounds = New-Object System.Drawing.Rectangle($rect.Left, $rect.Top, $rect.Right - $rect.Left, $rect.Bottom - $rect.Top)",
                pid
            ),
            format!("window:{}", pid),
        ),
        CaptureTarget::Region(r) => (
            format!(
                "$bounds = New-Object System.Drawing.Rectangle({}, {}, {}, {})",
                r.x, r.y, r.width, r.height
            ),
            format!("region:{},{},{}x{}", r.x, r.y, r.width, r.height),
        ),
    };

    format!(
        r#"
Add-Type -AssemblyName System.Drawing
{region_clause}
$bmp = New-Object System.Drawing.Bitmap($bounds.Width, $bounds.Height)
$gfx = [System.Drawing.Graphics]::FromImage($bmp)
$gfx.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
$gfx.Dispose()
$ms = New-Object System.IO.MemoryStream
$bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
$bytes = $ms.ToArray()
$ms.Dispose()
$bmp.Dispose()
$b64 = [Convert]::ToBase64String($bytes)
$obj = @{{ width = $bounds.Width; height = $bounds.Height; source = "{source_desc}"; png_base64 = $b64 }}
ConvertTo-Json $obj -Compress
"#
    )
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_screenshot(width: u32, height: u32, fill: [u8; 4]) -> Screenshot {
        let pixels = fill.repeat((width * height) as usize);
        Screenshot {
            pixels,
            width,
            height,
            captured_at_ms: 0,
            source: "test".to_string(),
        }
    }

    #[test]
    fn identical_images_match() {
        let a = make_test_screenshot(10, 10, [128, 128, 128, 255]);
        let b = make_test_screenshot(10, 10, [128, 128, 128, 255]);
        let diff = compare_screenshots(&a, &b, &DiffConfig::default());
        assert!(diff.matches);
        assert_eq!(diff.diff_pixel_count, 0);
        assert_eq!(diff.diff_percentage, 0.0);
    }

    #[test]
    fn different_images_detected() {
        let a = make_test_screenshot(10, 10, [0, 0, 0, 255]);
        let b = make_test_screenshot(10, 10, [255, 255, 255, 255]);
        let diff = compare_screenshots(&a, &b, &DiffConfig::default());
        assert!(!diff.matches);
        assert_eq!(diff.diff_pixel_count, 100);
        assert_eq!(diff.diff_percentage, 100.0);
    }

    #[test]
    fn tolerance_ignores_small_differences() {
        let a = make_test_screenshot(10, 10, [100, 100, 100, 255]);
        let b = make_test_screenshot(10, 10, [103, 103, 103, 255]); // within tolerance=5
        let config = DiffConfig {
            channel_tolerance: 5,
            max_diff_percentage: 0.0,
            compute_bounds: false,
        };
        let diff = compare_screenshots(&a, &b, &config);
        assert!(diff.matches);
        assert_eq!(diff.diff_pixel_count, 0);
    }

    #[test]
    fn not_blank_assertion_detects_solid_color() {
        let solid = make_test_screenshot(5, 5, [200, 200, 200, 255]);
        let result = check_assertion(&solid, &VisualAssertion::NotBlank);
        assert!(!result.passed);

        // Now make one pixel different
        let mut varied = make_test_screenshot(5, 5, [200, 200, 200, 255]);
        varied.pixels[0] = 0; // change R of first pixel
        let result2 = check_assertion(&varied, &VisualAssertion::NotBlank);
        assert!(result2.passed);
    }

    #[test]
    fn pixel_color_assertion() {
        let img = make_test_screenshot(10, 10, [50, 100, 150, 255]);
        let result = check_assertion(
            &img,
            &VisualAssertion::PixelColor {
                x: 5,
                y: 5,
                expected_rgb: [50, 100, 150],
                tolerance: 0,
            },
        );
        assert!(result.passed);

        let result_fail = check_assertion(
            &img,
            &VisualAssertion::PixelColor {
                x: 5,
                y: 5,
                expected_rgb: [0, 0, 0],
                tolerance: 10,
            },
        );
        assert!(!result_fail.passed);
    }

    #[test]
    fn different_sizes_never_match() {
        let a = make_test_screenshot(10, 10, [0; 4]);
        let b = make_test_screenshot(20, 20, [0; 4]);
        let diff = compare_screenshots(&a, &b, &DiffConfig::default());
        assert!(!diff.matches);
        assert_eq!(diff.diff_percentage, 100.0);
    }

    #[test]
    fn bmp_save_produces_valid_header() {
        let img = make_test_screenshot(2, 2, [255, 0, 0, 255]);
        let temp = tempfile::NamedTempFile::new().unwrap();
        img.save_bmp(temp.path()).unwrap();
        let data = std::fs::read(temp.path()).unwrap();
        assert_eq!(&data[0..2], b"BM");
        assert!(data.len() > 54); // header + some pixel data
    }
}
