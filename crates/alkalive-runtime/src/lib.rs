//! AlkALive runtime — the integration spine.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// The five-phase bootstrap sequence (§3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BootstrapSequence {
    /// 1. Streaming HTTP download of the WASM binary.
    Fetch,
    /// 2. Incremental WASM validation + AOT compile (overlaps phase 1).
    StreamingDecode,
    /// 3. Async WebGPU shader precompilation (overlaps phase 2).
    PipelinePrecompile,
    /// 4. Linear memory + SAB + COOP/COEP gate.
    MemorySabSetup,
    /// 5. Layout → render-graph → submit → present.
    FirstFrame,
}

/// Bootstrap failure modes (§3.6).
#[derive(Debug, Clone)]
pub enum BootstrapError {
    /// The host failed to stream the WASM binary.
    FetchError {
        /// URL that failed.
        url: String,
        /// HTTP status code returned, if any.
        status: u16,
    },
    /// Incremental WASM validation failed for a section.
    DecodeError {
        /// Section identifier that failed validation.
        section: String,
        /// Underlying validation failure message.
        cause: String,
    },
    /// WebGPU shader precompilation failed; degrades to runtime compile (non-fatal).
    PipelineCompileError {
        /// Shader identifier that failed to precompile.
        shader: String,
        /// Compiler diagnostic message.
        msg: String,
    },
    /// Cross-origin isolation (COOP/COEP) is unavailable; SAB blocked (fatal).
    CrossOriginIsolationUnavailable {
        /// The missing or malformed isolation header.
        header: String,
    },
    /// No suitable GPU adapter was available.
    GpuUnavailable {
        /// Reason the adapter request failed.
        adapter_reason: String,
    },
    /// The first-frame budget elapsed before bootstrap completed.
    FirstFrameTimeout {
        /// Phase at which the timeout occurred.
        phase: BootstrapSequence,
        /// Budget in milliseconds that was exceeded.
        budget_ms: u32,
    },
}

/// The top-level Runtime struct (§3.6) — a container for all subsystems.
/// Wave B: struct definition + phase tracking only; full wiring in later waves.
#[derive(Debug)]
pub struct Runtime {
    /// Current bootstrap phase.
    pub bootstrap_phase: BootstrapSequence,
    /// Whether the runtime has completed bootstrap.
    pub is_ready: bool,
}

impl Runtime {
    /// Create a new Runtime at the Fetch phase.
    pub fn new() -> Self {
        Self {
            bootstrap_phase: BootstrapSequence::Fetch,
            is_ready: false,
        }
    }

    /// Advance to the next bootstrap phase. Returns Err if the sequence is violated.
    ///
    /// Bootstrap is considered complete the moment the runtime transitions
    /// **into** `FirstFrame` (§3.4 phase 5): the `MemorySabSetup → FirstFrame`
    /// arm sets `is_ready = true`. A subsequent call from `FirstFrame` is an
    /// idempotent no-op (the runtime stays ready).
    pub fn advance_bootstrap(&mut self) -> Result<(), BootstrapError> {
        use BootstrapSequence::*;
        self.bootstrap_phase = match self.bootstrap_phase {
            Fetch => StreamingDecode,
            StreamingDecode => PipelinePrecompile,
            PipelinePrecompile => MemorySabSetup,
            MemorySabSetup => {
                self.is_ready = true;
                FirstFrame
            }
            FirstFrame => FirstFrame,
        };
        Ok(())
    }

    /// Check if bootstrap is complete.
    pub fn is_ready(&self) -> bool {
        self.is_ready
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

/// A simple frame-loop driver that tracks frame count and elapsed time (§3.5).
/// Wave B: frame counting only; full layout→compile→submit wiring in later waves.
#[derive(Debug, Default)]
pub struct FrameLoopDriver {
    /// Total frames rendered.
    pub frame_count: u64,
    /// Total elapsed time in seconds.
    pub elapsed: f32,
}

impl FrameLoopDriver {
    /// Create a new frame-loop driver.
    pub fn new() -> Self {
        Self::default()
    }

    /// Advance one frame by `dt` seconds.
    pub fn tick(&mut self, dt: f32) {
        self.frame_count += 1;
        self.elapsed += dt;
    }

    /// Total frames rendered.
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_starts_at_fetch() {
        let rt = Runtime::new();
        assert_eq!(rt.bootstrap_phase, BootstrapSequence::Fetch);
        assert!(!rt.is_ready());
    }

    #[test]
    fn bootstrap_advances_through_all_phases() {
        let mut rt = Runtime::new();
        rt.advance_bootstrap().unwrap();
        assert_eq!(rt.bootstrap_phase, BootstrapSequence::StreamingDecode);
        rt.advance_bootstrap().unwrap();
        assert_eq!(rt.bootstrap_phase, BootstrapSequence::PipelinePrecompile);
        rt.advance_bootstrap().unwrap();
        assert_eq!(rt.bootstrap_phase, BootstrapSequence::MemorySabSetup);
        rt.advance_bootstrap().unwrap();
        assert_eq!(rt.bootstrap_phase, BootstrapSequence::FirstFrame);
        assert!(rt.is_ready());
    }

    #[test]
    fn frame_loop_counts_frames() {
        let mut fl = FrameLoopDriver::new();
        assert_eq!(fl.frame_count(), 0);
        fl.tick(0.016);
        fl.tick(0.016);
        assert_eq!(fl.frame_count(), 2);
        assert!((fl.elapsed - 0.032).abs() < 0.001);
    }
}
