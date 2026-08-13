/// A single shaped glyph with metrics for layout and rendering.
#[derive(Debug, Clone)]
pub struct GlyphMetric {
    pub glyph_id: u32,
    pub advance_width: f32,
    pub left_side_bearing: f32,
    /// Height above the baseline.
    pub ascent: f32,
    /// Depth below the baseline (positive downward).
    pub descent: f32,
    /// Unicode codepoint this glyph represents.
    pub codepoint: char,
}

/// Font metrics for a given font face.
#[derive(Debug, Clone)]
pub struct FontMetrics {
    pub units_per_em: u32,
    pub ascent: f32,
    pub descent: f32,
    pub line_gap: f32,
    pub x_height: f32,
    pub cap_height: f32,
}

/// Character width class for proportional font simulation.
#[derive(Debug, Clone, Copy)]
enum CharWidthClass {
    Narrow,     // i, l, t, f, j, etc.
    Normal,     // a, b, c, d, e, etc.
    Wide,       // m, w, M, W, etc.
    Cjk,        // CJK Unified Ideographs
    Whitespace, // space, tab
    Zero,       // zero-width chars
}

/// Font shaping engine with character-class-based metrics and kerning.
pub struct FontShaperEngine {
    pub font_family: String,
    pub glyph_cache: Vec<GlyphMetric>,
    pub font_size: f32,
    pub metrics: FontMetrics,
}

impl FontShaperEngine {
    pub fn new(font_family: &str) -> Self {
        Self {
            font_family: font_family.to_string(),
            glyph_cache: Vec::new(),
            font_size: 16.0,
            metrics: FontMetrics {
                units_per_em: 1000,
                ascent: 0.8,
                descent: 0.2,
                line_gap: 0.1,
                x_height: 0.5,
                cap_height: 0.7,
            },
        }
    }

    /// Create a font shaper with a specific size.
    pub fn with_size(font_family: &str, font_size: f32) -> Self {
        let mut shaper = Self::new(font_family);
        shaper.font_size = font_size;
        shaper
    }

    /// Classify a character into a width class for proportional metrics.
    fn classify_char(ch: char) -> CharWidthClass {
        if ch == ' ' || ch == '\t' {
            return CharWidthClass::Whitespace;
        }
        if ch == '\u{200B}' || ch == '\u{200C}' || ch == '\u{200D}' || ch == '\u{FEFF}' {
            return CharWidthClass::Zero;
        }
        // CJK ranges
        if matches!(ch, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{F900}'..='\u{FAFF}' | '\u{3000}'..='\u{303F}' | '\u{FF00}'..='\u{FFEF}')
        {
            return CharWidthClass::Cjk;
        }
        match ch {
            'i' | 'l' | 't' | 'f' | 'j' | 'r' | 'I' | '1' | '|' | '!' | '.' | ',' | ';' | ':'
            | '\'' => CharWidthClass::Narrow,
            'm' | 'w' | 'M' | 'W' | 'q' | 'Q' | '@' | '#' | '%' | '&' => CharWidthClass::Wide,
            _ => CharWidthClass::Normal,
        }
    }

    /// Get the advance width for a character based on its width class.
    fn advance_for_char(ch: char, font_size: f32) -> f32 {
        let base = font_size * 0.55; // approximate average character width
        match Self::classify_char(ch) {
            CharWidthClass::Narrow => base * 0.5,
            CharWidthClass::Normal => base,
            CharWidthClass::Wide => base * 1.4,
            CharWidthClass::Cjk => font_size, // full-width
            CharWidthClass::Whitespace => base * 0.3,
            CharWidthClass::Zero => 0.0,
        }
    }

    /// Shape text into glyph metrics with proportional widths and kerning.
    pub fn shape_text(&mut self, text: &str) -> Vec<GlyphMetric> {
        let chars: Vec<char> = text.chars().collect();
        let mut glyphs = Vec::with_capacity(chars.len());

        for (idx, &ch) in chars.iter().enumerate() {
            let advance = Self::advance_for_char(ch, self.font_size);
            let glyph_id = ch as u32;

            // Apply kerning adjustment based on character pairs
            let kerning = if idx > 0 {
                Self::kerning_pair(chars[idx - 1], ch, self.font_size)
            } else {
                0.0
            };

            let lsb = match Self::classify_char(ch) {
                CharWidthClass::Narrow => self.font_size * 0.05,
                CharWidthClass::Normal => self.font_size * 0.08,
                CharWidthClass::Wide => self.font_size * 0.04,
                CharWidthClass::Cjk => 0.0,
                CharWidthClass::Whitespace => 0.0,
                CharWidthClass::Zero => 0.0,
            };

            let ascent = self.metrics.ascent * self.font_size;
            let descent = self.metrics.descent * self.font_size;

            glyphs.push(GlyphMetric {
                glyph_id,
                advance_width: advance + kerning,
                left_side_bearing: lsb,
                ascent,
                descent,
                codepoint: ch,
            });
        }

        self.glyph_cache = glyphs.clone();
        glyphs
    }

    /// Simulated kerning adjustment for common character pairs.
    fn kerning_pair(left: char, right: char, font_size: f32) -> f32 {
        let base = font_size * -0.03; // slight negative kern
        match (left, right) {
            ('A', 'V') | ('A', 'W') | ('A', 'Y') | ('V', 'A') | ('W', 'A') | ('Y', 'A') => {
                base * 1.5
            }
            ('T', 'a') | ('T', 'e') | ('T', 'o') | ('V', 'a') | ('V', 'e') | ('W', 'a') => {
                base * 1.2
            }
            ('f', 'i') | ('f', 'l') | ('f', 't') => base * 0.8,
            ('r', 'n') | ('r', 'm') | ('r', 'v') => base * 0.6,
            _ => 0.0,
        }
    }

    /// Compute the total advance width of a shaped text run.
    pub fn measure_text_width(&mut self, text: &str) -> f32 {
        let glyphs = self.shape_text(text);
        glyphs.iter().map(|g| g.advance_width).sum()
    }

    /// Compute line height for the current font size.
    pub fn line_height(&self) -> f32 {
        self.font_size * (self.metrics.ascent + self.metrics.descent + self.metrics.line_gap)
    }

    /// Get the baseline offset from the top of the line box.
    pub fn baseline_offset(&self) -> f32 {
        self.font_size * self.metrics.ascent
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shape_basic_text() {
        let mut shaper = FontShaperEngine::new("Roboto");
        let glyphs = shaper.shape_text("Hello");
        assert_eq!(glyphs.len(), 5);
        assert_eq!(glyphs[0].codepoint, 'H');
        assert_eq!(glyphs[4].codepoint, 'o');
    }

    #[test]
    fn test_proportional_widths() {
        let mut shaper = FontShaperEngine::new("Roboto");
        let glyphs = shaper.shape_text("il mw");
        // 'i' and 'l' should be narrower than 'm' and 'w'
        let i_width = glyphs[0].advance_width;
        let m_width = glyphs[3].advance_width; // 'm' is at index 3 (after 'i','l',' ')
        assert!(
            i_width < m_width,
            "Narrow char 'i' ({}) should be less than wide char 'm' ({})",
            i_width,
            m_width
        );
    }

    #[test]
    fn test_cjk_fullwidth() {
        let mut shaper = FontShaperEngine::new("Roboto");
        let glyphs = shaper.shape_text("\u{6f22}\u{5b57}");
        // CJK characters should be full-width (= font_size)
        assert!((glyphs[0].advance_width - 16.0).abs() < 0.01);
    }

    #[test]
    fn test_whitespace_width() {
        let mut shaper = FontShaperEngine::new("Roboto");
        let glyphs = shaper.shape_text("a b");
        let space_width = glyphs[1].advance_width;
        let a_width = glyphs[0].advance_width;
        assert!(space_width < a_width, "Space should be narrower than 'a'");
    }

    #[test]
    fn test_kerning() {
        let mut shaper = FontShaperEngine::new("Roboto");
        let glyphs_av = shaper.shape_text("AV");
        let glyphs_ab = shaper.shape_text("AB");
        // AV should have kerning applied (narrower total)
        let av_total: f32 = glyphs_av.iter().map(|g| g.advance_width).sum();
        let ab_total: f32 = glyphs_ab.iter().map(|g| g.advance_width).sum();
        assert!(
            av_total < ab_total,
            "AV ({}) should be narrower than AB ({}) due to kerning",
            av_total,
            ab_total
        );
    }

    #[test]
    fn test_measure_width() {
        let mut shaper = FontShaperEngine::new("Roboto");
        let width = shaper.measure_text_width("Hello World");
        assert!(width > 0.0);
    }

    #[test]
    fn test_line_height() {
        let shaper = FontShaperEngine::with_size("Roboto", 20.0);
        let lh = shaper.line_height();
        assert!(lh > 20.0, "Line height should exceed font size");
    }

    #[test]
    fn test_ascent_descent() {
        let mut shaper = FontShaperEngine::new("Roboto");
        let glyphs = shaper.shape_text("A");
        assert!(glyphs[0].ascent > 0.0);
        assert!(glyphs[0].descent > 0.0);
    }

    #[test]
    fn test_zero_width_chars() {
        let mut shaper = FontShaperEngine::new("Roboto");
        let glyphs = shaper.shape_text("a\u{200B}b");
        assert_eq!(glyphs[1].advance_width, 0.0);
    }

    #[test]
    fn test_custom_size() {
        let mut shaper = FontShaperEngine::with_size("Mono", 32.0);
        let glyphs = shaper.shape_text("A");
        assert!((glyphs[0].ascent - 32.0 * 0.8).abs() < 0.01);
    }
}
