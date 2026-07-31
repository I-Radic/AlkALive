//! Text input field — an editable text buffer rendered by the AlkALive text stack.
//!
//! This module implements a simplified in-WASM text input field that:
//! 1. Holds an editable text buffer (String) with a cursor position.
//! 2. Supports character insertion, backspace, delete, and cursor movement.
//! 3. Renders the text + a blinking cursor via the real HarfRust text stack.
//! 4. Has a focus state — only focused input receives keyboard input.
//!
//! This is a pragmatic implementation of ADR 023's goal (text input rendered
//! by AlkALive, not the DOM). It bypasses the full IME composition pipeline
//! (which requires the hidden `<input>` exception and `compositionstart`/
//! `compositionupdate`/`compositionend` events) and instead accepts characters
//! directly via `insert_char`. This is sufficient for ASCII text input and
//! demonstrates the concept. Full IME support can be layered in later.

use alkalive_text::{
    FontRegistry, GlyphAtlas, GlyphKey, HarfRustFontRegistry, HarfRustGlyphAtlas,
    HarfRustTextShaper, ShapeContext, ShapedRun, TextShaper,
};
use std::sync::Arc;

use crate::renderer::{ColorMode, PositionedGlyph};
use crate::text_scene::FONT_BYTES;

/// Input field font size (smaller than the title).
pub const INPUT_FONT_SIZE_PX: f32 = 28.0;

/// Maximum input field text length (to prevent unbounded growth).
pub const MAX_INPUT_LEN: usize = 200;

/// Cursor blink period in seconds.
pub const CURSOR_BLINK_PERIOD: f32 = 1.06;

/// An editable text input field rendered by the AlkALive text stack.
pub struct InputField {
    /// The text buffer.
    pub text: String,
    /// Cursor position (byte offset into `text`).
    pub cursor: usize,
    /// Whether this field is focused (receives keyboard input).
    pub focused: bool,
    /// Placeholder text shown when the buffer is empty.
    pub placeholder: String,
    /// The font ID used for shaping.
    font_id: alkalive_text::FontId,
    /// Cached shaped run of the current text (None if needs re-shape).
    cached_shaped: Option<ShapedRun>,
    /// Whether the shaped run is dirty (text or cursor changed).
    dirty: bool,
}

impl InputField {
    /// Create a new input field with the given placeholder.
    pub fn new(placeholder: &str) -> Self {
        // Load font to get a valid FontId. We load it here to keep the input
        // field self-contained; the font registry is cheap to construct.
        let mut registry = HarfRustFontRegistry::new();
        let loaded_id = registry.load_bundle(FONT_BYTES).ok();
        let font_id = loaded_id.unwrap_or(alkalive_text::FontId(0));
        Self {
            text: String::new(),
            cursor: 0,
            focused: false,
            placeholder: placeholder.to_string(),
            font_id,
            cached_shaped: None,
            dirty: true,
        }
    }

    /// Insert a character at the cursor position.
    pub fn insert_char(&mut self, c: char) {
        if self.text.len() >= MAX_INPUT_LEN {
            return;
        }
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
        self.dirty = true;
    }

    /// Insert a string at the cursor position.
    pub fn insert_str(&mut self, s: &str) {
        let remaining = MAX_INPUT_LEN.saturating_sub(self.text.len());
        if remaining == 0 {
            return;
        }
        let to_insert = if s.len() > remaining {
            &s[..remaining]
        } else {
            s
        };
        self.text.insert_str(self.cursor, to_insert);
        self.cursor += to_insert.len();
        self.dirty = true;
    }

    /// Delete the character before the cursor (backspace).
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        // Find the start of the previous UTF-8 character.
        let prev_char_start = self.text[..self.cursor]
            .char_indices()
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0);
        self.text.replace_range(prev_char_start..self.cursor, "");
        self.cursor = prev_char_start;
        self.dirty = true;
    }

    /// Delete the character after the cursor (forward delete).
    pub fn delete_forward(&mut self) {
        if self.cursor >= self.text.len() {
            return;
        }
        let next_char_end = self.text[self.cursor..]
            .char_indices()
            .nth(1)
            .map(|(i, _)| self.cursor + i)
            .unwrap_or(self.text.len());
        self.text.replace_range(self.cursor..next_char_end, "");
        self.dirty = true;
    }

    /// Move the cursor left by one character.
    pub fn cursor_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev_start = self.text[..self.cursor]
            .char_indices()
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0);
        self.cursor = prev_start;
    }

    /// Move the cursor right by one character.
    pub fn cursor_right(&mut self) {
        if self.cursor >= self.text.len() {
            return;
        }
        let next_end = self.text[self.cursor..]
            .char_indices()
            .nth(1)
            .map(|(i, _)| self.cursor + i)
            .unwrap_or(self.text.len());
        self.cursor = next_end;
    }

    /// Move the cursor to the start (Home key).
    pub fn cursor_home(&mut self) {
        self.cursor = 0;
    }

    /// Move the cursor to the end (End key).
    pub fn cursor_end(&mut self) {
        self.cursor = self.text.len();
    }

    /// Clear all text.
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.dirty = true;
    }

    /// Set focus state.
    pub fn set_focus(&mut self, focused: bool) {
        self.focused = focused;
    }

    /// Mark the shaped run as dirty (needs re-shaping on next render).
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Toggle focus.
    pub fn toggle_focus(&mut self) {
        self.focused = !self.focused;
    }

    /// Get the text to display (placeholder if empty).
    fn display_text(&self) -> &str {
        if self.text.is_empty() {
            &self.placeholder
        } else {
            &self.text
        }
    }

    /// Whether the displayed text is the placeholder.
    fn is_showing_placeholder(&self) -> bool {
        self.text.is_empty()
    }

    /// Shape the current text and return positioned glyphs.
    ///
    /// This uses the provided atlas (shared with the title scene) to
    /// rasterize glyphs. Returns the positioned glyphs and the total width.
    pub fn get_positioned_glyphs(
        &mut self,
        registry: &Arc<HarfRustFontRegistry>,
        atlas: &mut HarfRustGlyphAtlas,
    ) -> (Vec<PositionedGlyph>, f32) {
        let display = self.display_text();
        if display.is_empty() {
            self.cached_shaped = None;
            self.dirty = false;
            return (Vec::new(), 0.0);
        }

        // Re-shape if dirty or no cached run.
        if self.dirty || self.cached_shaped.is_none() {
            let shaper = HarfRustTextShaper::new(Arc::clone(registry));
            let ctx = ShapeContext {
                font: self.font_id,
                size_px: INPUT_FONT_SIZE_PX,
                direction: None,
            };
            match shaper.shape(display, &ctx) {
                Ok(run) => {
                    self.cached_shaped = Some(run);
                    self.dirty = false;
                }
                Err(_) => {
                    return (Vec::new(), 0.0);
                }
            }
        }

        let run = self.cached_shaped.as_ref().unwrap();
        let total_width = run.metrics.total_advance;

        // Rasterize glyphs and position them.
        let mut glyphs = Vec::with_capacity(run.glyph_ids.len());
        let mut pen_x = 0.0f32;

        for (i, &glyph_id) in run.glyph_ids.iter().enumerate() {
            let key = GlyphKey {
                font_id: run.font_id,
                glyph_id,
                phase: 0,
                size_px: INPUT_FONT_SIZE_PX as u16,
            };

            let slot = atlas.ensure(key);

            if slot.size.0 < 0.5 || slot.size.1 < 0.5 {
                pen_x += run.advances[i];
                continue;
            }

            let offset_x = run.offsets[i].0;
            let offset_y = run.offsets[i].1;

            let x = pen_x + offset_x + slot.bearing.0;
            let y = offset_y - slot.bearing.1;

            glyphs.push(PositionedGlyph {
                page: slot.page,
                uv_x: slot.uv.x,
                uv_y: slot.uv.y,
                uv_w: slot.uv.w,
                uv_h: slot.uv.h,
                x,
                y,
                w: slot.size.0,
                h: slot.size.1,
            });

            pen_x += run.advances[i];
        }

        (glyphs, total_width)
    }

    /// Get the cursor X position in text-space pixels (relative to text start).
    ///
    /// This is the sum of advances for all glyphs before the cursor byte offset.
    pub fn cursor_x(&self) -> f32 {
        if self.cached_shaped.is_none() || self.text.is_empty() {
            return 0.0;
        }
        let run = self.cached_shaped.as_ref().unwrap();

        // Map cursor byte offset to glyph index using cluster info.
        // HarfRust's cluster is the byte offset of the start of the character
        // that produced this glyph.
        let mut cursor_x = 0.0f32;
        for (i, &cluster) in run.clusters.iter().enumerate() {
            if cluster as usize >= self.cursor {
                break;
            }
            cursor_x += run.advances[i];
        }
        cursor_x
    }

    /// Whether the displayed text is the placeholder.
    pub fn is_placeholder(&self) -> bool {
        self.is_showing_placeholder()
    }

    /// Get the input text content.
    pub fn get_text(&self) -> &str {
        &self.text
    }

    /// Get the cursor position (byte offset).
    pub fn get_cursor(&self) -> usize {
        self.cursor
    }
}

/// Determine if a character should be inserted into the input field.
/// Returns true for printable characters (letters, digits, punctuation, space).
pub fn is_printable_char(c: char) -> bool {
    // Exclude control characters, but include space and common punctuation.
    !c.is_control() && c != '\t' && c != '\n' && c != '\r'
}

/// The color mode for the input field text.
/// When showing placeholder, use a dim gray. When focused with text, use white.
/// When unfocused with text, use light gray.
pub fn input_color_mode(field: &InputField) -> ColorMode {
    if field.is_placeholder() {
        // Dim gray placeholder.
        ColorMode::Solid(90, 90, 100)
    } else if field.focused {
        // Bright white when focused.
        ColorMode::Solid(240, 240, 255)
    } else {
        // Light gray when unfocused.
        ColorMode::Solid(180, 180, 190)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_field_starts_empty() {
        let field = InputField::new("Type here...");
        assert!(field.text.is_empty());
        assert_eq!(field.cursor, 0);
        assert!(!field.focused);
        assert!(field.is_placeholder());
    }

    #[test]
    fn insert_char_appends_text() {
        let mut field = InputField::new("placeholder");
        field.insert_char('H');
        field.insert_char('i');
        assert_eq!(field.text, "Hi");
        assert_eq!(field.cursor, 2);
    }

    #[test]
    fn insert_char_in_middle() {
        let mut field = InputField::new("");
        field.insert_str("Hello");
        field.cursor = 2; // After "He"
        field.insert_char('X');
        assert_eq!(field.text, "HeXllo");
        assert_eq!(field.cursor, 3);
    }

    #[test]
    fn backspace_removes_previous_char() {
        let mut field = InputField::new("");
        field.insert_str("Hello");
        field.backspace();
        assert_eq!(field.text, "Hell");
        assert_eq!(field.cursor, 4);
    }

    #[test]
    fn backspace_at_start_does_nothing() {
        let mut field = InputField::new("");
        field.insert_str("Hi");
        field.cursor = 0;
        field.backspace();
        assert_eq!(field.text, "Hi");
        assert_eq!(field.cursor, 0);
    }

    #[test]
    fn delete_forward_removes_next_char() {
        let mut field = InputField::new("");
        field.insert_str("Hello");
        field.cursor = 2; // After "He"
        field.delete_forward();
        // "Hello" with cursor at 2, delete_forward removes char at index 2
        // which is the first 'l'. Result: "He" + "lo" = "Helo"
        assert_eq!(field.text, "Helo");
        assert_eq!(field.cursor, 2);
    }

    #[test]
    fn cursor_left_right() {
        let mut field = InputField::new("");
        field.insert_str("Hello");
        assert_eq!(field.cursor, 5);
        field.cursor_left();
        assert_eq!(field.cursor, 4);
        field.cursor_left();
        assert_eq!(field.cursor, 3);
        field.cursor_right();
        assert_eq!(field.cursor, 4);
    }

    #[test]
    fn cursor_home_end() {
        let mut field = InputField::new("");
        field.insert_str("Hello");
        field.cursor_home();
        assert_eq!(field.cursor, 0);
        field.cursor_end();
        assert_eq!(field.cursor, 5);
    }

    #[test]
    fn clear_empties_buffer() {
        let mut field = InputField::new("");
        field.insert_str("Hello");
        field.clear();
        assert!(field.text.is_empty());
        assert_eq!(field.cursor, 0);
    }

    #[test]
    fn max_length_enforced() {
        let mut field = InputField::new("");
        for _ in 0..MAX_INPUT_LEN + 50 {
            field.insert_char('a');
        }
        assert_eq!(field.text.len(), MAX_INPUT_LEN);
    }

    #[test]
    fn unicode_handling() {
        let mut field = InputField::new("");
        field.insert_str("Hello 你好");
        assert_eq!(field.cursor, "Hello 你好".len());
        field.backspace(); // Remove '好'
        assert_eq!(field.text, "Hello 你");
        field.backspace(); // Remove '你'
        assert_eq!(field.text, "Hello ");
    }

    #[test]
    fn focus_toggle() {
        let mut field = InputField::new("");
        assert!(!field.focused);
        field.toggle_focus();
        assert!(field.focused);
        field.toggle_focus();
        assert!(!field.focused);
    }

    #[test]
    fn color_mode_placeholder() {
        let field = InputField::new("placeholder");
        let mode = input_color_mode(&field);
        match mode {
            ColorMode::Solid(r, g, b) => {
                assert!(r < 150 && g < 150 && b < 150, "Placeholder should be dim");
            }
            _ => panic!("Expected solid color for placeholder"),
        }
    }

    #[test]
    fn color_mode_focused_with_text() {
        let mut field = InputField::new("");
        field.focused = true;
        field.insert_str("text");
        let mode = input_color_mode(&field);
        match mode {
            ColorMode::Solid(r, g, b) => {
                assert!(r > 200 && g > 200 && b > 200, "Focused text should be bright");
            }
            _ => panic!("Expected solid color for focused text"),
        }
    }

    #[test]
    fn is_printable_char_filters_controls() {
        assert!(is_printable_char('a'));
        assert!(is_printable_char(' '));
        assert!(is_printable_char('!'));
        assert!(is_printable_char('你'));
        assert!(!is_printable_char('\n'));
        assert!(!is_printable_char('\t'));
        assert!(!is_printable_char('\x01'));
    }

    #[test]
    fn insert_str_truncates_at_max() {
        let mut field = InputField::new("");
        let long_str = "a".repeat(MAX_INPUT_LEN + 100);
        field.insert_str(&long_str);
        assert_eq!(field.text.len(), MAX_INPUT_LEN);
    }
}
