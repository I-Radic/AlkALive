//! Minimal scanline glyph rasterizer (no external dependencies).
//!
//! A small, self-contained rasterizer that converts vector path commands
//! (move_to / line_to / quad_to / close) into a grayscale alpha bitmap
//! (`Vec<u8>`, 0–255) using a scanline algorithm with the even-odd fill
//! rule.
//!
//! # Design
//!
//! - **Path intake:** path commands are recorded as a list of straight
//!   edges. Quadratic Bézier segments are flattened to line segments by
//!   4 fixed subdivisions of de Casteljau evaluation (16 segments per
//!   quad) — sufficient visual fidelity for Wave-2 glyph atlas tiles.
//! - **Rasterization:** for each pixel row, 4 vertical sub-samples are
//!   evaluated. At each sub-row, edges crossing `y_sub` are intersected
//!   and the resulting x-intersections are sorted and paired (even-odd
//!   rule). Horizontal pixel coverage is computed as the overlap of each
//!   span `[xa, xb]` with the pixel column `[p, p+1]`, giving smooth
//!   horizontal anti-aliasing; the 4 vertical sub-samples give 5 vertical
//!   coverage levels. The final per-pixel alpha is
//!   `coverage * 255 / (SUBSAMPLES * 256)`, clamped to `[0, 255]`.
//!
//! # Safety
//!
//! This crate is `#![forbid(unsafe_code)]` and has no external dependencies.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// A straight-line edge in path coordinates: `(x0, y0) -> (x1, y1)`.
type Edge = (f32, f32, f32, f32);

/// Number of vertical sub-samples per pixel row used for anti-aliasing.
const SUBSAMPLES: usize = 4;

/// Scanline rasterizer that converts path commands into a grayscale alpha
/// bitmap.
///
/// Build a path by calling [`move_to`](Self::move_to),
/// [`line_to`](Self::line_to), [`quad_to`](Self::quad_to), and
/// [`close`](Self::close) in any order, then call
/// [`rasterize`](Self::rasterize) to produce a `Vec<u8>` of length
/// `width * height` holding per-pixel alpha values in `[0, 255]`.
///
/// Coordinates are in pixel space with Y growing **downward** (bitmap
/// convention). The caller is responsible for flipping font-space Y if
/// necessary before emitting path commands.
#[derive(Clone)]
pub struct Rasterizer {
    width: usize,
    height: usize,
    edges: Vec<Edge>,
    current: Option<(f32, f32)>,
    subpath_start: Option<(f32, f32)>,
}

impl Rasterizer {
    /// Create a new `Rasterizer` for a `width x height` pixel bitmap.
    ///
    /// Both dimensions must be non-negative; passing `0` for either
    /// produces an empty bitmap on [`rasterize`](Self::rasterize).
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            edges: Vec::new(),
            current: None,
            subpath_start: None,
        }
    }

    /// Returns the bitmap width in pixels.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Returns the bitmap height in pixels.
    pub fn height(&self) -> usize {
        self.height
    }

    /// Begin a new subpath at `(x, y)`.
    ///
    /// If a subpath is already open, it is left unclosed (path topology
    /// does not affect the even-odd fill rule). The current point and
    /// subpath start are both set to `(x, y)`.
    pub fn move_to(&mut self, x: f32, y: f32) {
        self.current = Some((x, y));
        self.subpath_start = Some((x, y));
    }

    /// Append a straight line from the current point to `(x, y)`.
    ///
    /// If no current point is set, this is treated as a
    /// [`move_to`](Self::move_to).
    pub fn line_to(&mut self, x: f32, y: f32) {
        if let Some((cx, cy)) = self.current {
            self.edges.push((cx, cy, x, y));
        } else {
            self.move_to(x, y);
        }
        self.current = Some((x, y));
    }

    /// Append a quadratic Bézier from the current point with control point
    /// `(cx, cy)` and endpoint `(x, y)`.
    ///
    /// The curve is flattened to 16 line segments (4 levels of
    /// subdivision) via direct Bézier evaluation
    /// `B(t) = (1-t)^2 P0 + 2(1-t)t C + t^2 P1`.
    pub fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let Some((sx, sy)) = self.current else {
            self.move_to(x, y);
            return;
        };
        const N: usize = 16;
        let mut prev_x = sx;
        let mut prev_y = sy;
        for i in 1..=N {
            let t = i as f32 / N as f32;
            let one_t = 1.0 - t;
            let px = one_t * one_t * sx + 2.0 * one_t * t * cx + t * t * x;
            let py = one_t * one_t * sy + 2.0 * one_t * t * cy + t * t * y;
            self.edges.push((prev_x, prev_y, px, py));
            prev_x = px;
            prev_y = py;
        }
        // Snap the final point to the exact endpoint to avoid float drift.
        if (prev_x - x).abs() > 0.0 || (prev_y - y).abs() > 0.0 {
            self.edges.push((prev_x, prev_y, x, y));
        }
        self.current = Some((x, y));
    }

    /// Close the current subpath by appending a line from the current
    /// point back to the subpath start (if different).
    ///
    /// After closing, the current point is set to the subpath start so
    /// subsequent commands continue from there.
    pub fn close(&mut self) {
        if let (Some((cx, cy)), Some((sx, sy))) = (self.current, self.subpath_start) {
            if (cx - sx).abs() > 0.0 || (cy - sy).abs() > 0.0 {
                self.edges.push((cx, cy, sx, sy));
            }
            self.current = Some((sx, sy));
        }
    }

    /// Drop all recorded path data without changing the bitmap dimensions.
    ///
    /// Useful for reusing the same `Rasterizer` allocation across multiple
    /// glyphs.
    pub fn clear(&mut self) {
        self.edges.clear();
        self.current = None;
        self.subpath_start = None;
    }

    /// Rasterize the recorded path into a grayscale alpha bitmap.
    ///
    /// Returns a `Vec<u8>` of length `width * height` where each byte is
    /// a per-pixel alpha value in `[0, 255]`. An empty `Vec` is returned
    /// if either dimension is zero.
    pub fn rasterize(&self) -> Vec<u8> {
        let w = self.width;
        let h = self.height;
        if w == 0 || h == 0 {
            return Vec::new();
        }
        // Per-pixel coverage accumulator in fixed-point units of 1/256.
        // Max value = SUBSAMPLES * 256 (one full pixel from each sub-row).
        let mut coverage: Vec<u16> = vec![0; w * h];
        let mut xs: Vec<f32> = Vec::with_capacity(64);

        for y in 0..h {
            for s in 0..SUBSAMPLES {
                let y_sub = y as f32 + (s as f32 + 0.5) / SUBSAMPLES as f32;
                xs.clear();
                for &(x0, y0, x1, y1) in &self.edges {
                    // Skip horizontal edges; they don't cross scanlines.
                    if y0 == y1 {
                        continue;
                    }
                    let (ymin, ymax) = if y0 < y1 { (y0, y1) } else { (y1, y0) };
                    // Half-open [ymin, ymax) avoids double-counting at
                    // shared vertices under the even-odd rule.
                    if y_sub < ymin || y_sub >= ymax {
                        continue;
                    }
                    let dy = y1 - y0;
                    let t = (y_sub - y0) / dy;
                    xs.push(x0 + t * (x1 - x0));
                }
                xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
                let mut i = 0;
                while i + 1 < xs.len() {
                    let xa = xs[i].max(0.0);
                    let xb = (xs[i + 1]).min(w as f32);
                    i += 2;
                    if xa >= xb {
                        continue;
                    }
                    // Iterate only over pixel columns overlapping [xa, xb].
                    let p_lo = (xa as usize).min(w);
                    let p_hi = ((xb as usize) + 1).min(w);
                    for p in p_lo..p_hi {
                        let left = xa.max(p as f32);
                        let right = xb.min((p + 1) as f32);
                        if right > left {
                            // Coverage in [0, 1] for this sub-row, scaled by 256.
                            let cov = ((right - left) * 256.0) as u16;
                            let idx = y * w + p;
                            coverage[idx] = coverage[idx].saturating_add(cov);
                        }
                    }
                }
            }
        }

        let max_cov = (SUBSAMPLES * 256) as u32;
        coverage
            .iter()
            .map(|&c| (((c as u32) * 255) / max_cov).min(255) as u8)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit square from (0,0) to (1,1) should fill the single pixel at
    /// (0,0) of a 1x1 bitmap (with anti-aliased edges folded in by the
    /// sub-sampling).
    #[test]
    fn fills_unit_square_in_single_pixel_bitmap() {
        let mut r = Rasterizer::new(1, 1);
        r.move_to(0.0, 0.0);
        r.line_to(1.0, 0.0);
        r.line_to(1.0, 1.0);
        r.line_to(0.0, 1.0);
        r.close();
        let bmp = r.rasterize();
        assert_eq!(bmp.len(), 1);
        // Single pixel fully covered.
        assert_eq!(bmp[0], 255);
    }

    /// An empty path produces an all-zero bitmap.
    #[test]
    fn empty_path_produces_zero_bitmap() {
        let r = Rasterizer::new(4, 4);
        let bmp = r.rasterize();
        assert_eq!(bmp.len(), 16);
        assert!(bmp.iter().all(|&a| a == 0));
    }

    /// A 4x4 square placed exactly on pixel boundaries fully covers the
    /// 4x4 bitmap at full alpha.
    #[test]
    fn square_on_pixel_boundaries_fully_covers() {
        let mut r = Rasterizer::new(4, 4);
        r.move_to(0.0, 0.0);
        r.line_to(4.0, 0.0);
        r.line_to(4.0, 4.0);
        r.line_to(0.0, 4.0);
        r.close();
        let bmp = r.rasterize();
        assert_eq!(bmp.len(), 16);
        for (i, &a) in bmp.iter().enumerate() {
            assert_eq!(a, 255, "pixel {} should be fully covered, got {}", i, a);
        }
    }

    /// A small quad Beziers test: a curve that should produce non-zero
    /// alpha in the bitmap.
    #[test]
    fn quad_bezier_produces_nonzero_alpha() {
        let mut r = Rasterizer::new(8, 8);
        // A filled triangle with a curved edge.
        r.move_to(1.0, 1.0);
        r.quad_to(7.0, 1.0, 4.0, 7.0);
        r.close();
        let bmp = r.rasterize();
        assert_eq!(bmp.len(), 64);
        assert!(
            bmp.iter().any(|&a| a > 0),
            "expected at least one non-zero pixel"
        );
    }

    /// Zero-dimension rasterizer produces an empty bitmap.
    #[test]
    fn zero_dimension_produces_empty_bitmap() {
        let r = Rasterizer::new(0, 10);
        assert!(r.rasterize().is_empty());
        let r = Rasterizer::new(10, 0);
        assert!(r.rasterize().is_empty());
    }

    /// `clear` removes all path data.
    #[test]
    fn clear_resets_path() {
        let mut r = Rasterizer::new(4, 4);
        r.move_to(0.0, 0.0);
        r.line_to(4.0, 0.0);
        r.line_to(4.0, 4.0);
        r.line_to(0.0, 4.0);
        r.close();
        assert!(!r.edges.is_empty());
        r.clear();
        assert!(r.edges.is_empty());
        let bmp = r.rasterize();
        assert!(bmp.iter().all(|&a| a == 0));
    }

    /// A path entirely outside the bitmap produces an all-zero bitmap.
    #[test]
    fn path_outside_bitmap_produces_zero_alpha() {
        let mut r = Rasterizer::new(4, 4);
        r.move_to(10.0, 10.0);
        r.line_to(20.0, 10.0);
        r.line_to(20.0, 20.0);
        r.line_to(10.0, 20.0);
        r.close();
        let bmp = r.rasterize();
        assert!(bmp.iter().all(|&a| a == 0));
    }
}
