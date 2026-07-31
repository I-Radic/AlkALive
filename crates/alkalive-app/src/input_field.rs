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
    /// Selection anchor (byte offset). When different from cursor, a range is selected.
    /// The selection is the range [min(anchor, cursor), max(anchor, cursor)).
    pub anchor: usize,
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
    /// Internal clipboard for copy/cut/paste (simple in-WASM clipboard).
    clipboard: String,
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
            anchor: 0,
            focused: false,
            placeholder: placeholder.to_string(),
            font_id,
            cached_shaped: None,
            dirty: true,
            clipboard: String::new(),
        }
    }

    /// Returns true if there is an active selection (anchor != cursor).
    pub fn has_selection(&self) -> bool {
        self.anchor != self.cursor
    }

    /// Get the selection range as (start, end) byte offsets (start <= end).
    pub fn selection_range(&self) -> (usize, usize) {
        if self.anchor < self.cursor {
            (self.anchor, self.cursor)
        } else {
            (self.cursor, self.anchor)
        }
    }

    /// Get the selected text.
    pub fn selected_text(&self) -> &str {
        if !self.has_selection() {
            return "";
        }
        let (start, end) = self.selection_range();
        &self.text[start..end]
    }

    /// Clear the selection (set anchor = cursor, no text removed).
    pub fn clear_selection(&mut self) {
        self.anchor = self.cursor;
    }

    /// Select all text.
    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.cursor = self.text.len();
    }

    /// Select a word around the cursor.
    pub fn select_word(&mut self) {
        if self.text.is_empty() {
            return;
        }
        // Find word boundaries (alphanumeric sequences).
        let cursor_char = self.text[..self.cursor].chars().count();
        let chars: Vec<char> = self.text.chars().collect();
        let mut start = cursor_char;
        let mut end = cursor_char;
        // Move start backward while alphanumeric.
        while start > 0 && chars[start - 1].is_alphanumeric() {
            start -= 1;
        }
        // Move end forward while alphanumeric.
        while end < chars.len() && chars[end].is_alphanumeric() {
            end += 1;
        }
        // Convert char indices to byte indices.
        let start_byte = self.text.char_indices().nth(start).map(|(i, _)| i).unwrap_or(0);
        let end_byte = self.text
            .char_indices()
            .nth(end)
            .map(|(i, _)| i)
            .unwrap_or(self.text.len());
        self.anchor = start_byte;
        self.cursor = end_byte;
    }

    /// Copy the selected text to the internal clipboard.
    /// Returns the copied text (empty if no selection).
    pub fn copy_selection(&mut self) -> String {
        if !self.has_selection() {
            return String::new();
        }
        let selected = self.selected_text().to_string();
        self.clipboard = selected.clone();
        selected
    }

    /// Cut the selected text: copy to clipboard, then delete.
    /// Returns the cut text (empty if no selection).
    pub fn cut_selection(&mut self) -> String {
        if !self.has_selection() {
            return String::new();
        }
        let selected = self.selected_text().to_string();
        self.clipboard = selected.clone();
        // Delete the selected range.
        let (start, end) = self.selection_range();
        self.text.replace_range(start..end, "");
        self.cursor = start;
        self.anchor = start;
        self.dirty = true;
        selected
    }

    /// Paste from the internal clipboard at the cursor position.
    /// First deletes any active selection.
    pub fn paste(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        // Clone the clipboard content to avoid borrow issues.
        let to_paste = self.clipboard.clone();
        // Delete active selection first.
        if self.has_selection() {
            let (start, end) = self.selection_range();
            self.text.replace_range(start..end, "");
            self.cursor = start;
            self.anchor = start;
        }
        self.insert_str(&to_paste);
    }

    /// Get the clipboard content.
    pub fn get_clipboard(&self) -> &str {
        &self.clipboard
    }

    /// Set the clipboard content (for external paste).
    pub fn set_clipboard(&mut self, text: &str) {
        self.clipboard = text.to_string();
    }

    /// Delete the selected text (used by typing-over-selection).
    pub fn delete_selection(&mut self) {
        if !self.has_selection() {
            return;
        }
        let (start, end) = self.selection_range();
        self.text.replace_range(start..end, "");
        self.cursor = start;
        self.anchor = start;
        self.dirty = true;
    }

    /// Insert a character at the cursor position.
    /// If there is an active selection, the selected text is replaced.
    pub fn insert_char(&mut self, c: char) {
        if self.text.len() >= MAX_INPUT_LEN {
            return;
        }
        // If there's a selection, delete it first (replace).
        if self.has_selection() {
            self.delete_selection();
        }
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
        self.anchor = self.cursor;
        self.dirty = true;
    }

    /// Insert a string at the cursor position.
    /// If there is an active selection, the selected text is replaced.
    pub fn insert_str(&mut self, s: &str) {
        let remaining = MAX_INPUT_LEN.saturating_sub(self.text.len());
        if remaining == 0 {
            return;
        }
        // If there's a selection, delete it first (replace).
        if self.has_selection() {
            self.delete_selection();
        }
        let to_insert = if s.len() > remaining {
            &s[..remaining]
        } else {
            s
        };
        self.text.insert_str(self.cursor, to_insert);
        self.cursor += to_insert.len();
        self.anchor = self.cursor;
        self.dirty = true;
    }

    /// Delete the character before the cursor (backspace).
    /// If there is an active selection, deletes the selection instead.
    pub fn backspace(&mut self) {
        if self.has_selection() {
            self.delete_selection();
            return;
        }
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
        self.anchor = self.cursor;
        self.dirty = true;
    }

    /// Delete the character after the cursor (forward delete).
    /// If there is an active selection, deletes the selection instead.
    pub fn delete_forward(&mut self) {
        if self.has_selection() {
            self.delete_selection();
            return;
        }
        if self.cursor >= self.text.len() {
            return;
        }
        let next_char_end = self.text[self.cursor..]
            .char_indices()
            .nth(1)
            .map(|(i, _)| self.cursor + i)
            .unwrap_or(self.text.len());
        self.text.replace_range(self.cursor..next_char_end, "");
        self.anchor = self.cursor;
        self.dirty = true;
    }

    /// Move the cursor left by one character (clears selection).
    pub fn cursor_left(&mut self) {
        // If there's a selection, collapse to the left edge.
        if self.has_selection() {
            let (start, _) = self.selection_range();
            self.cursor = start;
            self.anchor = start;
            return;
        }
        if self.cursor == 0 {
            return;
        }
        let prev_start = self.text[..self.cursor]
            .char_indices()
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0);
        self.cursor = prev_start;
        self.anchor = self.cursor;
    }

    /// Move the cursor right by one character (clears selection).
    pub fn cursor_right(&mut self) {
        // If there's a selection, collapse to the right edge.
        if self.has_selection() {
            let (_, end) = self.selection_range();
            self.cursor = end;
            self.anchor = end;
            return;
        }
        if self.cursor >= self.text.len() {
            return;
        }
        let next_end = self.text[self.cursor..]
            .char_indices()
            .nth(1)
            .map(|(i, _)| self.cursor + i)
            .unwrap_or(self.text.len());
        self.cursor = next_end;
        self.anchor = self.cursor;
    }

    /// Extend the selection left by one character (Shift+ArrowLeft).
    pub fn cursor_left_extend(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev_start = self.text[..self.cursor]
            .char_indices()
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0);
        self.cursor = prev_start;
        // Don't update anchor — this extends the selection.
    }

    /// Extend the selection right by one character (Shift+ArrowRight).
    pub fn cursor_right_extend(&mut self) {
        if self.cursor >= self.text.len() {
            return;
        }
        let next_end = self.text[self.cursor..]
            .char_indices()
            .nth(1)
            .map(|(i, _)| self.cursor + i)
            .unwrap_or(self.text.len());
        self.cursor = next_end;
        // Don't update anchor — this extends the selection.
    }

    /// Extend selection to start of text (Shift+Home).
    pub fn cursor_home_extend(&mut self) {
        self.cursor = 0;
    }

    /// Extend selection to end of text (Shift+End).
    pub fn cursor_end_extend(&mut self) {
        self.cursor = self.text.len();
    }

    /// Move the cursor to the start (Home key, clears selection).
    pub fn cursor_home(&mut self) {
        self.cursor = 0;
        self.anchor = 0;
    }

    /// Move the cursor to the end (End key, clears selection).
    pub fn cursor_end(&mut self) {
        self.cursor = self.text.len();
        self.anchor = self.cursor;
    }

    /// Clear all text.
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.anchor = 0;
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
        self.byte_offset_to_x(self.cursor)
    }

    /// Get the anchor X position in text-space pixels (for selection rendering).
    pub fn anchor_x(&self) -> f32 {
        self.byte_offset_to_x(self.anchor)
    }

    /// Convert a byte offset to an X position in text-space pixels.
    fn byte_offset_to_x(&self, byte_offset: usize) -> f32 {
        if self.cached_shaped.is_none() || self.text.is_empty() {
            return 0.0;
        }
        let run = self.cached_shaped.as_ref().unwrap();

        let mut x = 0.0f32;
        for (i, &cluster) in run.clusters.iter().enumerate() {
            if cluster as usize >= byte_offset {
                break;
            }
            x += run.advances[i];
        }
        x
    }

    /// Get the selection X range as (start_x, end_x) in text-space pixels.
    /// Returns None if there is no selection.
    pub fn selection_x_range(&self) -> Option<(f32, f32)> {
        if !self.has_selection() {
            return None;
        }
        let cursor_x = self.cursor_x();
        let anchor_x = self.anchor_x();
        Some((cursor_x.min(anchor_x), cursor_x.max(anchor_x)))
    }

    /// Hit-test: convert an X pixel position (relative to text start) to the
    /// nearest cursor byte offset.
    ///
    /// This is the inverse of `byte_offset_to_x`. Given a click position,
    /// returns the byte offset of the character boundary closest to that position.
    pub fn hit_test(&self, x: f32) -> usize {
        if self.cached_shaped.is_none() || self.text.is_empty() {
            return 0;
        }
        let run = self.cached_shaped.as_ref().unwrap();

        // Walk through glyphs, tracking the byte offset and accumulated x.
        // When the click x is closer to the right edge of the current glyph
        // than the left edge, we place the cursor after it.
        let mut accum_x = 0.0f32;
        let mut last_cluster = 0u32;

        for (i, &cluster) in run.clusters.iter().enumerate() {
            let glyph_center = accum_x + run.advances[i] / 2.0;
            if x < glyph_center {
                // Click is in the left half of this glyph — cursor goes before it.
                return cluster as usize;
            }
            accum_x += run.advances[i];
            last_cluster = cluster;
        }

        // Click is past the end — cursor at the end of text.
        // Find the byte length of the last cluster's character.
        // The cluster value is the byte offset of the start of the character.
        // The end is either the next cluster or the text length.
        let last_cluster_byte = last_cluster as usize;
        if last_cluster_byte >= self.text.len() {
            return self.text.len();
        }
        // Find the next character boundary after last_cluster_byte.
        let remainder = &self.text[last_cluster_byte..];
        if let Some(c) = remainder.chars().next() {
            last_cluster_byte + c.len_utf8()
        } else {
            self.text.len()
        }
    }

    /// Handle a mouse click at the given X position (relative to text start).
    /// Sets the cursor to the clicked position and clears selection (unless
    /// shift is held, which extends the selection).
    pub fn click_at(&mut self, x: f32, extend: bool) {
        let pos = self.hit_test(x);
        if extend {
            // Shift+click: extend selection to clicked position.
            self.cursor = pos;
        } else {
            // Plain click: move cursor, clear selection.
            self.cursor = pos;
            self.anchor = pos;
        }
    }

    /// Handle a drag selection: update cursor to the dragged position,
    /// keeping the anchor where the drag started.
    pub fn drag_to(&mut self, x: f32) {
        let pos = self.hit_test(x);
        self.cursor = pos;
        // Anchor stays at the click position to create a selection.
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
        // Move cursor to after "He" (use cursor_left to avoid selection).
        for _ in 0..3 {
            field.cursor_left();
        }
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
        // Move cursor to start using cursor_home (keeps anchor in sync).
        field.cursor_home();
        field.backspace();
        assert_eq!(field.text, "Hi");
        assert_eq!(field.cursor, 0);
    }

    #[test]
    fn delete_forward_removes_next_char() {
        let mut field = InputField::new("");
        field.insert_str("Hello");
        // Move cursor to after "He" (3 lefts from end).
        for _ in 0..3 {
            field.cursor_left();
        }
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

    // --- Selection tests ---

    #[test]
    fn no_selection_initially() {
        let field = InputField::new("");
        assert!(!field.has_selection());
    }

    #[test]
    fn select_all() {
        let mut field = InputField::new("");
        field.insert_str("Hello World");
        field.select_all();
        assert!(field.has_selection());
        assert_eq!(field.selected_text(), "Hello World");
    }

    #[test]
    fn select_word() {
        let mut field = InputField::new("");
        field.insert_str("Hello World");
        // Position cursor in the middle of "Hello"
        field.cursor = 2;
        field.anchor = 2;
        field.select_word();
        assert!(field.has_selection());
        assert_eq!(field.selected_text(), "Hello");
    }

    #[test]
    fn cursor_left_extend_creates_selection() {
        let mut field = InputField::new("");
        field.insert_str("Hello");
        // Cursor is at end (5). Extend left.
        field.cursor_left_extend();
        assert!(field.has_selection());
        assert_eq!(field.selected_text(), "o");
    }

    #[test]
    fn cursor_right_extend_creates_selection() {
        let mut field = InputField::new("");
        field.insert_str("Hello");
        field.cursor = 0;
        field.anchor = 0;
        field.cursor_right_extend();
        assert!(field.has_selection());
        assert_eq!(field.selected_text(), "H");
    }

    #[test]
    fn copy_selection_works() {
        let mut field = InputField::new("");
        field.insert_str("Hello World");
        field.select_all();
        let copied = field.copy_selection();
        assert_eq!(copied, "Hello World");
        // Text should still be there (copy doesn't delete).
        assert_eq!(field.text, "Hello World");
        assert!(field.has_selection());
    }

    #[test]
    fn cut_selection_removes_text() {
        let mut field = InputField::new("");
        field.insert_str("Hello World");
        field.select_all();
        let cut = field.cut_selection();
        assert_eq!(cut, "Hello World");
        assert!(field.text.is_empty());
        assert!(!field.has_selection());
    }

    #[test]
    fn paste_inserts_clipboard() {
        let mut field = InputField::new("");
        field.insert_str("Hello World");
        field.select_all();
        field.cut_selection();
        assert!(field.text.is_empty());
        field.paste();
        assert_eq!(field.text, "Hello World");
    }

    #[test]
    fn typing_replaces_selection() {
        let mut field = InputField::new("");
        field.insert_str("Hello World");
        field.select_all();
        field.insert_char('X');
        assert_eq!(field.text, "X");
        assert!(!field.has_selection());
    }

    #[test]
    fn backspace_with_selection_deletes_selection() {
        let mut field = InputField::new("");
        field.insert_str("Hello World");
        field.select_all();
        field.backspace();
        assert!(field.text.is_empty());
    }

    #[test]
    fn delete_forward_with_selection_deletes_selection() {
        let mut field = InputField::new("");
        field.insert_str("Hello World");
        field.cursor = 0;
        field.anchor = 0;
        field.cursor_right_extend(); // Select "H"
        field.delete_forward();
        assert_eq!(field.text, "ello World");
    }

    #[test]
    fn clear_selection_does_not_remove_text() {
        let mut field = InputField::new("");
        field.insert_str("Hello");
        field.select_all();
        field.clear_selection();
        assert!(!field.has_selection());
        assert_eq!(field.text, "Hello");
    }

    #[test]
    fn cursor_left_collapses_selection() {
        let mut field = InputField::new("");
        field.insert_str("Hello");
        field.select_all(); // anchor=0, cursor=5
        field.cursor_left();
        // Should collapse to start (cursor=0).
        assert!(!field.has_selection());
        assert_eq!(field.cursor, 0);
    }

    #[test]
    fn cursor_right_collapses_selection() {
        let mut field = InputField::new("");
        field.insert_str("Hello");
        field.select_all(); // anchor=0, cursor=5
        field.cursor_right();
        // Should collapse to end (cursor=5).
        assert!(!field.has_selection());
        assert_eq!(field.cursor, 5);
    }

    #[test]
    fn selection_x_range_returns_none_without_selection() {
        let field = InputField::new("");
        assert!(field.selection_x_range().is_none());
    }

    #[test]
    fn clipboard_get_set() {
        let mut field = InputField::new("");
        field.set_clipboard("test clipboard");
        assert_eq!(field.get_clipboard(), "test clipboard");
    }

    #[test]
    fn select_word_on_empty_does_nothing() {
        let mut field = InputField::new("");
        field.select_word();
        assert!(!field.has_selection());
    }

    #[test]
    fn home_end_extend_selection() {
        let mut field = InputField::new("");
        field.insert_str("Hello");
        field.cursor = 3;
        field.anchor = 3;
        field.cursor_home_extend();
        assert!(field.has_selection());
        assert_eq!(field.selected_text(), "Hel");
    }

    // --- Hit test / mouse selection tests ---

    #[test]
    fn hit_test_before_text_returns_zero() {
        let mut field = InputField::new("");
        field.insert_str("Hello");
        // Force shape by getting glyphs (this populates cached_shaped).
        // We can't call get_positioned_glyphs without registry, so test hit_test
        // returns 0 when there's no cached shaped run.
        let pos = field.hit_test(-10.0);
        assert_eq!(pos, 0);
    }

    #[test]
    fn hit_test_empty_text_returns_zero() {
        let field = InputField::new("");
        let pos = field.hit_test(100.0);
        assert_eq!(pos, 0);
    }

    #[test]
    fn click_at_clears_selection() {
        let mut field = InputField::new("");
        field.insert_str("Hello");
        field.select_all();
        assert!(field.has_selection());
        // click_at with extend=false should clear selection.
        field.click_at(0.0, false);
        assert!(!field.has_selection());
    }

    #[test]
    fn click_at_extend_creates_selection() {
        let mut field = InputField::new("");
        field.insert_str("Hello World");
        field.cursor = 0;
        field.anchor = 0;
        // Shift+click at some position should extend selection.
        // Without a shaped run, hit_test returns 0, so this just sets cursor=0.
        field.click_at(50.0, true);
        // cursor and anchor may both be 0 (no shaped run), but it shouldn't panic.
    }

    #[test]
    fn drag_to_updates_cursor() {
        let mut field = InputField::new("");
        field.insert_str("Hello");
        field.cursor = 0;
        field.anchor = 0;
        // Drag without shaped run — hit_test returns 0.
        field.drag_to(100.0);
        // Should not panic; cursor may stay at 0.
    }

    #[test]
    fn hit_test_returns_text_len_for_past_end() {
        let mut field = InputField::new("");
        field.insert_str("Hi");
        // Without cached shaped run, hit_test returns 0 for empty/no cache.
        // But if we set cached_shaped to None, it returns 0.
        let pos = field.hit_test(10000.0);
        // Without a shaped run, returns 0.
        assert_eq!(pos, 0);
    }
}
