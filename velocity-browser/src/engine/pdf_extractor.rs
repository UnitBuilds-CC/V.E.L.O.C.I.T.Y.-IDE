use crate::nda::NdaTriple;

pub struct PdfMediaExtractor;

impl PdfMediaExtractor {
    pub fn parse_pdf_document(pdf_bytes: &[u8]) -> Vec<String> {
        let mut text_lines = Vec::new();
        text_lines.push(format!("Parsed PDF document of {} bytes", pdf_bytes.len()));
        text_lines
    }

    pub fn export_pdf_nda(session_id: &str, lines: &[String]) -> Vec<NdaTriple> {
        let mut triples = Vec::new();
        for (idx, line) in lines.iter().enumerate() {
            triples.push(NdaTriple::new(session_id, 251, &format!("pdf_text_{}:{}", idx, line)));
        }
        triples
    }
}
