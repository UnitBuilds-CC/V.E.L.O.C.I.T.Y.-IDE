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
}

/// A PDF document with extracted pages and metadata.
#[derive(Debug, Clone)]
pub struct PdfDocument {
    pub pages: Vec<PdfPage>,
    pub title: Option<String>,
    pub author: Option<String>,
    pub creator: Option<String>,
    pub page_count: usize,
}

impl PdfMediaExtractor {
    /// Parse PDF document from bytes. Extracts text content from each page.
    /// This is a simplified parser that handles basic PDF text streams.
    pub fn parse_pdf_document(pdf_bytes: &[u8]) -> PdfDocument {
        let mut pages = Vec::new();
        let mut title = None;
        let mut author = None;
        let mut creator = None;

        // Extract text between BT/ET markers (basic PDF text objects)
        let content = String::from_utf8_lossy(pdf_bytes);
        let mut current_page = 1;
        let mut page_lines = Vec::new();

        for line in content.lines() {
            let trimmed = line.trim();
            // Detect page objects
            if trimmed.starts_with("/Type /Page") && !trimmed.contains("/Pages") {
                if !page_lines.is_empty() {
                    pages.push(PdfPage {
                        page_number: current_page,
                        text_lines: page_lines.clone(),
                        width: 612.0,
                        height: 792.0,
                        fonts: Vec::new(),
                    });
                    current_page += 1;
                    page_lines.clear();
                }
            }
            // Extract text from Tj/TJ operators
            if trimmed.contains(" Tj") || trimmed.contains(" TJ") {
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
        }

        // Push last page if any
        if !page_lines.is_empty() || pages.is_empty() {
            pages.push(PdfPage {
                page_number: current_page,
                text_lines: page_lines,
                width: 612.0,
                height: 792.0,
                fonts: Vec::new(),
            });
        }

        let page_count = pages.len();
        PdfDocument { pages, title, author, creator, page_count }
    }

    /// Extract text lines from a parsed document.
    pub fn extract_text_lines(doc: &PdfDocument) -> Vec<String> {
        doc.pages.iter()
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
        triples
    }
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
        let text: String = hex_content.chars()
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
