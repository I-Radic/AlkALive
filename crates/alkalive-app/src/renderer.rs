//! CPU software renderer — composites glyph atlas pixels into an RGBA framebuffer.
//!
//! This module implements a CPU renderer that:
//! 1. Allocates an RGBA framebuffer (width × height × 4 bytes).
//! 2. Clears to a solid background color (black by default) or renders a starfield.
//! 3. Composites grayscale glyph atlas pixels into the framebuffer with
//!    color modulation (configurable color or gradient) and alpha blending.
//! 4. Supports a pseudo-3D Y-axis rotation transform.
//! 5. Supports a glow/bloom effect (multi-pass horizontal + vertical blur).

/// Golden text color: RGB(255, 215, 0) — the classic "gold" color.
pub const GOLDEN_R: u8 = 255;
pub const GOLDEN_G: u8 = 215;
pub const GOLDEN_B: u8 = 0;

/// Black background color.
pub const BG_R: u8 = 0;
pub const BG_G: u8 = 0;
pub const BG_B: u8 = 0;

/// Text color mode: solid color or gradient.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColorMode {
    /// Solid color (r, g, b).
    Solid(u8, u8, u8),
    /// Vertical gradient from top color to bottom color.
    Gradient(u8, u8, u8, u8, u8, u8),
}

impl Default for ColorMode {
    fn default() -> Self {
        ColorMode::Solid(GOLDEN_R, GOLDEN_G, GOLDEN_B)
    }
}

/// A positioned glyph ready for compositing into the framebuffer.
#[derive(Clone, Copy, Debug)]
pub struct PositionedGlyph {
    /// Atlas page index.
    pub page: u16,
    /// UV rectangle in the atlas page (x, y, w, h) normalized 0-1.
    pub uv_x: f32,
    pub uv_y: f32,
    pub uv_w: f32,
    pub uv_h: f32,
    /// Screen-space position (top-left of the glyph bitmap).
    pub x: f32,
    pub y: f32,
    /// Glyph bitmap size (w, h) in pixels.
    pub w: f32,
    pub h: f32,
}

/// CPU software renderer with an RGBA framebuffer.
pub struct SoftwareRenderer {
    /// RGBA framebuffer (row-major, top-to-bottom, left-to-right).
    pub framebuffer: Vec<u8>,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Scratch buffer for glow effect (same size as framebuffer).
    glow_buffer: Vec<u8>,
    /// Whether glow is enabled.
    pub glow_enabled: bool,
    /// Glow radius in pixels.
    pub glow_radius: u32,
    /// Glow intensity (0.0 to 1.0).
    pub glow_intensity: f32,
}

impl SoftwareRenderer {
    /// Create a new renderer with the given dimensions.
    pub fn new(width: u32, height: u32) -> Self {
        let size = (width * height * 4) as usize;
        Self {
            framebuffer: vec![0u8; size],
            width,
            height,
            glow_buffer: vec![0u8; size],
            glow_enabled: true,
            glow_radius: 4,
            glow_intensity: 0.6,
        }
    }

    /// Resize the framebuffer.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        let size = (width * height * 4) as usize;
        self.framebuffer.resize(size, 0);
        self.glow_buffer.resize(size, 0);
    }

    /// Set glow parameters.
    pub fn set_glow(&mut self, enabled: bool, radius: u32, intensity: f32) {
        self.glow_enabled = enabled;
        self.glow_radius = radius.max(1).min(16);
        self.glow_intensity = intensity.max(0.0).min(1.0);
    }

    /// Clear the framebuffer to black (RGB 0,0,0, alpha 255).
    pub fn clear(&mut self) {
        for chunk in self.framebuffer.chunks_exact_mut(4) {
            chunk[0] = BG_R;
            chunk[1] = BG_G;
            chunk[2] = BG_B;
            chunk[3] = 255;
        }
    }

    /// Get the text color at a given vertical position within a glyph.
    /// For solid mode, returns the solid color. For gradient mode, interpolates.
    fn color_at(color_mode: ColorMode, local_y: f32) -> (u8, u8, u8) {
        match color_mode {
            ColorMode::Solid(r, g, b) => (r, g, b),
            ColorMode::Gradient(r1, g1, b1, r2, g2, b2) => {
                let t = local_y.clamp(0.0, 1.0);
                let r = (r1 as f32 * (1.0 - t) + r2 as f32 * t) as u8;
                let g = (g1 as f32 * (1.0 - t) + g2 as f32 * t) as u8;
                let b = (b1 as f32 * (1.0 - t) + b2 as f32 * t) as u8;
                (r, g, b)
            }
        }
    }

    /// Composite a single glyph from the atlas into the framebuffer.
    ///
    /// The atlas page is a grayscale buffer (1 byte per pixel, 0–255 alpha).
    /// Each non-zero atlas pixel is blended with the text color into the
    /// framebuffer using source-over alpha compositing.
    ///
    /// Arguments:
    /// - `page_data`: The atlas page pixel data (grayscale, ATLAS_SIZE×ATLAS_SIZE).
    /// - `atlas_size`: The dimension of the (square) atlas page.
    /// - `glyph`: The positioned glyph to composite.
    /// - `scale_x`: Horizontal scale factor (for rotation; 1.0 = no scaling).
    /// - `offset_x`: Horizontal pixel offset (for rotation centering).
    /// - `color_mode`: Text color mode (solid or gradient).
    /// - `accumulate_glow`: If true, also write to the glow buffer.
    pub fn composite_glyph(
        &mut self,
        page_data: &[u8],
        atlas_size: usize,
        glyph: &PositionedGlyph,
        scale_x: f32,
        offset_x: f32,
        color_mode: ColorMode,
        accumulate_glow: bool,
    ) {
        let abs_scale_x = scale_x.abs();
        if abs_scale_x < 0.01 {
            return; // Glyph too thin to see (edge-on during rotation)
        }

        // Destination position (scaled and offset for rotation).
        let dest_x = glyph.x * scale_x + offset_x;
        let dest_y = glyph.y;
        let dest_w = glyph.w * abs_scale_x;
        let dest_h = glyph.h;

        if dest_w < 0.5 || dest_h < 0.5 {
            return; // Too small to render
        }

        // For each destination pixel, sample the atlas.
        let fb_w = self.width as i32;
        let fb_h = self.height as i32;

        let x0 = dest_x.floor() as i32;
        let y0 = dest_y.floor() as i32;
        let x1 = ((dest_x + dest_w).ceil() as i32).min(fb_w);
        let y1 = ((dest_y + dest_h).ceil() as i32).min(fb_h);

        let atlas_f = atlas_size as f32;

        for dy in y0..y1 {
            if dy < 0 {
                continue;
            }
            for dx in x0..x1 {
                if dx < 0 {
                    continue;
                }
                // Map destination pixel to atlas UV (normalized 0.0–1.0).
                // For negative scale_x, flip horizontally within the glyph.
                let local_x = if scale_x >= 0.0 {
                    (dx as f32 - dest_x) / dest_w
                } else {
                    1.0 - (dx as f32 - dest_x) / dest_w
                };
                let local_y = (dy as f32 - dest_y) / dest_h;

                // UVs are normalized — convert to atlas pixel coordinates.
                let atlas_u = ((glyph.uv_x + local_x * glyph.uv_w) * atlas_f) as usize;
                let atlas_v = ((glyph.uv_y + local_y * glyph.uv_h) * atlas_f) as usize;

                if atlas_u >= atlas_size || atlas_v >= atlas_size {
                    continue;
                }

                let alpha = page_data[atlas_v * atlas_size + atlas_u];
                if alpha == 0 {
                    continue;
                }

                let alpha_f = alpha as f32 / 255.0;
                let inv_alpha = 1.0 - alpha_f;

                let (cr, cg, cb) = Self::color_at(color_mode, local_y);

                let fb_idx = ((dy as usize) * (fb_w as usize) + (dx as usize)) * 4;

                // Source-over alpha blend with text color.
                let dst_r = self.framebuffer[fb_idx] as f32;
                let dst_g = self.framebuffer[fb_idx + 1] as f32;
                let dst_b = self.framebuffer[fb_idx + 2] as f32;

                self.framebuffer[fb_idx] =
                    (cr as f32 * alpha_f + dst_r * inv_alpha) as u8;
                self.framebuffer[fb_idx + 1] =
                    (cg as f32 * alpha_f + dst_g * inv_alpha) as u8;
                self.framebuffer[fb_idx + 2] =
                    (cb as f32 * alpha_f + dst_b * inv_alpha) as u8;
                self.framebuffer[fb_idx + 3] = 255;

                // Accumulate glow: store the text alpha (max with existing).
                if accumulate_glow {
                    let glow_idx = fb_idx; // Same index
                    let existing = self.glow_buffer[glow_idx];
                    let new_val = alpha.max(existing);
                    self.glow_buffer[glow_idx] = new_val;
                    // Also store color tint in the RGB channels of the glow buffer.
                    if new_val == alpha {
                        self.glow_buffer[glow_idx + 1] = cr;
                        self.glow_buffer[glow_idx + 2] = cg;
                        self.glow_buffer[glow_idx + 3] = cb;
                    }
                }
            }
        }
    }

    /// Composite multiple glyphs with a Y-axis rotation transform.
    ///
    /// The rotation pivots around `center_x`. Each glyph's horizontal position
    /// is transformed as: `new_x = center_x + (x - center_x) * cos(angle)`,
    /// and its width is scaled by `abs(cos(angle))`. When `cos(angle) < 0`,
    /// the glyph is rendered mirrored (text appears to rotate past edge-on).
    pub fn composite_glyphs_rotated(
        &mut self,
        page_data: &[u8],
        atlas_size: usize,
        glyphs: &[PositionedGlyph],
        angle: f32,
        center_x: f32,
        color_mode: ColorMode,
    ) {
        let cos_a = angle.cos();
        for glyph in glyphs {
            let scale_x = cos_a;
            let final_offset = center_x * (1.0 - cos_a);
            self.composite_glyph(
                page_data,
                atlas_size,
                glyph,
                scale_x,
                final_offset,
                color_mode,
                self.glow_enabled,
            );
        }
    }

    /// Apply the glow/bloom effect.
    ///
    /// This reads the glow buffer (accumulated during glyph compositing),
    /// applies a separable box blur, and additively blends the result back
    /// into the framebuffer.
    pub fn apply_glow(&mut self) {
        if !self.glow_enabled {
            return;
        }

        let w = self.width as usize;
        let h = self.height as usize;
        let radius = self.glow_radius as usize;
        let intensity = self.glow_intensity;

        // Separable box blur: horizontal then vertical.
        // We blur only the alpha channel (stored in R position of glow_buffer).
        // Use a simple box filter.
        let mut temp = vec![0u8; w * h];

        // Horizontal pass: average `radius` pixels on each side.
        for y in 0..h {
            let row_start = y * w;
            for x in 0..w {
                let mut sum: u32 = 0;
                let mut count: u32 = 0;
                let x0 = x.saturating_sub(radius);
                let x1 = (x + radius + 1).min(w);
                for sx in x0..x1 {
                    sum += self.glow_buffer[(row_start + sx) * 4] as u32;
                    count += 1;
                }
                temp[row_start + x] = if count > 0 {
                    (sum / count) as u8
                } else {
                    0
                };
            }
        }

        // Vertical pass: average `radius` pixels above and below.
        // Also blend into the framebuffer additively.
        for y in 0..h {
            for x in 0..w {
                let mut sum: u32 = 0;
                let mut count: u32 = 0;
                let y0 = y.saturating_sub(radius);
                let y1 = (y + radius + 1).min(h);
                for sy in y0..y1 {
                    sum += temp[sy * w + x] as u32;
                    count += 1;
                }
                let blurred = if count > 0 { (sum / count) as u8 } else { 0 };

                // Additive blend: add blurred glow * intensity to framebuffer.
                let glow_f = (blurred as f32 / 255.0) * intensity;
                let fb_idx = (y * w + x) * 4;
                let glow_r = self.glow_buffer[fb_idx + 1] as f32 * glow_f;
                let glow_g = self.glow_buffer[fb_idx + 2] as f32 * glow_f;
                let glow_b = self.glow_buffer[fb_idx + 3] as f32 * glow_f;

                let r = self.framebuffer[fb_idx] as f32 + glow_r;
                let g = self.framebuffer[fb_idx + 1] as f32 + glow_g;
                let b = self.framebuffer[fb_idx + 2] as f32 + glow_b;

                self.framebuffer[fb_idx] = r.min(255.0) as u8;
                self.framebuffer[fb_idx + 1] = g.min(255.0) as u8;
                self.framebuffer[fb_idx + 2] = b.min(255.0) as u8;
            }
        }

        // Clear the glow buffer for the next frame.
        for b in self.glow_buffer.iter_mut() {
            *b = 0;
        }
    }

    /// Draw a filled rectangle (no blending — overwrites).
    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, r: u8, g: u8, b: u8) {
        let fb_w = self.width as i32;
        let fb_h = self.height as i32;
        let x0 = x.max(0).min(fb_w);
        let y0 = y.max(0).min(fb_h);
        let x1 = (x + w).max(0).min(fb_w);
        let y1 = (y + h).max(0).min(fb_h);

        for dy in y0..y1 {
            for dx in x0..x1 {
                let idx = ((dy as usize) * (fb_w as usize) + (dx as usize)) * 4;
                if idx + 3 < self.framebuffer.len() {
                    self.framebuffer[idx] = r;
                    self.framebuffer[idx + 1] = g;
                    self.framebuffer[idx + 2] = b;
                    self.framebuffer[idx + 3] = 255;
                }
            }
        }
    }

    /// Draw a rectangle outline (1px border).
    pub fn draw_rect_outline(&mut self, x: i32, y: i32, w: i32, h: i32, r: u8, g: u8, b: u8) {
        // Top and bottom edges.
        self.fill_rect(x, y, w, 1, r, g, b);
        self.fill_rect(x, y + h - 1, w, 1, r, g, b);
        // Left and right edges.
        self.fill_rect(x, y, 1, h, r, g, b);
        self.fill_rect(x + w - 1, y, 1, h, r, g, b);
    }

    /// Draw a vertical line (for the text cursor).
    pub fn draw_vertical_line(&mut self, x: i32, y: i32, h: i32, r: u8, g: u8, b: u8) {
        self.fill_rect(x, y, 2, h, r, g, b);
    }

    /// Draw a horizontal line.
    pub fn draw_horizontal_line(&mut self, x: i32, y: i32, w: i32, r: u8, g: u8, b: u8) {
        self.fill_rect(x, y, w, 1, r, g, b);
    }

    /// Get a raw pointer to the framebuffer for WASM export.
    pub fn framebuffer_ptr(&self) -> *const u8 {
        self.framebuffer.as_ptr()
    }

    /// Get the framebuffer length in bytes.
    pub fn framebuffer_len(&self) -> usize {
        self.framebuffer.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_clears_to_black() {
        let mut r = SoftwareRenderer::new(10, 10);
        r.clear();
        for i in (0..r.framebuffer.len()).step_by(4) {
            assert_eq!(r.framebuffer[i], 0);
            assert_eq!(r.framebuffer[i + 1], 0);
            assert_eq!(r.framebuffer[i + 2], 0);
            assert_eq!(r.framebuffer[i + 3], 255);
        }
    }

    #[test]
    fn renderer_resize_works() {
        let mut r = SoftwareRenderer::new(10, 10);
        assert_eq!(r.framebuffer.len(), 400);
        r.resize(20, 30);
        assert_eq!(r.framebuffer.len(), 2400);
        assert_eq!(r.width, 20);
        assert_eq!(r.height, 30);
    }

    #[test]
    fn composite_glyph_writes_golden_pixels() {
        let mut r = SoftwareRenderer::new(10, 10);
        r.clear();

        // Create a 2x2 atlas page with all-white pixels (full alpha).
        let atlas = vec![255u8; 4];
        let glyph = PositionedGlyph {
            page: 0,
            uv_x: 0.0,
            uv_y: 0.0,
            uv_w: 2.0,
            uv_h: 2.0,
            x: 3.0,
            y: 3.0,
            w: 2.0,
            h: 2.0,
        };

        r.composite_glyph(&atlas, 2, &glyph, 1.0, 0.0, ColorMode::default(), false);

        // Check pixel at (3,3) — should be golden.
        let idx = (3 * 10 + 3) * 4;
        assert_eq!(r.framebuffer[idx], GOLDEN_R);
        assert_eq!(r.framebuffer[idx + 1], GOLDEN_G);
        assert_eq!(r.framebuffer[idx + 2], GOLDEN_B);
    }

    #[test]
    fn composite_glyph_gradient_color() {
        let mut r = SoftwareRenderer::new(10, 20);
        r.clear();

        // Use a 10x10 atlas (atlas_size=10), glyph is 2 wide, 10 tall.
        // UV: x=0, y=0, w=0.2 (2/10), h=1.0 (10/10).
        let atlas = vec![255u8; 100]; // 10x10 atlas
        let glyph = PositionedGlyph {
            page: 0,
            uv_x: 0.0,
            uv_y: 0.0,
            uv_w: 0.2, // 2/10
            uv_h: 1.0, // 10/10
            x: 3.0,
            y: 5.0,
            w: 2.0,
            h: 10.0,
        };

        // Gradient from red (top) to blue (bottom).
        let gradient = ColorMode::Gradient(255, 0, 0, 0, 0, 255);
        r.composite_glyph(&atlas, 10, &glyph, 1.0, 0.0, gradient, false);

        // Top pixel (y=5) should be reddish, bottom pixel (y=14) should be bluish.
        let top_idx = (5 * 10 + 3) * 4;
        let bot_idx = (14 * 10 + 3) * 4;
        assert!(
            r.framebuffer[top_idx] > r.framebuffer[top_idx + 2],
            "Top should be redder (R={}, B={})",
            r.framebuffer[top_idx],
            r.framebuffer[top_idx + 2]
        );
        assert!(
            r.framebuffer[bot_idx + 2] > r.framebuffer[bot_idx],
            "Bottom should be bluer (R={}, B={})",
            r.framebuffer[bot_idx],
            r.framebuffer[bot_idx + 2]
        );
    }

    #[test]
    fn framebuffer_ptr_is_valid() {
        let r = SoftwareRenderer::new(4, 4);
        let ptr = r.framebuffer_ptr();
        let len = r.framebuffer_len();
        assert_eq!(len, 64);
        assert!(!ptr.is_null());
    }

    #[test]
    fn glow_adds_brightness() {
        let mut r = SoftwareRenderer::new(20, 20);
        r.clear();
        r.set_glow(true, 2, 1.0);

        // Composite a glyph to populate the glow buffer.
        let atlas = vec![255u8; 4];
        let glyph = PositionedGlyph {
            page: 0,
            uv_x: 0.0,
            uv_y: 0.0,
            uv_w: 2.0,
            uv_h: 2.0,
            x: 10.0,
            y: 10.0,
            w: 2.0,
            h: 2.0,
        };
        r.composite_glyph(&atlas, 2, &glyph, 1.0, 0.0, ColorMode::default(), true);

        // Pixel at (10,10) before glow.
        let idx = (10 * 20 + 10) * 4;
        let _before = r.framebuffer[idx];

        r.apply_glow();

        // After glow, nearby pixels should be brighter than black.
        let mut has_glow = false;
        for y in 8..13 {
            for x in 8..13 {
                let i = (y * 20 + x) * 4;
                if r.framebuffer[i] > 0 {
                    has_glow = true;
                    break;
                }
            }
        }
        assert!(has_glow, "Glow should add brightness around the glyph");
    }

    #[test]
    fn set_glow_clamps_values() {
        let mut r = SoftwareRenderer::new(10, 10);
        r.set_glow(true, 100, 5.0);
        assert_eq!(r.glow_radius, 16, "Glow radius should be clamped to 16");
        assert_eq!(r.glow_intensity, 1.0, "Glow intensity should be clamped to 1.0");
    }
}
