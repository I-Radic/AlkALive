//! AlkALive alkalive-perf crate.
//!
//! Performance & resource budget trait surface — see
//! `docs/SPECIFICATION.md` §12 (Performance & Resource Budgets).
//! Realises ADR 016 (single author-owned trace + frame-budget watchdog)
//! and the §12.7 budget table.
//!
//! Wave 10: adds [`LinearMemoryPool`], a concrete [`MemoryPool`] for WASM
//! linear memory (HardCap enforcement → [`BudgetBreach::LinearMemoryCeiling`]).
//! Other pool implementations (SAB, atlas, GPU) remain deferred to later
//! waves. This crate is independent of `alkalive-error`: a local [`SpanKind`]
//! enum is defined here rather than re-exported.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

// ============================================================================
// Enums
// ============================================================================

/// Policy selected when a frame or resource budget is breached (§12.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreachPolicy {
    /// Drop the offending work; preserve cadence.
    Drop,
    /// Clamp the work to fit the remaining budget.
    Clamp,
    /// Emit a trace span and continue; no recovery action.
    Trace,
}

/// Concrete budget breach kind, surfaced as a [`PerfCounter`] breach and as
/// a `TraceSpan` on the unified timeline (§12.6 / §12.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetBreach {
    /// Frame total exceeded 16.7 ms (60 fps) or 8.3 ms (120 fps).
    FrameOverrun,
    /// First-frame startup exceeded 400 ms (§12.1).
    StartupOverrun,
    /// WASM linear memory ceiling hit (HardCap, 256 MB).
    LinearMemoryCeiling,
    /// SAB scene-graph budget exhausted (Backpressure, 64 MB).
    SABExhausted,
    /// Glyph atlas LRU evicted entries (32 MB cap).
    GlyphAtlasEvict,
    /// GPU attachment pool exhausted at `begin_pass`.
    AttachmentPoolEmpty,
    /// Pipeline cache LRU full (64 MB cap).
    PipelineCacheFull,
    /// Per-stage instance buffer overflow; forces dirty-rect flush.
    InstanceBufferFull,
    /// Draw-call count exceeded `draw_call_cap`.
    DrawCallBudget,
    /// Fill rate exceeded `fill_rate_cap_px`.
    FillRateBudget,
}

/// Kind of performance metric recorded by a [`PerfCounter`] (§12.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerfMetricKind {
    /// Total frame time in milliseconds.
    FrameTotalMs,
    /// Per-stage frame time in milliseconds.
    FrameStageMs,
    /// Streaming WASM decode time (startup).
    StartupDecodeMs,
    /// WebGPU pipeline precompile time (startup).
    StartupPipelineCompileMs,
    /// Draw calls submitted per frame.
    DrawCallCount,
    /// Fill rate in pixels per frame.
    FillRatePx,
    /// Triangles submitted per frame.
    TriangleCount,
    /// WASM linear memory usage in bytes.
    LinearMemoryBytes,
    /// SAB scene-graph usage in bytes.
    SABBytes,
    /// Glyph atlas usage in bytes.
    GlyphAtlasBytes,
    /// Pipeline cache usage in bytes.
    PipelineCacheBytes,
    /// Per-stage instance buffer usage in bytes.
    InstanceBufferBytes,
    /// Total GPU memory usage in bytes.
    GPUMemoryBytes,
}

/// Kind of [`MemoryPool`] backing a [`ResourceBudget`] (§12.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolKind {
    /// WASM linear memory.
    Linear,
    /// SharedArrayBuffer scene-graph staging.
    SAB,
    /// Glyph atlas.
    Atlas,
    /// GPU attachment / pipeline / instance pool.
    GPU,
}

/// Enforcement mechanism for a [`ResourceBudget`] (§12.6 / §12.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Enforcement {
    /// Reject growth past the cap (`LinearMemoryCeiling`).
    HardCap,
    /// Evict least-recently-used entries (`GlyphAtlasEvict`, `PipelineCacheFull`).
    LRU,
    /// Apply backpressure to producers (`SABExhausted`).
    Backpressure,
    /// Reject the requesting operation (`AttachmentPoolEmpty`, `InstanceBufferFull`).
    Reject,
}

/// Render-pipeline stage identifier. The stage a [`TraceSpan`] attributes to
/// (§12.6: text | layout | graph | compositor | draw).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StageId {
    /// Text shaping / measurement.
    Text,
    /// Layout solve.
    Layout,
    /// Render-graph compile / merge / batch.
    Graph,
    /// Compositor commit.
    Compositor,
    /// Backend draw submission.
    Draw,
}

/// Resource key for a [`ResourceBudget`] (§12.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceKey {
    /// WASM linear memory.
    LinearMem,
    /// SAB scene-graph.
    SABScene,
    /// Glyph atlas.
    GlyphAtlas,
    /// Pipeline cache.
    PipelineCache,
    /// Per-stage instance buffer.
    InstanceBuf,
    /// GPU attachment pool.
    AttachmentPool,
}

/// Owning ADR for a [`ResourceBudget`] (§12.7 budget table).
///
/// Each variant corresponds to one of the 22 architectural decision records
/// in `docs/adr/ADR.md`. Used to attribute a budget breach to the ADR that
/// owns the cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AdrId {
    /// ADR 001 — Render-graph IR.
    Adr001,
    /// ADR 002 — Dirty-rect locality.
    Adr002,
    /// ADR 003 — Main-thread GPU ownership.
    Adr003,
    /// ADR 004 — Pluggable layout solver.
    Adr004,
    /// ADR 005 — Style system.
    Adr005,
    /// ADR 006 — Text shaping.
    Adr006,
    /// ADR 007 — Owned subtree.
    Adr007,
    /// ADR 008 — WASM sandbox.
    Adr008,
    /// ADR 009 — Two-level type verification.
    Adr009,
    /// ADR 010 — CPU hit-test.
    Adr010,
    /// ADR 011 — Interaction model.
    Adr011,
    /// ADR 012 — Navigation contract.
    Adr012,
    /// ADR 013 — No DOM on hot path.
    Adr013,
    /// ADR 014 — Typed component contracts.
    Adr014,
    /// ADR 015 — Module lifecycle / HMR.
    Adr015,
    /// ADR 016 — Unified author-owned trace.
    Adr016,
    /// ADR 017 — Streaming decode + pipeline precompile.
    Adr017,
    /// ADR 018 — Capability-scoped imports.
    Adr018,
    /// ADR 019 — Accessibility deferred.
    Adr019,
    /// ADR 020 — Metadata-only DOM.
    Adr020,
    /// ADR 021 — Main thread + on-demand WASM workers, socket IPC.
    Adr021,
    /// ADR 022 — Forked HarfRust text stack.
    Adr022,
}

/// Kind of trace span opened on the unified timeline.
///
/// Local to this crate to keep `alkalive-perf` independent of
/// `alkalive-error` (which defines its own `SpanKind` for the error surface).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpanKind {
    /// Top-level frame span.
    Frame,
    /// Per-stage span within a frame.
    Stage,
    /// Frame-budget watchdog span.
    Budget,
    /// Watchdog-raised overrun span.
    Watchdog,
    /// Budget breach span.
    Breach,
    /// Memory eviction span.
    Eviction,
}

// ============================================================================
// Structs
// ============================================================================

/// Per-frame budget policing 60 fps (16.7 ms) or 120 fps (8.3 ms) plus
/// per-stage ceilings and GPU-side draw-call / fill-rate caps (§12.2 / §12.6).
#[derive(Debug, Clone)]
pub struct FrameBudget {
    /// Target frame rate: 60 or 120.
    pub target_fps: u16,
    /// Target frame time in milliseconds: 16.7 or 8.3.
    pub target_ms: f32,
    /// Per-span ceiling in milliseconds, keyed by [`StageId`].
    ///
    /// Stand-in for `Map<StageId, f32>`; the real container lands with the
    /// collections facade (§2.8).
    pub stage_limits: Vec<(StageId, f32)>,
    /// GPU-side draw-call cap, stage-tuned.
    pub draw_call_cap: u32,
    /// GPU-side fill-rate cap in pixels per frame.
    pub fill_rate_cap_px: u64,
    /// Policy applied on overrun.
    pub overrun_policy: BreachPolicy,
}

/// Resource budget row from the §12.7 budget table.
#[derive(Debug, Clone)]
pub struct ResourceBudget {
    /// Which resource is bounded.
    pub key: ResourceKey,
    /// Hard cap in bytes.
    pub limit_bytes: u64,
    /// Owning ADR.
    pub owning_adr: AdrId,
    /// Enforcement mechanism.
    pub enforcement: Enforcement,
    /// Live gauge of current usage in bytes.
    pub current_bytes: u64,
}

/// A single performance counter sample attributed to a frame and optionally
/// to a [`TraceSpan`] (§12.6).
#[derive(Debug, Clone)]
pub struct PerfCounter {
    /// Which metric this sample records.
    pub kind: PerfMetricKind,
    /// Sampled value.
    pub value: f64,
    /// Frame this sample belongs to.
    pub frame_id: u64,
    /// Span this sample is attributed to, if any.
    pub span: Option<TraceSpanId>,
    /// Breach flagged by this sample, if any.
    pub breach: Option<BudgetBreach>,
}

/// Identifier of a [`TraceSpan`] on the unified timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TraceSpanId(());

/// A span on the unified author-owned trace (ADR 016).
///
/// Every stage opens / closes a `TraceSpan` on one frame-aligned timeline;
/// per-stage span timing is the root-cause surface (§12.5).
#[derive(Debug, Clone)]
pub struct TraceSpan {
    /// Span identifier.
    pub id: TraceSpanId,
    /// Render-pipeline stage this span attributes to.
    pub stage: StageId,
    /// Frame this span belongs to.
    pub frame_id: u64,
    /// Open timestamp in microseconds (MonotonicClock).
    pub open_us: u64,
    /// Close timestamp in microseconds (MonotonicClock).
    pub close_us: u64,
    /// Ceiling for this stage in milliseconds.
    pub budget_ms: f32,
    /// Parent span, if nested.
    pub parent: Option<TraceSpanId>,
    /// Counters attributed to this span.
    pub counters: Vec<PerfCounter>,
}

/// A reserved region of a [`MemoryPool`], returned by [`MemoryPool::reserve`].
#[derive(Debug, Clone, Copy)]
pub struct Region {
    /// Offset from the pool base in bytes.
    pub offset: u64,
    /// Length in bytes.
    pub len: u64,
    /// Which pool this region belongs to.
    pub kind: PoolKind,
}

/// Statistics reported by [`MemoryPool::evict_lru`].
#[derive(Debug, Clone, Copy)]
pub struct EvictionStats {
    /// Bytes freed by the eviction pass.
    pub evicted_bytes: u64,
    /// Number of entries evicted.
    pub evicted_entries: u64,
    /// Which pool was evicted.
    pub kind: PoolKind,
}

/// Event returned by a frame-budget watchdog; carries the watched span,
/// the budget, the elapsed time, and any breach flagged.
#[derive(Debug, Clone)]
pub struct FrameBudgetEvent {
    /// Span watching the frame budget.
    pub span: TraceSpanId,
    /// Budget ceiling in milliseconds.
    pub budget_ms: f32,
    /// Elapsed frame time in milliseconds.
    pub elapsed_ms: f32,
    /// Breach flagged, if any.
    pub breach: Option<BudgetBreach>,
}

// ============================================================================
// Traits
// ============================================================================

/// Memory pool backing one row of the §12.7 budget table.
///
/// `reserve` rejects growth past the cap according to the pool's
/// [`Enforcement`]; `release` returns a [`Region`] to the pool;
/// `evict_lru` reclaims at least `target_bytes` via LRU eviction and
/// reports the eviction statistics.
pub trait MemoryPool {
    /// Which pool this is.
    fn kind(&self) -> PoolKind;
    /// Hard cap in bytes.
    fn cap_bytes(&self) -> u64;
    /// Currently used bytes.
    fn used_bytes(&self) -> u64;
    /// High-water mark in bytes.
    fn high_water_bytes(&self) -> u64;
    /// Reserve `n` bytes; rejects with a [`BudgetBreach`] on overflow.
    fn reserve(&mut self, n: u64) -> Result<Region, BudgetBreach>;
    /// Release a previously reserved [`Region`] back to the pool.
    fn release(&mut self, region: Region);
    /// Evict least-recently-used entries until at least `target_bytes`
    /// are free; returns the eviction statistics.
    fn evict_lru(&mut self, target_bytes: u64) -> EvictionStats;
}

// ============================================================================
// Concrete implementations (Wave 10)
// ============================================================================

/// Concrete [`MemoryPool`] for WASM linear memory (§12.7 — HardCap, 256 MB).
///
/// `reserve` rejects growth past `cap_bytes` with
/// [`BudgetBreach::LinearMemoryCeiling`]; `release` decrements the used
/// counter; `evict_lru` is a no-op (linear memory is not LRU-evictable —
/// callers grow or shrink the linear memory, they do not evict entries
/// from it).
#[derive(Debug, Clone)]
pub struct LinearMemoryPool {
    /// Hard cap in bytes.
    cap_bytes: u64,
    /// Currently used bytes.
    used_bytes: u64,
    /// High-water mark in bytes (peak `used_bytes` observed).
    high_water_bytes: u64,
}

impl LinearMemoryPool {
    /// Create a new linear-memory pool with the given hard cap in bytes.
    pub fn new(cap_bytes: u64) -> Self {
        Self {
            cap_bytes,
            used_bytes: 0,
            high_water_bytes: 0,
        }
    }
}

impl MemoryPool for LinearMemoryPool {
    fn kind(&self) -> PoolKind {
        PoolKind::Linear
    }

    fn cap_bytes(&self) -> u64 {
        self.cap_bytes
    }

    fn used_bytes(&self) -> u64 {
        self.used_bytes
    }

    fn high_water_bytes(&self) -> u64 {
        self.high_water_bytes
    }

    fn reserve(&mut self, n: u64) -> Result<Region, BudgetBreach> {
        // `checked_add` rejects both cap overflow and `u64` overflow.
        match self.used_bytes.checked_add(n) {
            Some(new_used) if new_used <= self.cap_bytes => {
                let offset = self.used_bytes;
                self.used_bytes = new_used;
                if new_used > self.high_water_bytes {
                    self.high_water_bytes = new_used;
                }
                Ok(Region {
                    offset,
                    len: n,
                    kind: PoolKind::Linear,
                })
            }
            _ => Err(BudgetBreach::LinearMemoryCeiling),
        }
    }

    fn release(&mut self, region: Region) {
        // Saturating subtract guards against underflow from a malformed
        // (oversized) release; production callers should always release
        // exactly what they reserved.
        self.used_bytes = self.used_bytes.saturating_sub(region.len);
    }

    fn evict_lru(&mut self, _target_bytes: u64) -> EvictionStats {
        // Linear memory is not LRU-evictable: eviction is a no-op.
        EvictionStats {
            evicted_bytes: 0,
            evicted_entries: 0,
            kind: PoolKind::Linear,
        }
    }
}

// ============================================================================
// Concrete implementations (Wave F — Gap H11)
// ============================================================================
//
// The §12.7 budget table mandates five more pools besides WASM linear memory:
//
// | Pool                | Cap    | Enforcement  | Breach on overflow        |
// |---------------------|--------|--------------|---------------------------|
// | SABPool             | 64 MB  | Backpressure | SABExhausted              |
// | GlyphAtlasPool      | 32 MB  | LRU          | GlyphAtlasEvict           |
// | PipelineCachePool   | 64 MB  | LRU          | PipelineCacheFull         |
// | AttachmentPool      | 256 MB | Reject       | AttachmentPoolEmpty       |
// | InstanceBufferPool  | 128 MB | Reject       | InstanceBufferFull        |
//
// `reserve` rejects growth past the cap with the pool's designated
// [`BudgetBreach`]. LRU pools additionally track reserved regions in a `Vec`
// (front = oldest, back = newest) so that `evict_lru` actually evicts entries;
// the non-LRU pools' `evict_lru` is a no-op. `release` returns a region to the
// pool (and, for LRU pools, removes the matching entry from the tracking Vec).

/// 64 MB cap, `Backpressure` enforcement → [`BudgetBreach::SABExhausted`].
///
/// Backpressure pools are not LRU-evictable: `evict_lru` is a no-op and the
/// caller is expected to yield and retry on `Err(SABExhausted)`.
#[derive(Debug, Clone)]
pub struct SABPool {
    /// Hard cap in bytes (default 64 MB per §12.7).
    cap_bytes: u64,
    /// Currently used bytes.
    used_bytes: u64,
    /// High-water mark in bytes (peak `used_bytes` observed).
    high_water_bytes: u64,
}

impl SABPool {
    /// 64 MB cap, the §12.7 default for the SAB scene-graph budget.
    const DEFAULT_CAP: u64 = 64 * 1024 * 1024;

    /// Create a pool with the §12.7 default 64 MB cap.
    pub fn new() -> Self {
        Self::with_cap(Self::DEFAULT_CAP)
    }

    /// Create a pool with a custom cap in bytes (primarily for tests).
    pub fn with_cap(cap_bytes: u64) -> Self {
        Self {
            cap_bytes,
            used_bytes: 0,
            high_water_bytes: 0,
        }
    }
}

impl Default for SABPool {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryPool for SABPool {
    fn kind(&self) -> PoolKind {
        PoolKind::SAB
    }

    fn cap_bytes(&self) -> u64 {
        self.cap_bytes
    }

    fn used_bytes(&self) -> u64 {
        self.used_bytes
    }

    fn high_water_bytes(&self) -> u64 {
        self.high_water_bytes
    }

    fn reserve(&mut self, n: u64) -> Result<Region, BudgetBreach> {
        match self.used_bytes.checked_add(n) {
            Some(new_used) if new_used <= self.cap_bytes => {
                let offset = self.used_bytes;
                self.used_bytes = new_used;
                if new_used > self.high_water_bytes {
                    self.high_water_bytes = new_used;
                }
                Ok(Region {
                    offset,
                    len: n,
                    kind: PoolKind::SAB,
                })
            }
            _ => Err(BudgetBreach::SABExhausted),
        }
    }

    fn release(&mut self, region: Region) {
        self.used_bytes = self.used_bytes.saturating_sub(region.len);
    }

    fn evict_lru(&mut self, _target_bytes: u64) -> EvictionStats {
        // Backpressure pools are not LRU-evictable: eviction is a no-op.
        EvictionStats {
            evicted_bytes: 0,
            evicted_entries: 0,
            kind: PoolKind::SAB,
        }
    }
}

/// 32 MB cap, `LRU` enforcement → [`BudgetBreach::GlyphAtlasEvict`] on
/// overflow.
///
/// Reserved regions are tracked in a `Vec<(offset, len)>` (front = oldest,
/// back = newest). `evict_lru` pops from the front until `target_bytes` are
/// free; `release` removes the matching entry so the tracking Vec stays
/// consistent with `used_bytes`.
#[derive(Debug, Clone)]
pub struct GlyphAtlasPool {
    /// Hard cap in bytes (default 32 MB per §12.7).
    cap_bytes: u64,
    /// Currently used bytes.
    used_bytes: u64,
    /// High-water mark in bytes (peak `used_bytes` observed).
    high_water_bytes: u64,
    /// LRU entries: front = oldest (next to evict), back = newest.
    entries: Vec<(u64, u64)>,
}

impl GlyphAtlasPool {
    /// 32 MB cap, the §12.7 default for the glyph atlas.
    const DEFAULT_CAP: u64 = 32 * 1024 * 1024;

    /// Create a pool with the §12.7 default 32 MB cap.
    pub fn new() -> Self {
        Self::with_cap(Self::DEFAULT_CAP)
    }

    /// Create a pool with a custom cap in bytes (primarily for tests).
    pub fn with_cap(cap_bytes: u64) -> Self {
        Self {
            cap_bytes,
            used_bytes: 0,
            high_water_bytes: 0,
            entries: Vec::new(),
        }
    }
}

impl Default for GlyphAtlasPool {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryPool for GlyphAtlasPool {
    fn kind(&self) -> PoolKind {
        PoolKind::Atlas
    }

    fn cap_bytes(&self) -> u64 {
        self.cap_bytes
    }

    fn used_bytes(&self) -> u64 {
        self.used_bytes
    }

    fn high_water_bytes(&self) -> u64 {
        self.high_water_bytes
    }

    fn reserve(&mut self, n: u64) -> Result<Region, BudgetBreach> {
        match self.used_bytes.checked_add(n) {
            Some(new_used) if new_used <= self.cap_bytes => {
                let offset = self.used_bytes;
                self.used_bytes = new_used;
                if new_used > self.high_water_bytes {
                    self.high_water_bytes = new_used;
                }
                // Track the new entry at the back (newest).
                self.entries.push((offset, n));
                Ok(Region {
                    offset,
                    len: n,
                    kind: PoolKind::Atlas,
                })
            }
            _ => Err(BudgetBreach::GlyphAtlasEvict),
        }
    }

    fn release(&mut self, region: Region) {
        // Remove the matching entry (by offset) so the tracking Vec stays
        // consistent with `used_bytes`. A missing entry is a no-op for the
        // Vec; the used-byte counter is always decremented.
        if let Some(pos) = self.entries.iter().position(|(o, _)| *o == region.offset) {
            self.entries.remove(pos);
        }
        self.used_bytes = self.used_bytes.saturating_sub(region.len);
    }

    fn evict_lru(&mut self, target_bytes: u64) -> EvictionStats {
        let mut freed = 0u64;
        let mut count = 0u64;
        // Pop oldest entries (front of the Vec) until `target_bytes` are free
        // or the pool is empty.
        while freed < target_bytes {
            match self.entries.first() {
                Some(&(_offset, len)) => {
                    self.entries.remove(0);
                    self.used_bytes = self.used_bytes.saturating_sub(len);
                    freed += len;
                    count += 1;
                }
                None => break,
            }
        }
        EvictionStats {
            evicted_bytes: freed,
            evicted_entries: count,
            kind: PoolKind::Atlas,
        }
    }
}

/// 64 MB cap, `LRU` enforcement → [`BudgetBreach::PipelineCacheFull`] on
/// overflow.
///
/// Structurally identical to [`GlyphAtlasPool`]; only the cap, the
/// [`PoolKind`] (`GPU`), and the breach variant differ.
#[derive(Debug, Clone)]
pub struct PipelineCachePool {
    /// Hard cap in bytes (default 64 MB per §12.7).
    cap_bytes: u64,
    /// Currently used bytes.
    used_bytes: u64,
    /// High-water mark in bytes (peak `used_bytes` observed).
    high_water_bytes: u64,
    /// LRU entries: front = oldest (next to evict), back = newest.
    entries: Vec<(u64, u64)>,
}

impl PipelineCachePool {
    /// 64 MB cap, the §12.7 default for the pipeline cache.
    const DEFAULT_CAP: u64 = 64 * 1024 * 1024;

    /// Create a pool with the §12.7 default 64 MB cap.
    pub fn new() -> Self {
        Self::with_cap(Self::DEFAULT_CAP)
    }

    /// Create a pool with a custom cap in bytes (primarily for tests).
    pub fn with_cap(cap_bytes: u64) -> Self {
        Self {
            cap_bytes,
            used_bytes: 0,
            high_water_bytes: 0,
            entries: Vec::new(),
        }
    }
}

impl Default for PipelineCachePool {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryPool for PipelineCachePool {
    fn kind(&self) -> PoolKind {
        // The `PoolKind` enum lumps pipeline/attachment/instance pools under
        // `GPU` (§12.6); there is no dedicated `Pipeline` variant.
        PoolKind::GPU
    }

    fn cap_bytes(&self) -> u64 {
        self.cap_bytes
    }

    fn used_bytes(&self) -> u64 {
        self.used_bytes
    }

    fn high_water_bytes(&self) -> u64 {
        self.high_water_bytes
    }

    fn reserve(&mut self, n: u64) -> Result<Region, BudgetBreach> {
        match self.used_bytes.checked_add(n) {
            Some(new_used) if new_used <= self.cap_bytes => {
                let offset = self.used_bytes;
                self.used_bytes = new_used;
                if new_used > self.high_water_bytes {
                    self.high_water_bytes = new_used;
                }
                self.entries.push((offset, n));
                Ok(Region {
                    offset,
                    len: n,
                    kind: PoolKind::GPU,
                })
            }
            _ => Err(BudgetBreach::PipelineCacheFull),
        }
    }

    fn release(&mut self, region: Region) {
        if let Some(pos) = self.entries.iter().position(|(o, _)| *o == region.offset) {
            self.entries.remove(pos);
        }
        self.used_bytes = self.used_bytes.saturating_sub(region.len);
    }

    fn evict_lru(&mut self, target_bytes: u64) -> EvictionStats {
        let mut freed = 0u64;
        let mut count = 0u64;
        while freed < target_bytes {
            match self.entries.first() {
                Some(&(_offset, len)) => {
                    self.entries.remove(0);
                    self.used_bytes = self.used_bytes.saturating_sub(len);
                    freed += len;
                    count += 1;
                }
                None => break,
            }
        }
        EvictionStats {
            evicted_bytes: freed,
            evicted_entries: count,
            kind: PoolKind::GPU,
        }
    }
}

/// 256 MB cap, `Reject` enforcement → [`BudgetBreach::AttachmentPoolEmpty`]
/// on overflow.
///
/// Reject pools are not LRU-evictable: `evict_lru` is a no-op and the caller
/// is expected to fail the requesting `begin_pass` on
/// `Err(AttachmentPoolEmpty)`.
#[derive(Debug, Clone)]
pub struct AttachmentPool {
    /// Hard cap in bytes (default 256 MB per §12.7).
    cap_bytes: u64,
    /// Currently used bytes.
    used_bytes: u64,
    /// High-water mark in bytes (peak `used_bytes` observed).
    high_water_bytes: u64,
}

impl AttachmentPool {
    /// 256 MB cap, the §12.7 default for the GPU attachment pool.
    const DEFAULT_CAP: u64 = 256 * 1024 * 1024;

    /// Create a pool with the §12.7 default 256 MB cap.
    pub fn new() -> Self {
        Self::with_cap(Self::DEFAULT_CAP)
    }

    /// Create a pool with a custom cap in bytes (primarily for tests).
    pub fn with_cap(cap_bytes: u64) -> Self {
        Self {
            cap_bytes,
            used_bytes: 0,
            high_water_bytes: 0,
        }
    }
}

impl Default for AttachmentPool {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryPool for AttachmentPool {
    fn kind(&self) -> PoolKind {
        PoolKind::GPU
    }

    fn cap_bytes(&self) -> u64 {
        self.cap_bytes
    }

    fn used_bytes(&self) -> u64 {
        self.used_bytes
    }

    fn high_water_bytes(&self) -> u64 {
        self.high_water_bytes
    }

    fn reserve(&mut self, n: u64) -> Result<Region, BudgetBreach> {
        match self.used_bytes.checked_add(n) {
            Some(new_used) if new_used <= self.cap_bytes => {
                let offset = self.used_bytes;
                self.used_bytes = new_used;
                if new_used > self.high_water_bytes {
                    self.high_water_bytes = new_used;
                }
                Ok(Region {
                    offset,
                    len: n,
                    kind: PoolKind::GPU,
                })
            }
            _ => Err(BudgetBreach::AttachmentPoolEmpty),
        }
    }

    fn release(&mut self, region: Region) {
        self.used_bytes = self.used_bytes.saturating_sub(region.len);
    }

    fn evict_lru(&mut self, _target_bytes: u64) -> EvictionStats {
        EvictionStats {
            evicted_bytes: 0,
            evicted_entries: 0,
            kind: PoolKind::GPU,
        }
    }
}

/// 128 MB cap, `Reject` enforcement → [`BudgetBreach::InstanceBufferFull`]
/// on overflow.
///
/// Structurally identical to [`AttachmentPool`]; only the cap and the breach
/// variant differ.
#[derive(Debug, Clone)]
pub struct InstanceBufferPool {
    /// Hard cap in bytes (default 128 MB per §12.7).
    cap_bytes: u64,
    /// Currently used bytes.
    used_bytes: u64,
    /// High-water mark in bytes (peak `used_bytes` observed).
    high_water_bytes: u64,
}

impl InstanceBufferPool {
    /// 128 MB cap, the §12.7 default for the per-stage instance buffer.
    const DEFAULT_CAP: u64 = 128 * 1024 * 1024;

    /// Create a pool with the §12.7 default 128 MB cap.
    pub fn new() -> Self {
        Self::with_cap(Self::DEFAULT_CAP)
    }

    /// Create a pool with a custom cap in bytes (primarily for tests).
    pub fn with_cap(cap_bytes: u64) -> Self {
        Self {
            cap_bytes,
            used_bytes: 0,
            high_water_bytes: 0,
        }
    }
}

impl Default for InstanceBufferPool {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryPool for InstanceBufferPool {
    fn kind(&self) -> PoolKind {
        PoolKind::GPU
    }

    fn cap_bytes(&self) -> u64 {
        self.cap_bytes
    }

    fn used_bytes(&self) -> u64 {
        self.used_bytes
    }

    fn high_water_bytes(&self) -> u64 {
        self.high_water_bytes
    }

    fn reserve(&mut self, n: u64) -> Result<Region, BudgetBreach> {
        match self.used_bytes.checked_add(n) {
            Some(new_used) if new_used <= self.cap_bytes => {
                let offset = self.used_bytes;
                self.used_bytes = new_used;
                if new_used > self.high_water_bytes {
                    self.high_water_bytes = new_used;
                }
                Ok(Region {
                    offset,
                    len: n,
                    kind: PoolKind::GPU,
                })
            }
            _ => Err(BudgetBreach::InstanceBufferFull),
        }
    }

    fn release(&mut self, region: Region) {
        self.used_bytes = self.used_bytes.saturating_sub(region.len);
    }

    fn evict_lru(&mut self, _target_bytes: u64) -> EvictionStats {
        EvictionStats {
            evicted_bytes: 0,
            evicted_entries: 0,
            kind: PoolKind::GPU,
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- LinearMemoryPool::reserve ---------------------------------------

    #[test]
    fn reserve_succeeds_under_cap() {
        let mut pool = LinearMemoryPool::new(1024);
        let region = pool.reserve(512).expect("reserve under cap should succeed");
        assert_eq!(region.len, 512);
        assert_eq!(region.kind, PoolKind::Linear);
        assert_eq!(pool.used_bytes(), 512);
        assert_eq!(pool.high_water_bytes(), 512);
    }

    #[test]
    fn reserve_exactly_at_cap_succeeds() {
        let mut pool = LinearMemoryPool::new(1024);
        let region = pool
            .reserve(1024)
            .expect("reserve exactly at cap should succeed");
        assert_eq!(region.offset, 0);
        assert_eq!(pool.used_bytes(), 1024);
        assert_eq!(pool.high_water_bytes(), 1024);
    }

    #[test]
    fn reserve_fails_over_cap() {
        let mut pool = LinearMemoryPool::new(1024);
        let _ = pool.reserve(512).unwrap();
        let err = pool.reserve(600).unwrap_err();
        assert_eq!(err, BudgetBreach::LinearMemoryCeiling);
        // used_bytes unchanged after a failed reserve.
        assert_eq!(pool.used_bytes(), 512);
    }

    #[test]
    fn reserve_at_cap_then_one_byte_fails() {
        let mut pool = LinearMemoryPool::new(1024);
        let _ = pool.reserve(1024).unwrap();
        let err = pool.reserve(1).unwrap_err();
        assert_eq!(err, BudgetBreach::LinearMemoryCeiling);
    }

    #[test]
    fn reserve_offsets_increment() {
        let mut pool = LinearMemoryPool::new(1024);
        let r1 = pool.reserve(256).unwrap();
        let r2 = pool.reserve(256).unwrap();
        let r3 = pool.reserve(256).unwrap();
        assert_eq!(r1.offset, 0);
        assert_eq!(r2.offset, 256);
        assert_eq!(r3.offset, 512);
        assert_eq!(pool.used_bytes(), 768);
        assert_eq!(pool.high_water_bytes(), 768);
    }

    // ---- LinearMemoryPool::release ---------------------------------------

    #[test]
    fn release_decrements_used() {
        let mut pool = LinearMemoryPool::new(1024);
        let region = pool.reserve(512).unwrap();
        assert_eq!(pool.used_bytes(), 512);
        pool.release(region);
        assert_eq!(pool.used_bytes(), 0);
        // high_water remains at the peak.
        assert_eq!(pool.high_water_bytes(), 512);
    }

    #[test]
    fn release_allows_re_reserve() {
        let mut pool = LinearMemoryPool::new(1024);
        let r1 = pool.reserve(1024).unwrap();
        pool.release(r1);
        assert_eq!(pool.used_bytes(), 0);
        // After release, the full cap is available again.
        let r2 = pool.reserve(1024).unwrap();
        assert_eq!(r2.offset, 0);
        assert_eq!(pool.used_bytes(), 1024);
    }

    // ---- LinearMemoryPool::evict_lru -------------------------------------

    #[test]
    fn evict_lru_is_noop_for_linear_memory() {
        let mut pool = LinearMemoryPool::new(1024);
        let _ = pool.reserve(512).unwrap();
        let stats = pool.evict_lru(256);
        assert_eq!(stats.evicted_bytes, 0);
        assert_eq!(stats.evicted_entries, 0);
        assert_eq!(stats.kind, PoolKind::Linear);
        // used_bytes unchanged.
        assert_eq!(pool.used_bytes(), 512);
    }

    // ---- LinearMemoryPool::kind / cap_bytes ------------------------------

    #[test]
    fn pool_kind_and_cap() {
        let pool = LinearMemoryPool::new(2048);
        assert_eq!(pool.kind(), PoolKind::Linear);
        assert_eq!(pool.cap_bytes(), 2048);
        assert_eq!(pool.used_bytes(), 0);
        assert_eq!(pool.high_water_bytes(), 0);
    }

    // ====================================================================
    // Wave F — Gap H11: SAB / GlyphAtlas / PipelineCache / Attachment /
    // InstanceBuffer pools
    // ====================================================================

    // ---- default caps per §12.7 -----------------------------------------

    #[test]
    fn sab_pool_default_cap_is_64mb() {
        let pool = SABPool::new();
        assert_eq!(pool.kind(), PoolKind::SAB);
        assert_eq!(pool.cap_bytes(), 64 * 1024 * 1024);
        assert_eq!(pool.used_bytes(), 0);
        assert_eq!(pool.high_water_bytes(), 0);
    }

    #[test]
    fn glyph_atlas_pool_default_cap_is_32mb() {
        let pool = GlyphAtlasPool::new();
        assert_eq!(pool.kind(), PoolKind::Atlas);
        assert_eq!(pool.cap_bytes(), 32 * 1024 * 1024);
    }

    #[test]
    fn pipeline_cache_pool_default_cap_is_64mb() {
        let pool = PipelineCachePool::new();
        assert_eq!(pool.kind(), PoolKind::GPU);
        assert_eq!(pool.cap_bytes(), 64 * 1024 * 1024);
    }

    #[test]
    fn attachment_pool_default_cap_is_256mb() {
        let pool = AttachmentPool::new();
        assert_eq!(pool.kind(), PoolKind::GPU);
        assert_eq!(pool.cap_bytes(), 256 * 1024 * 1024);
    }

    #[test]
    fn instance_buffer_pool_default_cap_is_128mb() {
        let pool = InstanceBufferPool::new();
        assert_eq!(pool.kind(), PoolKind::GPU);
        assert_eq!(pool.cap_bytes(), 128 * 1024 * 1024);
    }

    // ---- SABPool: rejects at cap (Backpressure → SABExhausted) ----------

    #[test]
    fn sab_pool_reserve_under_cap_succeeds() {
        let mut pool = SABPool::with_cap(1024);
        let region = pool.reserve(512).expect("reserve under cap should succeed");
        assert_eq!(region.len, 512);
        assert_eq!(region.kind, PoolKind::SAB);
        assert_eq!(pool.used_bytes(), 512);
        assert_eq!(pool.high_water_bytes(), 512);
    }

    #[test]
    fn sab_pool_rejects_at_cap() {
        let mut pool = SABPool::with_cap(1024);
        let _ = pool.reserve(1024).unwrap();
        let err = pool.reserve(1).unwrap_err();
        assert_eq!(err, BudgetBreach::SABExhausted);
        // used_bytes unchanged after a failed reserve.
        assert_eq!(pool.used_bytes(), 1024);
    }

    #[test]
    fn sab_pool_release_frees_space() {
        let mut pool = SABPool::with_cap(1024);
        let r = pool.reserve(1024).unwrap();
        pool.release(r);
        assert_eq!(pool.used_bytes(), 0);
        // After release, the full cap is available again.
        let _ = pool.reserve(1024).unwrap();
    }

    #[test]
    fn sab_pool_evict_lru_is_noop() {
        let mut pool = SABPool::with_cap(1024);
        let _ = pool.reserve(512).unwrap();
        let stats = pool.evict_lru(256);
        assert_eq!(stats.evicted_bytes, 0);
        assert_eq!(stats.evicted_entries, 0);
        assert_eq!(stats.kind, PoolKind::SAB);
        // used_bytes unchanged.
        assert_eq!(pool.used_bytes(), 512);
    }

    // ---- GlyphAtlasPool: rejects at cap + LRU eviction ------------------

    #[test]
    fn glyph_atlas_pool_reserve_under_cap_succeeds() {
        let mut pool = GlyphAtlasPool::with_cap(1024);
        let region = pool.reserve(256).expect("reserve under cap should succeed");
        assert_eq!(region.kind, PoolKind::Atlas);
        assert_eq!(pool.used_bytes(), 256);
        assert_eq!(pool.high_water_bytes(), 256);
    }

    #[test]
    fn glyph_atlas_pool_rejects_at_cap() {
        let mut pool = GlyphAtlasPool::with_cap(1024);
        let _ = pool.reserve(1024).unwrap();
        let err = pool.reserve(1).unwrap_err();
        assert_eq!(err, BudgetBreach::GlyphAtlasEvict);
        assert_eq!(pool.used_bytes(), 1024);
    }

    #[test]
    fn glyph_atlas_pool_evict_lru_removes_oldest_first() {
        let mut pool = GlyphAtlasPool::with_cap(1024);
        let _r0 = pool.reserve(256).unwrap(); // oldest
        let _r1 = pool.reserve(256).unwrap();
        let _r2 = pool.reserve(256).unwrap(); // newest
        assert_eq!(pool.used_bytes(), 768);

        let stats = pool.evict_lru(256);
        assert_eq!(stats.evicted_bytes, 256);
        assert_eq!(stats.evicted_entries, 1);
        assert_eq!(stats.kind, PoolKind::Atlas);
        // The oldest entry was evicted; used_bytes drops by 256.
        assert_eq!(pool.used_bytes(), 512);

        // Evicting again removes the next-oldest entry.
        let stats2 = pool.evict_lru(256);
        assert_eq!(stats2.evicted_bytes, 256);
        assert_eq!(stats2.evicted_entries, 1);
        assert_eq!(pool.used_bytes(), 256);
    }

    #[test]
    fn glyph_atlas_pool_evict_lru_evicts_multiple_until_target_met() {
        let mut pool = GlyphAtlasPool::with_cap(1024);
        let _ = pool.reserve(128).unwrap();
        let _ = pool.reserve(128).unwrap();
        let _ = pool.reserve(128).unwrap();
        let _ = pool.reserve(128).unwrap(); // 512 used
                                            // Request 384 bytes freed: needs 3 × 128-byte entries.
        let stats = pool.evict_lru(384);
        assert_eq!(stats.evicted_bytes, 384);
        assert_eq!(stats.evicted_entries, 3);
        assert_eq!(pool.used_bytes(), 128);
    }

    #[test]
    fn glyph_atlas_pool_evict_lru_on_empty_pool_is_noop() {
        let mut pool = GlyphAtlasPool::with_cap(1024);
        let stats = pool.evict_lru(512);
        assert_eq!(stats.evicted_bytes, 0);
        assert_eq!(stats.evicted_entries, 0);
    }

    #[test]
    fn glyph_atlas_pool_evict_lru_frees_space_for_new_reserve() {
        let mut pool = GlyphAtlasPool::with_cap(1024);
        // Fill the pool with two 512-byte entries so eviction can hit an
        // exact target (a single 1024-byte entry would overshoot).
        let _ = pool.reserve(512).unwrap();
        let _ = pool.reserve(512).unwrap();
        assert_eq!(pool.used_bytes(), 1024);
        // Pool is full → reserve fails with GlyphAtlasEvict.
        assert_eq!(pool.reserve(1).unwrap_err(), BudgetBreach::GlyphAtlasEvict);
        // Evict 512 bytes (one entry); a 512-byte reserve should now succeed.
        let stats = pool.evict_lru(512);
        assert_eq!(stats.evicted_bytes, 512);
        assert_eq!(stats.evicted_entries, 1);
        let r = pool
            .reserve(512)
            .expect("reserve should succeed after eviction");
        assert_eq!(r.len, 512);
        assert_eq!(pool.used_bytes(), 1024);
    }

    #[test]
    fn glyph_atlas_pool_release_removes_entry_and_frees_space() {
        let mut pool = GlyphAtlasPool::with_cap(1024);
        let r0 = pool.reserve(256).unwrap();
        let _r1 = pool.reserve(256).unwrap();
        assert_eq!(pool.used_bytes(), 512);
        pool.release(r0);
        assert_eq!(pool.used_bytes(), 256);
        // The released entry is no longer tracked, so evict_lru should only
        // find the one remaining entry.
        let stats = pool.evict_lru(1024);
        assert_eq!(stats.evicted_bytes, 256);
        assert_eq!(stats.evicted_entries, 1);
        assert_eq!(pool.used_bytes(), 0);
    }

    // ---- PipelineCachePool: rejects at cap + LRU eviction ---------------

    #[test]
    fn pipeline_cache_pool_reserve_under_cap_succeeds() {
        let mut pool = PipelineCachePool::with_cap(1024);
        let region = pool.reserve(512).expect("reserve under cap should succeed");
        assert_eq!(region.kind, PoolKind::GPU);
        assert_eq!(pool.used_bytes(), 512);
    }

    #[test]
    fn pipeline_cache_pool_rejects_at_cap() {
        let mut pool = PipelineCachePool::with_cap(1024);
        let _ = pool.reserve(1024).unwrap();
        let err = pool.reserve(1).unwrap_err();
        assert_eq!(err, BudgetBreach::PipelineCacheFull);
        assert_eq!(pool.used_bytes(), 1024);
    }

    #[test]
    fn pipeline_cache_pool_evict_lru_removes_oldest_first() {
        let mut pool = PipelineCachePool::with_cap(1024);
        let _ = pool.reserve(512).unwrap(); // oldest
        let _ = pool.reserve(256).unwrap(); // newest
        assert_eq!(pool.used_bytes(), 768);

        let stats = pool.evict_lru(512);
        assert_eq!(stats.evicted_bytes, 512);
        assert_eq!(stats.evicted_entries, 1);
        assert_eq!(stats.kind, PoolKind::GPU);
        assert_eq!(pool.used_bytes(), 256);
    }

    #[test]
    fn pipeline_cache_pool_evict_lru_frees_space_for_new_reserve() {
        let mut pool = PipelineCachePool::with_cap(1024);
        // Fill the pool with two 512-byte entries so eviction can hit an
        // exact target (a single 1024-byte entry would overshoot).
        let _ = pool.reserve(512).unwrap();
        let _ = pool.reserve(512).unwrap();
        assert_eq!(pool.used_bytes(), 1024);
        assert_eq!(
            pool.reserve(1).unwrap_err(),
            BudgetBreach::PipelineCacheFull
        );
        let stats = pool.evict_lru(512);
        assert_eq!(stats.evicted_bytes, 512);
        assert_eq!(stats.evicted_entries, 1);
        let _ = pool
            .reserve(512)
            .expect("reserve should succeed after eviction");
    }

    // ---- AttachmentPool: rejects at cap (Reject → AttachmentPoolEmpty) --

    #[test]
    fn attachment_pool_reserve_under_cap_succeeds() {
        let mut pool = AttachmentPool::with_cap(1024);
        let region = pool.reserve(512).expect("reserve under cap should succeed");
        assert_eq!(region.kind, PoolKind::GPU);
        assert_eq!(pool.used_bytes(), 512);
    }

    #[test]
    fn attachment_pool_rejects_at_cap() {
        let mut pool = AttachmentPool::with_cap(1024);
        let _ = pool.reserve(1024).unwrap();
        let err = pool.reserve(1).unwrap_err();
        assert_eq!(err, BudgetBreach::AttachmentPoolEmpty);
        assert_eq!(pool.used_bytes(), 1024);
    }

    #[test]
    fn attachment_pool_evict_lru_is_noop() {
        let mut pool = AttachmentPool::with_cap(1024);
        let _ = pool.reserve(512).unwrap();
        let stats = pool.evict_lru(256);
        assert_eq!(stats.evicted_bytes, 0);
        assert_eq!(stats.evicted_entries, 0);
        assert_eq!(pool.used_bytes(), 512);
    }

    // ---- InstanceBufferPool: rejects at cap (Reject → InstanceBufferFull)

    #[test]
    fn instance_buffer_pool_reserve_under_cap_succeeds() {
        let mut pool = InstanceBufferPool::with_cap(1024);
        let region = pool.reserve(512).expect("reserve under cap should succeed");
        assert_eq!(region.kind, PoolKind::GPU);
        assert_eq!(pool.used_bytes(), 512);
    }

    #[test]
    fn instance_buffer_pool_rejects_at_cap() {
        let mut pool = InstanceBufferPool::with_cap(1024);
        let _ = pool.reserve(1024).unwrap();
        let err = pool.reserve(1).unwrap_err();
        assert_eq!(err, BudgetBreach::InstanceBufferFull);
        assert_eq!(pool.used_bytes(), 1024);
    }

    #[test]
    fn instance_buffer_pool_evict_lru_is_noop() {
        let mut pool = InstanceBufferPool::with_cap(1024);
        let _ = pool.reserve(512).unwrap();
        let stats = pool.evict_lru(256);
        assert_eq!(stats.evicted_bytes, 0);
        assert_eq!(stats.evicted_entries, 0);
        assert_eq!(pool.used_bytes(), 512);
    }
}
