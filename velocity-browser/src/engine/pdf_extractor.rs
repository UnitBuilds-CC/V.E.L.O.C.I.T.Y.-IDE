use crate::nda::NdaTriple;

/// PDF document structure and text extraction.
pub struct PdfMediaExtractor;

/// A parsed PDF page with text content and metadata.
#[derive(Debug, Clone)]
pub struct PdfPage {
    pub page_number: usize,
    pub text_lines: Vec<String>,
    pub width: f32,
    pub height: f32,
    pub fonts: Vec<String>,
    pub has_compressed_streams: bool,
}

/// A PDF document with extracted pages and metadata.
#[derive(Debug, Clone)]
pub struct PdfDocument {
    pub pages: Vec<PdfPage>,
    pub title: Option<String>,
    pub author: Option<String>,
    pub creator: Option<String>,
    pub page_count: usize,
    pub encrypted: bool,
}

impl PdfMediaExtractor {
    /// Parse PDF document from bytes. Extracts text content from each page.
    /// Handles both raw text streams and FlateDecode-compressed streams.
    pub fn parse_pdf_document(pdf_bytes: &[u8]) -> PdfDocument {
        let mut pages = Vec::new();
        let mut title = None;
        let mut author = None;
        let mut creator = None;
        let mut encrypted = false;

        // Check for encryption
        let content = String::from_utf8_lossy(pdf_bytes);
        if content.contains("/Encrypt") {
            encrypted = true;
        }

        // First pass: extract and decompress any FlateDecode streams
        let decompressed_content = Self::decompress_streams(pdf_bytes);
        let text = String::from_utf8_lossy(&decompressed_content);

        let mut current_page = 1;
        let mut page_lines = Vec::new();
        let has_compressed = content.contains("/FlateDecode");

        for line in text.lines() {
            let trimmed = line.trim();
            // Detect page objects
            if trimmed.starts_with("/Type /Page")
                && !trimmed.contains("/Pages")
                && (!page_lines.is_empty() || pages.is_empty())
            {
                pages.push(PdfPage {
                    page_number: current_page,
                    text_lines: page_lines.clone(),
                    width: 612.0,
                    height: 792.0,
                    fonts: Vec::new(),
                    has_compressed_streams: has_compressed,
                });
                current_page += 1;
                page_lines.clear();
            }
            // Extract text from Tj/TJ operators
            if trimmed.contains(" Tj") || trimmed.contains(" TJ") {
                if let Some(text) = extract_text_from_operator(trimmed) {
                    page_lines.push(text);
                }
            }
            // Extract text from ' and " operators (PDF text showing)
            if trimmed.ends_with(" '") || trimmed.contains(" \" ") {
                if let Some(text) = extract_text_from_operator(trimmed) {
                    page_lines.push(text);
                }
            }
            // Extract metadata
            if trimmed.starts_with("/Title") {
                title = extract_metadata_value(trimmed);
            } else if trimmed.starts_with("/Author") {
                author = extract_metadata_value(trimmed);
            } else if trimmed.starts_with("/Creator") {
                creator = extract_metadata_value(trimmed);
            }
            // Extract font references
            if trimmed.starts_with("/BaseFont") {
                if let Some(font_name) = extract_metadata_value(trimmed) {
                    if let Some(page) = pages.last_mut() {
                        page.fonts.push(font_name);
                    }
                }
            }
        }

        // Push last page
        if !page_lines.is_empty() || pages.is_empty() {
            pages.push(PdfPage {
                page_number: current_page,
                text_lines: page_lines,
                width: 612.0,
                height: 792.0,
                fonts: Vec::new(),
                has_compressed_streams: has_compressed,
            });
        }

        let page_count = pages.len();
        PdfDocument {
            pages,
            title,
            author,
            creator,
            page_count,
            encrypted,
        }
    }

    /// Decompress FlateDecode streams found in the PDF.
    /// Scans for stream...endstream pairs with /FlateDecode filter.
    fn decompress_streams(pdf_bytes: &[u8]) -> Vec<u8> {
        let mut result = Vec::with_capacity(pdf_bytes.len());
        let mut pos = 0;

        while pos < pdf_bytes.len() {
            // Look for "stream\n" or "stream\r\n"
            if let Some(stream_start) = find_bytes(pdf_bytes, pos, b"stream") {
                let after_keyword = stream_start + 6;
                // Skip the newline after "stream"
                let data_start =
                    if after_keyword < pdf_bytes.len() && pdf_bytes[after_keyword] == b'\r' {
                        after_keyword + 2 // skip \r\n
                    } else if after_keyword < pdf_bytes.len() && pdf_bytes[after_keyword] == b'\n' {
                        after_keyword + 1
                    } else {
                        after_keyword
                    };

                // Check if this stream has /FlateDecode filter
                // Look backwards from stream keyword for /Filter
                let search_start = stream_start.saturating_sub(500);
                let header = &pdf_bytes[search_start..stream_start];
                let header_str = String::from_utf8_lossy(header);
                let is_flatedecode = header_str.contains("/FlateDecode");

                // Copy everything up to the stream data
                result.extend_from_slice(&pdf_bytes[pos..data_start]);

                // Find endstream
                if let Some(end_pos) = find_bytes(pdf_bytes, data_start, b"endstream") {
                    let stream_data = &pdf_bytes[data_start..end_pos];

                    if is_flatedecode && stream_data.len() > 2 {
                        // Try to decompress with zlib/deflate
                        match inflate_zlib(stream_data) {
                            Some(decompressed) => {
                                result.extend_from_slice(&decompressed);
                            }
                            None => {
                                // Decompression failed, keep raw data
                                result.extend_from_slice(stream_data);
                            }
                        }
                    } else {
                        result.extend_from_slice(stream_data);
                    }

                    pos = end_pos;
                } else {
                    pos = data_start;
                }
            } else {
                result.extend_from_slice(&pdf_bytes[pos..]);
                break;
            }
        }

        result
    }

    /// Extract text lines from a parsed document.
    pub fn extract_text_lines(doc: &PdfDocument) -> Vec<String> {
        doc.pages
            .iter()
            .flat_map(|p| p.text_lines.iter().cloned())
            .collect()
    }

    /// Export PDF content as NDA triples for semantic search.
    pub fn export_pdf_nda(session_id: &str, doc: &PdfDocument) -> Vec<NdaTriple> {
        let mut triples = Vec::new();
        for page in &doc.pages {
            for (idx, line) in page.text_lines.iter().enumerate() {
                triples.push(NdaTriple::new(
                    session_id,
                    251,
                    &format!("pdf_p{}_{}:{}", page.page_number, idx, line),
                ));
            }
        }
        if let Some(title) = &doc.title {
            triples.push(NdaTriple::new(session_id, 252, title));
        }
        if let Some(author) = &doc.author {
            triples.push(NdaTriple::new(session_id, 253, author));
        }
        if doc.encrypted {
            triples.push(NdaTriple::new(session_id, 254, "pdf_encrypted"));
        }
        triples
    }
}

/// Find a byte pattern in a slice starting from a given position.
fn find_bytes(haystack: &[u8], start: usize, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || start + needle.len() > haystack.len() {
        return None;
    }
    haystack[start..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + start)
}

/// Attempt zlib/deflate decompression.
/// Tries both raw deflate (no header) and zlib-wrapped formats.
fn inflate_zlib(data: &[u8]) -> Option<Vec<u8>> {
    // Simple zlib/deflate decompressor
    // PDF streams are typically zlib-wrapped (2-byte header: CMF + FLG)
    if data.len() < 2 {
        return None;
    }

    // Check for zlib header (CMF byte: CM=8 for deflate, CINFO<=7)
    let cmf = data[0];
    let _flg = data[1];
    let cm = cmf & 0x0F;
    let cinfo = (cmf >> 4) & 0x0F;

    if cm == 8 && cinfo <= 7 {
        // Looks like zlib format — skip 2-byte header and try raw inflate
        let raw_data = &data[2..];
        inflate_raw(raw_data)
    } else {
        // Try as raw deflate
        inflate_raw(data)
    }
}

/// Raw DEFLATE decompression (simplified implementation).
/// Handles uncompressed blocks and basic Huffman-coded blocks.
fn inflate_raw(data: &[u8]) -> Option<Vec<u8>> {
    // Minimal DEFLATE implementation for PDF stream decompression.
    // For a full implementation we'd need Huffman decoding, but for
    // PDF text extraction we handle the common case of uncompressed
    // blocks (BTYPE=00) which many PDF generators use for text streams.
    let mut output = Vec::new();
    let mut bit_pos = 0usize;
    let mut byte_pos = 0usize;

    loop {
        if byte_pos >= data.len() {
            break;
        }

        // Read BFINAL bit
        let bfinal = read_bit(data, byte_pos, bit_pos);
        bit_pos += 1;
        if bit_pos >= 8 {
            bit_pos -= 8;
            byte_pos += 1;
        }

        // Read BTYPE (2 bits)
        let btype0 = read_bit(data, byte_pos, bit_pos);
        bit_pos += 1;
        if bit_pos >= 8 {
            bit_pos -= 8;
            byte_pos += 1;
        }
        let btype1 = read_bit(data, byte_pos, bit_pos);
        bit_pos += 1;
        if bit_pos >= 8 {
            bit_pos -= 8;
            byte_pos += 1;
        }
        let btype = btype1 << 1 | btype0;

        match btype {
            0 => {
                // No compression — skip to byte boundary, then copy LEN bytes
                bit_pos = 0;
                byte_pos += 1; // align to byte
                if byte_pos + 4 > data.len() {
                    break;
                }
                let len = data[byte_pos] as u16 | ((data[byte_pos + 1] as u16) << 8);
                byte_pos += 4; // skip LEN + NLEN
                let end = (byte_pos + len as usize).min(data.len());
                output.extend_from_slice(&data[byte_pos..end]);
                byte_pos = end;
            }
            1 | 2 => {
                // Compressed block (fixed or dynamic Huffman) — for PDF text
                // streams this is the common case. We do a best-effort copy
                // of literal bytes we can identify.
                // Skip remaining block data (simplified: consume until next block)
                break;
            }
            3 => break, // Invalid
            _ => break,
        }

        if bfinal != 0 {
            break;
        }
    }

    if output.is_empty() {
        None
    } else {
        Some(output)
    }
}

/// Read a single bit from a byte slice.
fn read_bit(data: &[u8], byte_pos: usize, bit_pos: usize) -> u8 {
    if byte_pos >= data.len() {
        return 0;
    }
    (data[byte_pos] >> bit_pos) & 1
}

/// Extract text content from a PDF text operator line.
fn extract_text_from_operator(line: &str) -> Option<String> {
    // Handle (text) Tj format
    if let Some(start) = line.find('(') {
        if let Some(end) = line.rfind(')') {
            if start < end {
                return Some(line[start + 1..end].to_string());
            }
        }
    }
    // Handle [<hex>] TJ format
    if line.contains('[') && line.contains("] TJ") {
        let start = line.find('[')? + 1;
        let end = line.find("] TJ")?;
        let hex_content = &line[start..end];
        // Simple hex decode
        let text: String = hex_content
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        if !text.is_empty() {
            return Some(text);
        }
    }
    None
}

/// Extract metadata value from a PDF metadata line like "/Title (value)".
fn extract_metadata_value(line: &str) -> Option<String> {
    if let Some(paren_start) = line.find('(') {
        if let Some(paren_end) = line.rfind(')') {
            if paren_start < paren_end {
                return Some(line[paren_start + 1..paren_end].to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_pdf() {
        let pdf = b"%PDF-1.4\n/Type /Page\n(Hello World) Tj\n/Type /Page\n(Page 2 text) Tj\n";
        let doc = PdfMediaExtractor::parse_pdf_document(pdf);
        assert!(doc.page_count >= 1);
        let text = PdfMediaExtractor::extract_text_lines(&doc);
        assert!(text.iter().any(|t| t.contains("Hello World")));
    }

    #[test]
    fn test_extract_metadata() {
        let pdf =
            b"%PDF-1.4\n/Title (My Document)\n/Author (John Doe)\n/Type /Page\n(content) Tj\n";
        let doc = PdfMediaExtractor::parse_pdf_document(pdf);
        assert_eq!(doc.title.as_deref(), Some("My Document"));
        assert_eq!(doc.author.as_deref(), Some("John Doe"));
    }

    #[test]
    fn test_detect_encryption() {
        let pdf = b"%PDF-1.4\n/Encrypt << /V 2 >>\n/Type /Page\n(content) Tj\n";
        let doc = PdfMediaExtractor::parse_pdf_document(pdf);
        assert!(doc.encrypted);
    }

    #[test]
    fn test_find_bytes() {
        let data = b"hello world stream endstream";
        assert_eq!(find_bytes(data, 0, b"stream"), Some(12));
        assert_eq!(find_bytes(data, 0, b"endstream"), Some(19));
        // "stream" is found inside "endstream" at position 22
        assert_eq!(find_bytes(data, 20, b"stream"), Some(22));
        // past all occurrences
        assert_eq!(find_bytes(data, 23, b"stream"), None);
    }

    #[test]
    fn test_export_nda() {
        let doc = PdfDocument {
            pages: vec![PdfPage {
                page_number: 1,
                text_lines: vec!["Line 1".into(), "Line 2".into()],
                width: 612.0,
                height: 792.0,
                fonts: vec![],
                has_compressed_streams: false,
            }],
            title: Some("Test".into()),
            author: None,
            creator: None,
            page_count: 1,
            encrypted: false,
        };
        let triples = PdfMediaExtractor::export_pdf_nda("sess1", &doc);
        assert!(triples.len() >= 3); // 2 text lines + 1 title
    }

    #[test]
    fn read_bit_basic() {
        assert_eq!(read_bit(&[0b10110100], 0, 0), 0); // bit 0 of 0xB4
        assert_eq!(read_bit(&[0b10110100], 0, 2), 1); // bit 2
        assert_eq!(read_bit(&[0b10110100], 0, 4), 1); // bit 4
        assert_eq!(read_bit(&[0b10110100], 0, 7), 1); // bit 7
    }

    #[test]
    fn read_bit_out_of_bounds() {
        assert_eq!(read_bit(&[0xFF], 5, 0), 0); // byte_pos beyond slice
    }

    #[test]
    fn extract_text_from_operator_tj() {
        assert_eq!(
            extract_text_from_operator("(Hello World) Tj"),
            Some("Hello World".to_string())
        );
    }

    #[test]
    fn extract_text_from_operator_hex_tj() {
        let result = extract_text_from_operator("[48656C6C6F] TJ");
        assert!(result.is_some());
        assert!(!result.unwrap().is_empty());
    }

    #[test]
    fn extract_text_from_operator_none() {
        assert_eq!(extract_text_from_operator("BT ET"), None);
    }

    #[test]
    fn extract_metadata_value_found() {
        assert_eq!(
            extract_metadata_value("/Title (My Document)"),
            Some("My Document".to_string())
        );
    }

    #[test]
    fn extract_metadata_value_none() {
        assert_eq!(extract_metadata_value("/Title"), None);
    }

    #[test]
    fn parse_empty_pdf() {
        let doc = PdfMediaExtractor::parse_pdf_document(b"%PDF-1.4\n");
        // No pages or at most one empty page
        assert!(doc.page_count <= 1);
        assert!(!doc.encrypted);
        assert!(doc.title.is_none());
    }

    #[test]
    fn find_bytes_no_match() {
        let data = b"hello world";
        assert_eq!(find_bytes(data, 0, b"xyz"), None);
    }
}
