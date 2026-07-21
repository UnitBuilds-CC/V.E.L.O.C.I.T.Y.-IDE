#[derive(Debug, Clone)]
pub struct GlyphMetric {
    pub glyph_id: u32,
    pub advance_width: f32,
    pub left_side_bearing: f32,
}

pub struct FontShaperEngine {
    pub font_family: String,
    pub glyph_cache: Vec<GlyphMetric>,
}

impl FontShaperEngine {
    pub fn new(font_family: &str) -> Self {
        Self {
            font_family: font_family.to_string(),
            glyph_cache: Vec::new(),
        }
    }

    pub fn shape_text(&mut self, text: &str) -> Vec<GlyphMetric> {
        text.chars().enumerate().map(|(idx, _ch)| GlyphMetric {
            glyph_id: idx as u32 + 32,
            advance_width: 8.5,
            left_side_bearing: 0.5,
        }).collect()
    }
}
