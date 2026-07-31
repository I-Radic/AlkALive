//! CPU software renderer — composites glyph atlas pixels into an RGBA framebuffer.
//!
//! This module implements a minimal CPU renderer that:
//! 1. Allocates an RGBA framebuffer (width × height × 4 bytes).
//! 2. Clears to a solid background color (black by default).
//! 3. Composites grayscale glyph atlas pixels into the framebuffer with
//!    color modulation (golden tint) and alpha blending.
//! 4. Supports a pseudo-3D Y-axis rotation transform.
//!
//! This bypasses the abstract `alkalive_render::Backend` trait (which has no
//! concrete implementation yet) and provides a direct CPU path for the
//! Hello World deployment. The framebuffer is exposed to JavaScript via
//! `wasm_bindgen` pointer exports and copied to a `<canvas>` via
//! `putImageData`.

/// Golden text color: RGB(255, 215, 0) — the classic "gold" color.
pub const GOLDEN_R: u8 = 255;
pub const GOLDEN_G: u8 = 215;
pub const GOLDEN_B: u8 = 0;

/// Black background color.
pub const BG_R: u8 = 0;
pub const BG_G: u8 = 0;
pub const BG_B: u8 = 0;

/// A positioned glyph ready for compositing into the framebuffer.
#[derive(Clone, Copy, Debug)]
pub struct PositionedGlyph {
    /// Atlas page index.
    pub page: u16,
    /// UV rectangle in the atlas page (x, y, w, h) in pixels.
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
}

impl SoftwareRenderer {
    /// Create a new renderer with the given dimensions.
    pub fn new(width: u32, height: u32) -> Self {
        let framebuffer = vec![0u8; (width * height * 4) as usize];
        Self {
            framebuffer,
            width,
            height,
        }
    }

    /// Resize the framebuffer.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.framebuffer.resize((width * height * 4) as usize, 0);
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

    /// Composite a single glyph from the atlas into the framebuffer.
    ///
    /// The atlas page is a grayscale buffer (1 byte per pixel, 0–255 alpha).
    /// Each non-zero atlas pixel is blended with the golden color into the
    /// framebuffer using source-over alpha compositing.
    ///
    /// Arguments:
    /// - `page_data`: The atlas page pixel data (grayscale, ATLAS_SIZE×ATLAS_SIZE).
    /// - `atlas_size`: The dimension of the (square) atlas page.
    /// - `glyph`: The positioned glyph to composite.
    /// - `scale_x`: Horizontal scale factor (for rotation; 1.0 = no scaling).
    /// - `offset_x`: Horizontal pixel offset (for rotation centering).
    pub fn composite_glyph(
        &mut self,
        page_data: &[u8],
        atlas_size: usize,
        glyph: &PositionedGlyph,
        scale_x: f32,
        offset_x: f32,
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
                let atlas_f = atlas_size as f32;
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

                let fb_idx = ((dy as usize) * (fb_w as usize) + (dx as usize)) * 4;

                // Source-over alpha blend with golden color.
                let dst_r = self.framebuffer[fb_idx] as f32;
                let dst_g = self.framebuffer[fb_idx + 1] as f32;
                let dst_b = self.framebuffer[fb_idx + 2] as f32;

                self.framebuffer[fb_idx] =
                    (GOLDEN_R as f32 * alpha_f + dst_r * inv_alpha) as u8;
                self.framebuffer[fb_idx + 1] =
                    (GOLDEN_G as f32 * alpha_f + dst_g * inv_alpha) as u8;
                self.framebuffer[fb_idx + 2] =
                    (GOLDEN_B as f32 * alpha_f + dst_b * inv_alpha) as u8;
                self.framebuffer[fb_idx + 3] = 255;
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
    ) {
        let cos_a = angle.cos();
        for glyph in glyphs {
            // Y-axis rotation: scale X by cos(angle), shift so the center
            // stays at the rotated position.
            // dest_x = center_x + (glyph.x - center_x) * cos_a
            //        = glyph.x * cos_a + center_x * (1 - cos_a)
            let scale_x = cos_a;
            let final_offset = center_x * (1.0 - cos_a);
            self.composite_glyph(
                page_data,
                atlas_size,
                glyph,
                scale_x,
                final_offset,
            );
        }
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

        r.composite_glyph(&atlas, 2, &glyph, 1.0, 0.0);

        // Check pixel at (3,3) — should be golden.
        let idx = (3 * 10 + 3) * 4;
        assert_eq!(r.framebuffer[idx], GOLDEN_R);
        assert_eq!(r.framebuffer[idx + 1], GOLDEN_G);
        assert_eq!(r.framebuffer[idx + 2], GOLDEN_B);
    }

    #[test]
    fn framebuffer_ptr_is_valid() {
        let r = SoftwareRenderer::new(4, 4);
        let ptr = r.framebuffer_ptr();
        let len = r.framebuffer_len();
        assert_eq!(len, 64);
        assert!(!ptr.is_null());
    }
}
