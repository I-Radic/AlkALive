//! Starfield background — a procedural starfield with twinkling stars.
//!
//! This module generates a deterministic starfield (using a simple LCG random
//! number generator) and renders it into the framebuffer with a subtle
//! twinkling animation. The stars provide visual depth behind the rotating
//! "Hello World!" text.

/// A single star in the starfield.
#[derive(Clone, Copy, Debug)]
pub struct Star {
    /// X position (0.0 to 1.0, normalized).
    pub x: f32,
    /// Y position (0.0 to 1.0, normalized).
    pub y: f32,
    /// Base brightness (0.0 to 1.0).
    pub brightness: f32,
    /// Twinkle phase offset (0.0 to 2π).
    pub twinkle_phase: f32,
    /// Twinkle speed.
    pub twinkle_speed: f32,
    /// Star size category: 0 = small (1px), 1 = medium (2px), 2 = large (3px cross).
    pub size: u8,
}

/// A starfield with twinkling animation.
pub struct Starfield {
    /// The stars.
    pub stars: Vec<Star>,
    /// Random seed for deterministic generation.
    seed: u64,
}

impl Starfield {
    /// Create a new starfield with `count` stars, using `seed` for deterministic
    /// generation.
    pub fn new(count: usize, seed: u64) -> Self {
        let mut rng = LcgRng::new(seed);
        let mut stars = Vec::with_capacity(count);
        for _ in 0..count {
            let brightness = 0.3 + rng.next_f32() * 0.7;
            let size = if brightness > 0.85 {
                2 // Large stars are rare and bright
            } else if brightness > 0.6 {
                1
            } else {
                0
            };
            stars.push(Star {
                x: rng.next_f32(),
                y: rng.next_f32(),
                brightness,
                twinkle_phase: rng.next_f32() * std::f32::consts::TAU,
                twinkle_speed: 0.5 + rng.next_f32() * 2.0,
                size,
            });
        }
        Self { stars, seed }
    }

    /// Return the seed used for generation.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Render the starfield into the framebuffer at the given time.
    ///
    /// Stars are rendered as small bright dots with twinkling brightness.
    /// Large stars get a cross-shaped sparkle.
    pub fn render(&self, framebuffer: &mut [u8], width: u32, height: u32, time: f32) {
        let w = width as i32;
        let h = height as i32;

        for star in &self.stars {
            // Twinkle: brightness oscillates ±30% around the base.
            let twinkle =
                0.7 + 0.3 * (time * star.twinkle_speed + star.twinkle_phase).sin();
            let brightness = star.brightness * twinkle;

            // Convert normalized coords to pixel coords.
            let px = (star.x * width as f32) as i32;
            let py = (star.y * height as f32) as i32;

            if px < 0 || px >= w || py < 0 || py >= h {
                continue;
            }

            // Star color: warm white with slight variation.
            // Brighter stars are more white, dimmer stars are slightly blue.
            let r = (255.0 * brightness) as u8;
            let g = (245.0 * brightness) as u8;
            let b = (220.0 * brightness * if star.size == 0 { 1.1 } else { 1.0 })
                .min(255.0) as u8;

            // Plot the center pixel.
            Self::plot(framebuffer, w, h, px, py, r, g, b);

            // Large stars get a cross-shaped sparkle.
            if star.size == 2 {
                let sparkle = (brightness * 0.5) as u8;
                Self::plot(framebuffer, w, h, px - 1, py, sparkle, sparkle, sparkle / 2);
                Self::plot(framebuffer, w, h, px + 1, py, sparkle, sparkle, sparkle / 2);
                Self::plot(framebuffer, w, h, px, py - 1, sparkle, sparkle, sparkle / 2);
                Self::plot(framebuffer, w, h, px, py + 1, sparkle, sparkle, sparkle / 2);
            } else if star.size == 1 {
                // Medium stars get a single brighter adjacent pixel.
                let sparkle = (brightness * 0.3) as u8;
                if px + 1 < w {
                    Self::plot(framebuffer, w, h, px + 1, py, sparkle, sparkle, sparkle / 2);
                }
            }
        }
    }

    /// Plot a single pixel with alpha blending (source-over).
    fn plot(fb: &mut [u8], w: i32, h: i32, x: i32, y: i32, r: u8, g: u8, b: u8) {
        if x < 0 || x >= w || y < 0 || y >= h {
            return;
        }
        let idx = ((y as usize) * (w as usize) + (x as usize)) * 4;
        if idx + 3 >= fb.len() {
            return;
        }
        // Alpha blend: assume star alpha is encoded in brightness (r value).
        let alpha = r as f32 / 255.0;
        let inv = 1.0 - alpha;
        fb[idx] = (r as f32 + fb[idx] as f32 * inv) as u8;
        fb[idx + 1] = (g as f32 + fb[idx + 1] as f32 * inv) as u8;
        fb[idx + 2] = (b as f32 + fb[idx + 2] as f32 * inv) as u8;
        fb[idx + 3] = 255;
    }
}

/// Simple LCG (Linear Congruential Generator) random number generator.
/// Deterministic and `#![forbid(unsafe_code)]` friendly.
struct LcgRng {
    state: u64,
}

impl LcgRng {
    fn new(seed: u64) -> Self {
        Self {
            state: seed.wrapping_add(0x9E3779B97F4A7C15),
        }
    }

    /// Next 64-bit random value.
    fn next_u64(&mut self) -> u64 {
        // Using the SplitMix64 algorithm for good distribution.
        let mut z = self.state.wrapping_add(0x9E3779B97F4A7C15);
        self.state = z;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    /// Next float in [0.0, 1.0).
    fn next_f32(&mut self) -> f32 {
        let v = self.next_u64() >> 40; // Use top 24 bits for float precision.
        v as f32 / (1u64 << 24) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starfield_generates_correct_count() {
        let sf = Starfield::new(100, 42);
        assert_eq!(sf.stars.len(), 100);
    }

    #[test]
    fn starfield_is_deterministic() {
        let sf1 = Starfield::new(50, 123);
        let sf2 = Starfield::new(50, 123);
        assert_eq!(sf1.stars.len(), sf2.stars.len());
        for (s1, s2) in sf1.stars.iter().zip(sf2.stars.iter()) {
            assert_eq!(s1.x, s2.x);
            assert_eq!(s1.y, s2.y);
            assert_eq!(s1.brightness, s2.brightness);
        }
    }

    #[test]
    fn starfield_renders_non_black_pixels() {
        let sf = Starfield::new(50, 42);
        let mut fb = vec![0u8; 100 * 100 * 4];
        sf.render(&mut fb, 100, 100, 0.0);
        let non_black = fb.chunks_exact(4).filter(|c| c[0] > 0 || c[1] > 0 || c[2] > 0).count();
        assert!(non_black > 0, "Starfield should render some stars");
    }

    #[test]
    fn starfield_respects_bounds() {
        let sf = Starfield::new(200, 99);
        let mut fb = vec![0u8; 10 * 10 * 4];
        // Should not panic even with more stars than pixels.
        sf.render(&mut fb, 10, 10, 100.0);
    }

    #[test]
    fn lcg_rng_produces_different_values() {
        let mut rng = LcgRng::new(1);
        let v1 = rng.next_f32();
        let v2 = rng.next_f32();
        assert_ne!(v1, v2, "RNG should produce different values");
    }

    #[test]
    fn lcg_rng_in_range() {
        let mut rng = LcgRng::new(42);
        for _ in 0..100 {
            let v = rng.next_f32();
            assert!(v >= 0.0 && v < 1.0, "RNG value {} out of [0, 1)", v);
        }
    }
}
