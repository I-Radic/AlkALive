//! Particle system — emits and animates particles for visual effects.
//!
//! This module implements a CPU particle system that:
//! 1. Spawns particles at configurable rates from emission points.
//! 2. Updates particle positions, velocities, lifetimes, and colors each frame.
//! 3. Renders particles as small colored dots with alpha blending.
//!
//! Used to add visual flair: particles can emit from the rotating text edges,
//! creating a "sparkle" or "embers" effect that enhances the golden text.

/// Maximum number of active particles (prevents unbounded memory growth).
pub const MAX_PARTICLES: usize = 500;

/// A single particle.
#[derive(Clone, Copy, Debug)]
pub struct Particle {
    /// Current X position in pixels.
    pub x: f32,
    /// Current Y position in pixels.
    pub y: f32,
    /// Velocity X in pixels/second.
    pub vx: f32,
    /// Velocity Y in pixels/second.
    pub vy: f32,
    /// Remaining lifetime in seconds.
    pub life: f32,
    /// Initial lifetime (for fade calculations).
    pub max_life: f32,
    /// Particle size in pixels (radius).
    pub size: f32,
    /// Red component (0-255).
    pub r: u8,
    /// Green component (0-255).
    pub g: u8,
    /// Blue component (0-255).
    pub b: u8,
}

impl Particle {
    /// Create a new particle with the given properties.
    pub fn new(x: f32, y: f32, vx: f32, vy: f32, life: f32, size: f32, r: u8, g: u8, b: u8) -> Self {
        Self {
            x, y, vx, vy,
            life,
            max_life: life,
            size,
            r, g, b,
        }
    }

    /// Whether this particle is still alive (life > 0).
    pub fn is_alive(&self) -> bool {
        self.life > 0.0
    }

    /// Current alpha based on remaining life (fades out).
    pub fn alpha(&self) -> f32 {
        if self.max_life <= 0.0 {
            return 0.0;
        }
        (self.life / self.max_life).clamp(0.0, 1.0)
    }
}

/// Particle emission mode — determines spawn position and behavior.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EmissionMode {
    /// No particles.
    Off,
    /// Emit from random positions along a horizontal line (text baseline).
    TextLine,
    /// Emit upward from bottom of screen (rising sparks).
    RisingSparks,
    /// Emit radially from center (explosion effect).
    RadialBurst,
    /// Emit from all edges of the screen (ambient).
    Ambient,
}

impl Default for EmissionMode {
    fn default() -> Self {
        EmissionMode::Off
    }
}

/// The particle system.
pub struct ParticleSystem {
    /// Active particles (pre-allocated pool).
    pub particles: Vec<Particle>,
    /// Current emission mode.
    pub mode: EmissionMode,
    /// Emission rate in particles per second.
    pub emission_rate: f32,
    /// Accumulator for emission timing.
    emit_accumulator: f32,
    /// Random seed for deterministic generation.
    rng_state: u64,
    /// Emission center X (for radial/text modes).
    pub center_x: f32,
    /// Emission center Y (for radial/text modes).
    pub center_y: f32,
    /// Emission line width (for text mode).
    pub line_width: f32,
}

impl ParticleSystem {
    /// Create a new particle system with the given capacity.
    pub fn new() -> Self {
        Self {
            particles: Vec::with_capacity(MAX_PARTICLES),
            mode: EmissionMode::Off,
            emission_rate: 50.0, // 50 particles/sec default
            emit_accumulator: 0.0,
            rng_state: 12345,
            center_x: 400.0,
            center_y: 300.0,
            line_width: 300.0,
        }
    }

    /// Set the emission mode.
    pub fn set_mode(&mut self, mode: EmissionMode) {
        self.mode = mode;
    }

    /// Set the emission rate (particles per second).
    pub fn set_emission_rate(&mut self, rate: f32) {
        self.emission_rate = rate.max(0.0).min(500.0);
    }

    /// Set the emission center point.
    pub fn set_center(&mut self, x: f32, y: f32) {
        self.center_x = x;
        self.center_y = y;
    }

    /// Set the emission line width (for text mode).
    pub fn set_line_width(&mut self, w: f32) {
        self.line_width = w;
    }

    /// Next random float in [0.0, 1.0).
    fn rand_f32(&mut self) -> f32 {
        // LCG step
        self.rng_state = self
            .rng_state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let v = (self.rng_state >> 40) as u32;
        v as f32 / (1u32 << 24) as f32
    }

    /// Update the particle system by `dt` seconds and emit new particles.
    pub fn update(&mut self, dt: f32, width: u32, height: u32) {
        // Emit new particles based on mode.
        if self.mode != EmissionMode::Off && self.particles.len() < MAX_PARTICLES {
            self.emit_accumulator += self.emission_rate * dt;
            while self.emit_accumulator >= 1.0 && self.particles.len() < MAX_PARTICLES {
                self.emit_accumulator -= 1.0;
                self.spawn_particle(width, height);
            }
        }

        // Update existing particles.
        for p in self.particles.iter_mut() {
            p.x += p.vx * dt;
            p.y += p.vy * dt;
            p.life -= dt;

            // Apply slight gravity for rising sparks mode.
            if self.mode == EmissionMode::RisingSparks {
                p.vy += 30.0 * dt; // Gentle downward pull
            }

            // Apply slight drag for radial burst.
            if self.mode == EmissionMode::RadialBurst {
                p.vx *= 1.0 - 0.5 * dt;
                p.vy *= 1.0 - 0.5 * dt;
            }
        }

        // Remove dead particles (swap-remove for efficiency).
        self.particles.retain(|p| p.is_alive());
    }

    /// Spawn a single particle based on the current emission mode.
    fn spawn_particle(&mut self, width: u32, height: u32) {
        let w = width as f32;
        let h = height as f32;

        match self.mode {
            EmissionMode::Off => {}
            EmissionMode::TextLine => {
                // Emit from random position along the text line, rising upward.
                let x = self.center_x - self.line_width / 2.0
                    + self.rand_f32() * self.line_width;
                let y = self.center_y;
                let vx = (self.rand_f32() - 0.5) * 20.0;
                let vy = -20.0 - self.rand_f32() * 40.0; // Upward
                let life = 1.0 + self.rand_f32() * 1.5;
                let size = 1.0 + self.rand_f32() * 2.0;
                // Golden particle colors
                let r = 255;
                let g = (180 + (self.rand_f32() * 75.0) as u8) as u8;
                let b = (self.rand_f32() * 100.0) as u8;
                self.particles.push(Particle::new(x, y, vx, vy, life, size, r, g, b));
            }
            EmissionMode::RisingSparks => {
                // Emit from bottom of screen, rising upward.
                let x = self.rand_f32() * w;
                let y = h + 5.0;
                let vx = (self.rand_f32() - 0.5) * 30.0;
                let vy = -50.0 - self.rand_f32() * 80.0;
                let life = 2.0 + self.rand_f32() * 2.0;
                let size = 1.0 + self.rand_f32() * 2.0;
                let color_choice = self.rand_f32();
                let (r, g, b) = if color_choice < 0.5 {
                    (255, 215, 0) // Gold
                } else if color_choice < 0.8 {
                    (255, 180, 50) // Orange-gold
                } else {
                    (255, 255, 200) // Light gold
                };
                self.particles.push(Particle::new(x, y, vx, vy, life, size, r, g, b));
            }
            EmissionMode::RadialBurst => {
                // Emit radially from center.
                let angle = self.rand_f32() * std::f32::consts::TAU;
                let speed = 30.0 + self.rand_f32() * 80.0;
                let vx = angle.cos() * speed;
                let vy = angle.sin() * speed;
                let life = 1.5 + self.rand_f32() * 1.5;
                let size = 1.5 + self.rand_f32() * 2.5;
                let r = 255;
                let g = (150 + (self.rand_f32() * 105.0) as u8) as u8;
                let b = (self.rand_f32() * 80.0) as u8;
                self.particles
                    .push(Particle::new(self.center_x, self.center_y, vx, vy, life, size, r, g, b));
            }
            EmissionMode::Ambient => {
                // Emit from random edges.
                let edge = (self.rand_f32() * 4.0) as u8;
                let (x, y, vx, vy) = match edge {
                    0 => {
                        // Top edge
                        let x = self.rand_f32() * w;
                        (x, 0.0, (self.rand_f32() - 0.5) * 10.0, 10.0 + self.rand_f32() * 20.0)
                    }
                    1 => {
                        // Bottom edge
                        let x = self.rand_f32() * w;
                        (x, h, (self.rand_f32() - 0.5) * 10.0, -(10.0 + self.rand_f32() * 20.0))
                    }
                    2 => {
                        // Left edge
                        let y = self.rand_f32() * h;
                        (0.0, y, 10.0 + self.rand_f32() * 20.0, (self.rand_f32() - 0.5) * 10.0)
                    }
                    _ => {
                        // Right edge
                        let y = self.rand_f32() * h;
                        (w, y, -(10.0 + self.rand_f32() * 20.0), (self.rand_f32() - 0.5) * 10.0)
                    }
                };
                let life = 3.0 + self.rand_f32() * 2.0;
                let size = 1.0 + self.rand_f32() * 1.5;
                let brightness = 100 + (self.rand_f32() * 155.0) as u8;
                self.particles
                    .push(Particle::new(x, y, vx, vy, life, size, brightness, brightness, brightness));
            }
        }
    }

    /// Render all active particles into the framebuffer.
    pub fn render(&self, framebuffer: &mut [u8], width: u32, height: u32) {
        let w = width as i32;
        let h = height as i32;

        for p in &self.particles {
            if !p.is_alive() {
                continue;
            }

            let alpha = p.alpha();
            if alpha <= 0.0 {
                continue;
            }

            let px = p.x as i32;
            let py = p.y as i32;
            let radius = p.size as i32;

            // Draw a small filled circle (or square for size <= 1).
            if radius <= 1 {
                Self::plot_blend(framebuffer, w, h, px, py, p.r, p.g, p.b, alpha);
            } else {
                // Draw a filled circle using the midpoint algorithm (simplified).
                for dy in -radius..=radius {
                    for dx in -radius..=radius {
                        if dx * dx + dy * dy <= radius * radius {
                            Self::plot_blend(
                                framebuffer,
                                w,
                                h,
                                px + dx,
                                py + dy,
                                p.r,
                                p.g,
                                p.b,
                                alpha,
                            );
                        }
                    }
                }
            }
        }
    }

    /// Plot a pixel with alpha blending (source-over).
    fn plot_blend(
        fb: &mut [u8],
        w: i32,
        h: i32,
        x: i32,
        y: i32,
        r: u8,
        g: u8,
        b: u8,
        alpha: f32,
    ) {
        if x < 0 || x >= w || y < 0 || y >= h {
            return;
        }
        let idx = ((y as usize) * (w as usize) + (x as usize)) * 4;
        if idx + 3 >= fb.len() {
            return;
        }
        let inv = 1.0 - alpha;
        fb[idx] = (r as f32 * alpha + fb[idx] as f32 * inv) as u8;
        fb[idx + 1] = (g as f32 * alpha + fb[idx + 1] as f32 * inv) as u8;
        fb[idx + 2] = (b as f32 * alpha + fb[idx + 2] as f32 * inv) as u8;
        fb[idx + 3] = 255;
    }

    /// Get the number of active particles.
    pub fn count(&self) -> usize {
        self.particles.iter().filter(|p| p.is_alive()).count()
    }

    /// Clear all particles.
    pub fn clear(&mut self) {
        self.particles.clear();
    }
}

impl Default for ParticleSystem {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn particle_system_starts_empty() {
        let ps = ParticleSystem::new();
        assert_eq!(ps.count(), 0);
        assert_eq!(ps.mode, EmissionMode::Off);
    }

    #[test]
    fn particle_life_decreases() {
        let mut p = Particle::new(0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 255, 215, 0);
        assert!(p.is_alive());
        p.life -= 0.5;
        assert!(p.is_alive());
        p.life -= 0.5;
        assert!(!p.is_alive());
    }

    #[test]
    fn particle_alpha_fades() {
        let p = Particle::new(0.0, 0.0, 0.0, 0.0, 2.0, 2.0, 255, 215, 0);
        assert!((p.alpha() - 1.0).abs() < 0.001);
        let mut p2 = p;
        p2.life = 1.0;
        assert!((p2.alpha() - 0.5).abs() < 0.001);
    }

    #[test]
    fn emission_mode_off_produces_no_particles() {
        let mut ps = ParticleSystem::new();
        ps.set_mode(EmissionMode::Off);
        ps.update(1.0, 800, 600);
        assert_eq!(ps.count(), 0);
    }

    #[test]
    fn text_line_mode_emits_particles() {
        let mut ps = ParticleSystem::new();
        ps.set_mode(EmissionMode::TextLine);
        ps.set_emission_rate(100.0);
        ps.set_center(400.0, 300.0);
        ps.set_line_width(300.0);
        ps.update(0.1, 800, 600); // 0.1s → ~10 particles
        assert!(ps.count() > 0, "Expected particles to be emitted");
    }

    #[test]
    fn rising_sparks_mode_emits_from_bottom() {
        let mut ps = ParticleSystem::new();
        ps.set_mode(EmissionMode::RisingSparks);
        ps.set_emission_rate(200.0);
        // Use a very small dt so particles don't move much from their spawn position.
        ps.update(0.01, 800, 600);
        assert!(ps.count() > 0);
        // Particles should start near bottom of screen (within 20px for the small dt).
        for p in &ps.particles {
            assert!(p.y > 580.0, "Rising sparks should start at bottom, got y={}", p.y);
        }
    }

    #[test]
    fn radial_burst_emits_from_center() {
        let mut ps = ParticleSystem::new();
        ps.set_mode(EmissionMode::RadialBurst);
        ps.set_emission_rate(100.0);
        ps.set_center(400.0, 300.0);
        // Use a very small dt so particles don't move much from their spawn position.
        ps.update(0.01, 800, 600);
        assert!(ps.count() > 0);
        // All particles should start at center (within 5px for the small dt).
        for p in &ps.particles {
            assert!((p.x - 400.0).abs() < 5.0, "Radial burst should start at center_x, got x={}", p.x);
            assert!((p.y - 300.0).abs() < 5.0, "Radial burst should start at center_y, got y={}", p.y);
        }
    }

    #[test]
    fn ambient_mode_emits_particles() {
        let mut ps = ParticleSystem::new();
        ps.set_mode(EmissionMode::Ambient);
        ps.set_emission_rate(100.0);
        ps.update(0.1, 800, 600);
        assert!(ps.count() > 0);
    }

    #[test]
    fn max_particles_enforced() {
        let mut ps = ParticleSystem::new();
        ps.set_mode(EmissionMode::RadialBurst);
        ps.set_emission_rate(10000.0); // Very high rate
        ps.set_center(400.0, 300.0);
        ps.update(1.0, 800, 600);
        assert!(ps.count() <= MAX_PARTICLES, "Should not exceed MAX_PARTICLES");
    }

    #[test]
    fn particles_die_over_time() {
        let mut ps = ParticleSystem::new();
        ps.set_mode(EmissionMode::TextLine);
        ps.set_emission_rate(10.0);
        ps.set_center(400.0, 300.0);
        ps.set_line_width(300.0);
        ps.update(0.5, 800, 600); // Emit some
        let count_after_emit = ps.count();
        assert!(count_after_emit > 0);
        ps.set_mode(EmissionMode::Off); // Stop emitting
        ps.update(10.0, 800, 600); // Wait for all to die
        assert_eq!(ps.count(), 0, "All particles should be dead after 10s");
    }

    #[test]
    fn clear_removes_all_particles() {
        let mut ps = ParticleSystem::new();
        ps.set_mode(EmissionMode::TextLine);
        ps.set_emission_rate(100.0);
        ps.set_center(400.0, 300.0);
        ps.set_line_width(300.0);
        ps.update(0.1, 800, 600);
        assert!(ps.count() > 0);
        ps.clear();
        assert_eq!(ps.count(), 0);
    }

    #[test]
    fn render_does_not_crash() {
        let mut ps = ParticleSystem::new();
        ps.set_mode(EmissionMode::RadialBurst);
        ps.set_emission_rate(50.0);
        ps.set_center(400.0, 300.0);
        ps.update(0.5, 800, 600);
        let mut fb = vec![0u8; 800 * 600 * 4];
        ps.render(&mut fb, 800, 600);
        // Should have some non-black pixels.
        let non_black = fb.chunks_exact(4).filter(|c| c[0] > 0 || c[1] > 0 || c[2] > 0).count();
        assert!(non_black > 0, "Particle render should produce visible pixels");
    }

    #[test]
    fn emission_rate_clamped() {
        let mut ps = ParticleSystem::new();
        ps.set_emission_rate(-10.0);
        assert_eq!(ps.emission_rate, 0.0);
        ps.set_emission_rate(10000.0);
        assert_eq!(ps.emission_rate, 500.0);
    }

    #[test]
    fn render_respects_bounds() {
        let mut ps = ParticleSystem::new();
        // Manually add a particle outside bounds.
        ps.particles.push(Particle::new(-100.0, -100.0, 0.0, 0.0, 1.0, 5.0, 255, 255, 255));
        let mut fb = vec![0u8; 10 * 10 * 4];
        ps.render(&mut fb, 10, 10);
        // Should not panic or write out of bounds.
    }
}
