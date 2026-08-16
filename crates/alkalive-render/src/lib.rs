//! AlkALive alkalive-render crate.
//!
//! Backend-abstracted render-graph IR, the render-graph compiler, the retained
//! render loop, and the compositor defined in `docs/SPECIFICATION.md`
//! §4.1–§4.7.
//!
//! Wave 5 (task IMPL-W5 / W5-T1, W5-T2, W5-T6) implements the render-graph
//! compiler ([`compile`]), the [`PipelineCache`] linear-search lookup, and the
//! [`PassBuilder`] draw-call recorder. The [`Backend`], [`RenderLoop`], and
//! [`Compositor`] traits remain **abstract**: their concrete implementations
//! require WebGPU/host bindings delivered in later waves — `WebGPUBackend` in
//! W5-T5, `Compositor` in W5-T4, and the `MockBackend`/`SoftwareBackend` test
//! seams in W11-T1/T2.
//!
//! Per `IMPLEMENTATION_PLAN.md` task W5-T2 / Wave 5 DoD, `#![forbid(unsafe_code)]`
//! is preserved; only safe Rust is used.
//!
//! See `docs/SPECIFICATION.md` §4.1–§4.7 for the authoritative signatures.
//!
//! # Wave 11 — practical Render-Graph IR (Gap 6)
//!
//! Wave 11 (Task ID 11 — Gap 6) adds the practical [`graph`] module: a
//! real, data-driven render-graph IR consumed by the GPU backend at frame
//! time. The [`graph::RenderGraph`] / [`graph::RenderPass`] /
//! [`graph::Attachment`] / [`graph::DrawCall`] / [`graph::DrawCallKind`]
//! types and the [`graph::build_render_graph`] function together replace
//! the previously hardcoded dispatch sequence in
//! `WgpuRenderer::render_frame_internal` with a data-driven loop over the
//! graph's passes. The Wave 5 compiler IR at the crate root
//! ([`RenderGraph`], [`RenderPass`], [`Attachment`], [`DrawCall`]) and
//! the [`compile`] function remain in place for the compiler tests; the
//! two IRs coexist, and the long-term plan (per the rendering spec §1.2)
//! is for them to converge.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub use alkalive_core::ModuleId;

use core::ops::Range;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};

/// Practical render-graph IR consumed by the GPU backend (Wave 11, Gap 6).
///
/// This module defines the data-driven [`graph::RenderGraph`] type and the
/// [`graph::build_render_graph`] constructor that the renderer iterates at
/// frame time. See the module-level docs for design rationale and the
/// relationship to the Wave 5 compiler IR at the crate root.
pub mod graph;

// ---------------------------------------------------------------------------
// Opaque identifiers and helper types referenced by the IR (§4.2)
// ---------------------------------------------------------------------------

/// Opaque identifier for a render-graph pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PassId {
    /// Opaque identifier value.
    pub value: u64,
}

/// Opaque identifier for a render-graph attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AttachmentId {
    /// Opaque identifier value.
    pub value: u64,
}

/// Opaque identifier for a draw call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DrawCallId {
    /// Opaque identifier value.
    pub value: u64,
}

/// Two-component vector (§5.2 geometry primitive shared with layout/input).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec2 {
    /// X component.
    pub x: f32,
    /// Y component.
    pub y: f32,
}

/// Dirty rectangle tagging a retained-frame invalidation region (ADR 002).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DirtyRect {
    /// Origin X.
    pub x: f32,
    /// Origin Y.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

/// Result of a single frame tick.
#[derive(Debug, Clone, Default)]
pub struct FrameResult;

/// Batched input samples consumed by the render loop (§8).
#[derive(Debug, Clone, Default)]
pub struct InputBatch;

/// Hit-test result (§8); computed in-WASM with no DOM crossing.
#[derive(Debug, Clone, Default)]
pub struct HitResult;

/// Opaque handle to a backend-allocated attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AttachmentHandle {
    /// Opaque handle value.
    pub value: u64,
}

// ===========================================================================
// §4.2 Render-graph IR
// ===========================================================================

/// Classification of a render-graph pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PassType {
    /// A rasterisation render pass.
    Render,
    /// A compute pass.
    Compute,
    /// A copy/transfer pass.
    CopyTransfer,
    /// The compositor-wide occlusion-cull pass.
    OcclusionCull,
}

/// Pixel/texel format of an attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttachmentFormat {
    /// 8-bit BGRA, unorm.
    Bgra8Unorm,
    /// 8-bit RGBA, sRGB.
    Rgba8UnormSrgb,
    /// 16-bit RGBA, float.
    Rgba16Float,
    /// 24+ depth.
    Depth24Plus,
    /// 32-bit float depth.
    Depth32Float,
    /// 8-bit stencil.
    Stencil8,
    /// BC1 compressed block.
    Bc1,
    /// BC2 compressed block.
    Bc2,
    /// BC3 compressed block.
    Bc3,
    /// BC4 compressed block.
    Bc4,
    /// BC5 compressed block.
    Bc5,
    /// BC6 compressed block.
    Bc6,
    /// BC7 compressed block.
    Bc7,
    /// ASTC 4×4 compressed block.
    Astc4x4,
    /// 32-bit unsigned integer.
    R32Uint,
}

/// Load/store operation at attachment lifetime boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClearOp {
    /// Clear the attachment to a default value.
    Clear,
    /// Load the previous contents.
    Load,
    /// Don't care about previous contents.
    DontCare,
}

/// MSAA sample count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SampleCount {
    /// 1× (no MSAA).
    X1,
    /// 2× MSAA.
    X2,
    /// 4× MSAA.
    X4,
    /// 8× MSAA.
    X8,
}

/// Absolute or surface-relative extent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExtentOrRelative {
    /// Absolute pixel dimensions, if specified.
    pub absolute: Option<(u32, u32)>,
    /// Relative `[0.0, 1.0]` dimensions, if specified.
    pub relative: Option<(f32, f32)>,
}

/// A render-graph attachment (§4.2).
#[derive(Debug, Clone)]
pub struct Attachment {
    /// Attachment identifier.
    pub id: AttachmentId,
    /// Pixel format.
    pub format: AttachmentFormat,
    /// Absolute or relative size.
    pub size: ExtentOrRelative,
    /// MSAA sample count.
    pub samples: SampleCount,
    /// `[producer, last_consumer]` lifetime range.
    pub lifetime: (PassId, PassId),
    /// Load/store op at the producer boundary.
    pub clear_op: ClearOp,
}

/// A render-graph pass (§4.2).
#[derive(Debug, Clone)]
pub struct RenderPass {
    /// Pass identifier.
    pub id: PassId,
    /// Pass kind.
    pub kind: PassType,
    /// Colour attachment identifiers.
    pub color_attachments: Box<[AttachmentId]>,
    /// Optional depth-stencil attachment.
    pub depth_stencil: Option<AttachmentId>,
    /// Draw-call identifiers recorded in this pass.
    pub draw_calls: Box<[DrawCallId]>,
    /// Barrier-edge dependencies; the compiler may reorder/batch respecting these.
    pub dependencies: Box<[PassId]>,
}

/// Vertex input binding.
#[derive(Debug, Clone, Default)]
pub struct VertexBinding;

/// Index input binding.
#[derive(Debug, Clone, Default)]
pub struct IndexBinding;

/// A bound resource group (owned-style uniforms per ADR 005, instance tables,
/// glyph runs).
#[derive(Debug, Clone, Default)]
pub struct BindGroup;

/// A single draw call (§4.2).
#[derive(Debug, Clone)]
pub struct DrawCall {
    /// Cached WGSL pipeline handle (§4.6).
    pub pipeline: PipelineHandle,
    /// Vertex input binding.
    pub vertices: VertexBinding,
    /// Optional index binding.
    pub indices: Option<IndexBinding>,
    /// Bound resource groups.
    pub bindings: Box<[BindGroup]>,
    /// GPU-resident instance range; cost decoupled from tree size.
    pub instances: Range<u32>,
    /// Optional scissor (ADR 002 scope tag).
    pub scissor: Option<DirtyRect>,
}

/// The compositor-wide occlusion-cull pass descriptor.
#[derive(Debug, Clone, Default)]
pub struct OcclusionCullPass;

/// An immutable render-graph IR submitted by a module or worker (§4.2).
///
/// The IR is immutable at submission: workers produce it; the render thread
/// consumes it.
#[derive(Debug, Clone)]
pub struct RenderGraph {
    /// Passes in this graph.
    pub passes: Box<[RenderPass]>,
    /// Attachments in this graph.
    pub attachments: Box<[Attachment]>,
    /// Draw calls in this graph.
    pub draw_calls: Box<[DrawCall]>,
    /// Occlusion-cull pass descriptor.
    pub occlusion_cull: OcclusionCullPass,
    /// Barrier edges `(from, to)`.
    pub edges: Box<[(PassId, PassId)]>,
    /// Owning module of this graph's source subtree.
    pub source_module: ModuleId,
}

// ===========================================================================
// §4.1 Backend abstraction
// ===========================================================================

/// Adapter power preference for [`Backend::request_adapter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PowerPref {
    /// When `true`, prefer the high-performance adapter; when `false`, prefer low-power.
    pub high_performance: bool,
}

/// Opaque handle to a physical adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AdapterHandle {
    /// Opaque handle value.
    pub value: u64,
}

/// Opaque handle to a logical device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceHandle {
    /// Opaque handle value.
    pub value: u64,
}

/// Opaque handle to a cached render pipeline (§4.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PipelineHandle {
    /// Opaque handle value.
    pub value: u64,
}

/// Description of a pipeline to be created (§4.6).
///
/// Cache lookups are keyed by `(shader_hash, layout_hash, target_format)`.
#[derive(Debug, Clone)]
pub struct PipelineDesc {
    /// Hash of the WGSL shader source.
    pub shader_hash: u64,
    /// Hash of the bind-group layout.
    pub layout_hash: u64,
    /// Render-target attachment format.
    pub target_format: AttachmentFormat,
    /// MSAA sample count.
    pub sample_count: SampleCount,
}

/// Description of an attachment to be allocated.
#[derive(Debug, Clone)]
pub struct AttachmentDesc {
    /// Pixel format.
    pub format: AttachmentFormat,
    /// Absolute or relative size.
    pub size: ExtentOrRelative,
    /// MSAA sample count.
    pub samples: SampleCount,
    /// Load/store op.
    pub clear_op: ClearOp,
}

/// An encoded command buffer ready for submission.
#[derive(Debug, Clone, Default)]
pub struct CommandBuffer;

/// Handle returned by [`Backend::submit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubmitHandle {
    /// Opaque handle value.
    pub value: u64,
}

/// The backend abstraction trait (§4.1).
///
/// `WebGPUBackend` is the only shipped implementation; `VulkanBackend` and
/// `MetalBackend` are future native options. Authors never call `Backend`
/// directly — the compositor mediates. The trait is also the primary
/// testability seam (§14): `MockBackend` and `SoftwareBackend` implement it
/// for headless CI.
///
/// # Implementation status
///
/// The trait itself is **abstract** in Wave 5: the method bodies are not
/// implemented here because they require WebGPU/host bindings. Concrete
/// implementations land in:
/// - **W5-T5** — `WebGPUBackend` over `wgpu` (or host binding); `encode` + `submit`.
/// - **W11-T1** — `MockBackend` recording draw calls for headless CI.
/// - **W11-T2** — `SoftwareBackend` deterministic rasteriser (ADR 016 fallback).
pub trait Backend {
    /// Request an adapter matching `pref`.
    fn request_adapter(&mut self, pref: PowerPref) -> Result<AdapterHandle, BackendError>;
    /// Create a logical device on `adapter`.
    fn create_device(&self, adapter: &AdapterHandle) -> Result<DeviceHandle, BackendError>;
    /// Create (or fetch a cached) render pipeline.
    fn create_pipeline(
        &self,
        dev: &DeviceHandle,
        desc: PipelineDesc,
    ) -> Result<PipelineHandle, PipelineError>;
    /// Allocate an attachment.
    fn create_attachment(
        &self,
        dev: &DeviceHandle,
        desc: AttachmentDesc,
    ) -> Result<AttachmentHandle, AllocationError>;
    /// Encode a compiled graph into a [`CommandBuffer`].
    fn encode(
        &self,
        dev: &DeviceHandle,
        compiled: &CompiledGraph,
    ) -> Result<CommandBuffer, EncodeError>;
    /// Submit encoded command buffers to the device queue.
    fn submit(
        &self,
        dev: &mut DeviceHandle,
        cmds: &[CommandBuffer],
    ) -> Result<SubmitHandle, SubmitError>;
}

// ===========================================================================
// §4.3 Render-graph compiler
// ===========================================================================

/// A merged, reordered, batched, barrier-inserted graph (§4.3).
///
/// Wave B populates `sorted_passes`, `pass_count`, and `draw_call_count` from
/// the merge + topological-sort result so downstream code no longer has to
/// discard the merged data. Draw-call batching by `(pipeline,
/// bind_group_topology)`, barrier insertion at lifetime boundaries, and the
/// occlusion-cull pass against `dirty`/`depth` remain W5-T2/W5-T3 follow-ups
/// that extend — not replace — these fields.
#[derive(Debug, Clone, Default)]
pub struct CompiledGraph {
    /// Merged and topologically-sorted pass IDs.
    pub sorted_passes: Vec<PassId>,
    /// Total number of merged passes.
    pub pass_count: usize,
    /// Total number of merged draw calls.
    pub draw_call_count: usize,
}

/// A compiled frame ready for occlusion culling (§3.6).
#[derive(Debug, Clone, Default)]
pub struct CompiledFrame;

/// A frame with occluded draw calls removed (§4.3).
#[derive(Debug, Clone, Default)]
pub struct CulledFrame;

/// The merger result of multiple scene-graph IRs (§3.6).
#[derive(Debug, Clone, Default)]
pub struct MergedGraph;

/// Compositor-wide depth buffer (§4.3 / §4.5).
#[derive(Debug, Clone, Default)]
pub struct DepthBuffer;

/// Compositor-wide visibility buffer (§4.3).
#[derive(Debug, Clone, Default)]
pub struct VisibilityBuffer;

/// Compile a slice of submitted graphs into a single [`CompiledGraph`] (§4.3).
///
/// The compiler **merges** graphs from all scene graphs (UI, particles, world,
/// overlays), **reorders** passes respecting barrier edges, **batches** draw
/// calls sharing pipeline+bind-group topology, inserts **barriers** at
/// attachment-lifetime boundaries, and runs the **occlusion-cull pass** against
/// the compositor-wide depth/visibility buffer to drop occluded draw calls
/// before encoding. Declaration order need not equal submission order (ADR 001).
///
/// # Wave 5 implementation scope
///
/// Wave 5 (W5-T2) implements the **merge**, **edge validation**, **barrier-cycle
/// detection** (topological sort via Kahn's algorithm), and **attachment-lifetime
/// validation**. Draw-call batching by `(pipeline, bind_group_topology)`, barrier
/// insertion at lifetime boundaries, and the occlusion-cull pass against
/// `dirty`/`depth` are W5-T2/W5-T3 follow-ups and do not alter the public
/// signature; `dirty` and `depth` are accepted here solely so the signature
/// matches §4.3.
pub fn compile(
    graphs: &[RenderGraph],
    dirty: &[DirtyRect],
    depth: &DepthBuffer,
) -> Result<CompiledGraph, CompileError> {
    // `dirty` and `depth` feed the occlusion-cull pass (W5-T3); consumed in a
    // follow-up without changing this signature.
    let _ = (dirty, depth);

    // --- Merge phase: collect passes, attachments, draw calls, and edges ---
    let mut merged_passes: Vec<RenderPass> = Vec::new();
    let mut merged_attachments: Vec<Attachment> = Vec::new();
    let mut merged_draw_calls: Vec<DrawCall> = Vec::new();
    let mut merged_edges: Vec<(PassId, PassId)> = Vec::new();

    for g in graphs {
        merged_passes.extend(g.passes.iter().cloned());
        merged_attachments.extend(g.attachments.iter().cloned());
        merged_draw_calls.extend(g.draw_calls.iter().cloned());
        merged_edges.extend(g.edges.iter().copied());
    }

    // Index every known pass id → position in `merged_passes`. This serves as
    // both the existence test (for edge/lifetime validation) and the
    // adjacency-list index (for the topological sort).
    let mut index_of: HashMap<PassId, usize> = HashMap::with_capacity(merged_passes.len());
    for (i, p) in merged_passes.iter().enumerate() {
        index_of.insert(p.id, i);
    }

    // --- Validate attachment lifetimes reference existing passes (§4.3) ---
    // Each attachment's `[producer, last_consumer]` range must name passes
    // present in the merged graph; otherwise the barrier schedule is undefined.
    for att in &merged_attachments {
        let (producer, last_consumer) = att.lifetime;
        if !index_of.contains_key(&producer) || !index_of.contains_key(&last_consumer) {
            return Err(CompileError::AttachmentLifetimeViolation);
        }
    }

    // --- Topological sort passes by barrier edges (Kahn's algorithm) ---
    // Build the in-degree vector and adjacency list while simultaneously
    // validating that every edge endpoint resolves to a known pass. An edge
    // referencing an unknown pass is a malformed IR → `InvalidEdge`.
    let mut in_degree: Vec<usize> = vec![0usize; merged_passes.len()];
    let mut adj: Vec<Vec<PassId>> = vec![Vec::new(); merged_passes.len()];

    for (from, to) in &merged_edges {
        match (index_of.get(from), index_of.get(to)) {
            (Some(&fi), Some(&ti)) => {
                adj[fi].push(*to);
                in_degree[ti] += 1;
            }
            _ => return Err(CompileError::InvalidEdge),
        }
    }

    // Seed the queue with all zero-in-degree passes (declaration order is
    // preserved among independent passes, satisfying ADR 001's
    // declaration-order-need-not-equal-submission-order guarantee).
    let mut queue: VecDeque<usize> = (0..in_degree.len())
        .filter(|&i| in_degree[i] == 0)
        .collect();

    // `sorted_passes` is the topologically-sorted output of Kahn's algorithm:
    // each pass is appended exactly once when it is dequeued. Declaration order
    // among independent passes is preserved because zero-in-degree seeds are
    // enqueued in index order and successors are appended in edge-encounter
    // order.
    let mut sorted_passes: Vec<PassId> = Vec::with_capacity(merged_passes.len());
    let mut visited: usize = 0;
    while let Some(i) = queue.pop_front() {
        visited += 1;
        sorted_passes.push(merged_passes[i].id);
        for to in &adj[i] {
            if let Some(&ti) = index_of.get(to) {
                in_degree[ti] -= 1;
                if in_degree[ti] == 0 {
                    queue.push_back(ti);
                }
            }
        }
    }

    // If not every pass was visited, a barrier cycle exists: the residual
    // subgraph has no zero-in-degree node, so Kahn's algorithm cannot drain it.
    if visited != merged_passes.len() {
        return Err(CompileError::BarrierCycle);
    }

    // The merged, topologically-validated graph is the compiler's output. Wave
    // B populates `CompiledGraph` with the topologically-sorted pass IDs, the
    // total merged pass count, and the total merged draw-call count so
    // downstream consumers (the batcher, the encoder) receive a stable handoff.
    // Draw-call batching by `(pipeline, bind_group_topology)`, barrier insertion
    // at lifetime boundaries, and the occlusion-cull pass (W5-T2/T3) extend
    // these fields in a follow-up without changing the public signature.
    // (`merged_attachments` and `merged_edges` are consumed above by the
    // attachment-lifetime and edge-validation passes.)
    Ok(CompiledGraph {
        sorted_passes,
        pass_count: merged_passes.len(),
        draw_call_count: merged_draw_calls.len(),
    })
}

// ===========================================================================
// §4.4 Retained render loop & dirty-rect invalidation
// ===========================================================================

/// Monotonic counter backing [`PassBuilder::finish`] draw-call identifiers.
///
/// A crate-local `AtomicU64` is used so that [`PassBuilder`] remains `Clone`/
/// `Default` (an `AtomicU64` field would forbid `Clone`) while still issuing
/// strictly-increasing [`DrawCallId`]s across every pass built in the process.
static NEXT_DRAW_CALL_ID: AtomicU64 = AtomicU64::new(0);

/// Builder returned by [`RenderLoop::begin_pass`].
///
/// Wave 5 (W5-T2) implements the recorder surface: callers bind a cached
/// pipeline, record one or more instance ranges via [`PassBuilder::draw`], and
/// call [`PassBuilder::finish`] to receive a fresh [`DrawCallId`] drawn from a
/// monotonic counter. The recorded state is consumed by the future
/// render-graph encoder (W5-T5).
#[derive(Debug, Clone, Default)]
pub struct PassBuilder {
    /// Cached pipeline bound via [`PassBuilder::bind_pipeline`].
    pipeline: Option<PipelineHandle>,
    /// Instance ranges recorded via [`PassBuilder::draw`], in recording order.
    draws: Vec<Range<u32>>,
}

impl PassBuilder {
    /// Bind a cached pipeline to the pass under construction.
    pub fn bind_pipeline(&mut self, handle: PipelineHandle) -> &mut Self {
        self.pipeline = Some(handle);
        self
    }

    /// Record a draw over the given instance range.
    pub fn draw(&mut self, instances: Range<u32>) -> &mut Self {
        self.draws.push(instances);
        self
    }

    /// Finish the pass and return its assigned draw-call identifier.
    ///
    /// The identifier is drawn from a process-wide monotonic counter; each
    /// call to `finish` yields a strictly-greater [`DrawCallId`].
    pub fn finish(self) -> DrawCallId {
        let value = NEXT_DRAW_CALL_ID.fetch_add(1, Ordering::Relaxed);
        DrawCallId { value }
    }
}

/// The retained-mode render loop trait (§4.4).
///
/// Scene-graph state persists across frames and only dirty rectangles or
/// per-object subsets are re-emitted (ADR 002).
///
/// # Implementation status
///
/// The trait is **abstract** in Wave 5: the concrete `RenderLoop` requires the
/// compositor host binding and the retained-frame store. It lands in **W5-T4**
/// alongside the [`Compositor`] impl and is exercised headlessly by the
/// `TestHarness` in **W11**.
pub trait RenderLoop {
    /// Advance one vsync tick.
    fn tick(&mut self, dt: f32, dirty: &[DirtyRect], input: &InputBatch) -> FrameResult;
    /// Mark `scope` as requiring layout; locality enforced by the solver.
    fn request_layout(&self, scope: ModuleId);
    /// Enqueue `graph` for merge/compile/reorder/submit.
    fn submit(&self, graph: RenderGraph) -> SubmitHandle;
    /// Hit-test a point in-WASM; no DOM crossing.
    fn hit_test(&self, point: Vec2) -> HitResult;
    /// Begin building a pass writing to `att`.
    fn begin_pass(&mut self, att: &Attachment) -> PassBuilder;
}

// ===========================================================================
// §4.5 Compositor & draw-call submission
// ===========================================================================

/// Cache of compiled pipelines keyed by `(shader_hash, layout_hash,
/// target_format)` (§4.6).
///
/// Wave 5 (W5-T6) implements a linear-search cache bounded by a 64 MB LRU
/// cap (§12.7): when an [`insert`](Self::insert) would push `total_bytes`
/// past [`MAX_CACHE_BYTES`](Self::MAX_CACHE_BYTES), the oldest entries
/// (front of the FIFO queue) are evicted until there is room. The
/// degraded-builtin fallback path lands in a follow-up wave.
///
/// The entries live in a [`VecDeque`] rather than a `Vec` so that
/// `pop_front()` eviction is O(1) — the cap holds ~1–2 M entries at the
/// simplified ~64-byte-per-entry estimate, which makes `Vec::remove(0)`
/// (O(n) per eviction) prohibitive. Linear-search lookup semantics are
/// preserved; a hashed index bounded by the 64 MB cap may land in a
/// later wave.
#[derive(Debug, Clone, Default)]
pub struct PipelineCache {
    /// Linear-search entries in FIFO insertion order; the oldest entry is
    /// at the front of the deque. A `HashMap` is intentionally avoided at
    /// this stage so that `PipelineDesc`'s derive set stays minimal.
    entries: VecDeque<(PipelineDesc, PipelineHandle)>,
    /// Running total of the byte cost of all stored entries. Each entry
    /// contributes [`entry_size`](Self::entry_size) bytes (~64 bytes per
    /// the simplified `size_of` estimate).
    total_bytes: usize,
}

impl PipelineCache {
    /// Maximum cache byte budget (§12.7): 64 MB.
    pub const MAX_CACHE_BYTES: usize = 64 * 1024 * 1024;

    /// Per-entry byte cost: `size_of::<PipelineDesc>() + size_of::<PipelineHandle>()`
    /// (simplified — ~64 bytes per entry). The estimate is recomputed from
    /// the live struct layout so the budget stays accurate even if either
    /// type grows.
    const fn entry_size() -> usize {
        std::mem::size_of::<PipelineDesc>() + std::mem::size_of::<PipelineHandle>()
    }

    /// Look up a cached pipeline by its key triple.
    ///
    /// The lookup matches `(shader_hash, layout_hash, target_format)` via
    /// linear search; the first matching entry wins. `sample_count` is **not**
    /// part of the lookup key (matching the §4.6 contract), but is stored on
    /// the descriptor for the future `Backend::create_pipeline` call.
    pub fn get(
        &self,
        shader_hash: u64,
        layout_hash: u64,
        format: AttachmentFormat,
    ) -> Option<PipelineHandle> {
        for (desc, handle) in &self.entries {
            if desc.shader_hash == shader_hash
                && desc.layout_hash == layout_hash
                && desc.target_format == format
            {
                return Some(*handle);
            }
        }
        None
    }

    /// Insert a compiled pipeline into the cache.
    ///
    /// If the new entry would push `total_bytes` past
    /// [`MAX_CACHE_BYTES`](Self::MAX_CACHE_BYTES), the oldest entries
    /// (front of the FIFO deque) are evicted until there is room. If a
    /// single entry is larger than the cap, the cache is cleared and the
    /// entry is still inserted so lookups always reflect the latest
    /// compile.
    pub fn insert(&mut self, desc: &PipelineDesc, handle: PipelineHandle) {
        let entry_size = Self::entry_size();

        // Evict oldest entries (front of the FIFO deque) until the new
        // entry fits within MAX_CACHE_BYTES. If a single entry would not
        // fit even in an empty cache, drop everything and still insert
        // the new entry — the latest compile must always be findable.
        while !self.entries.is_empty() && self.total_bytes + entry_size > Self::MAX_CACHE_BYTES {
            self.entries.pop_front();
            self.total_bytes = self.total_bytes.saturating_sub(entry_size);
        }

        self.entries.push_back((desc.clone(), handle));
        self.total_bytes += entry_size;
    }

    /// Number of entries currently cached.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache holds zero entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Total byte cost of all stored entries. Each entry contributes
    /// [`entry_size`](Self::entry_size) bytes; the value is always
    /// `≤ MAX_CACHE_BYTES` after [`insert`](Self::insert) returns.
    pub fn byte_size(&self) -> usize {
        self.total_bytes
    }
}

/// The compositor trait (§3.6 / §4.5).
///
/// The main thread owns the lone `GPUDevice` (ADR 003 / ADR 021); on-demand
/// WASM workers never acquire it. The compositor merges, compiles, reorders,
/// batches, then submits.
///
/// # Implementation status
///
/// The trait is **abstract** in Wave 5: the concrete `Compositor` —
/// `enqueue` (SAB/socket feed stubbed), `commit`, and `depth_buffer` — lands
/// in **W5-T4**, atop the [`compile`] compiler implemented here.
pub trait Compositor {
    /// Enqueue a graph fed over SAB/socket from a worker.
    fn enqueue(&self, graph: RenderGraph) -> SubmitHandle;
    /// Merge, compile, cull, and submit all queued graphs.
    fn commit(&mut self, dirty: &[DirtyRect]) -> Result<FrameResult, CompositeError>;
    /// Borrow the compositor-wide depth buffer (occlusion source).
    fn depth_buffer(&self) -> &DepthBuffer;
}

// ===========================================================================
// §4.7 Pipeline stages & errors
// ===========================================================================

/// Backend-level failure.
#[derive(Debug, Clone)]
pub enum BackendError {
    /// No suitable adapter was available.
    AdapterUnavailable,
    /// The device was lost.
    DeviceLost,
    /// A requested feature or format is unsupported.
    Unsupported,
}

/// Pipeline creation or lookup failure (§4.6).
#[derive(Debug, Clone)]
pub enum PipelineError {
    /// WGSL compilation failed.
    WgslCompileFailure,
    /// Bind-group layout mismatch.
    LayoutMismatch,
    /// Cache miss with no degraded fallback available.
    CacheMiss,
}

/// Attachment or memory allocation failure.
#[derive(Debug, Clone)]
pub enum AllocationError {
    /// Attachment allocation exhausted.
    AttachmentExhausted,
    /// Pool exhausted.
    PoolExhausted,
    /// Out of memory.
    Oom,
}

/// Command encoding failure (device-lost, validation).
#[derive(Debug, Clone)]
pub enum EncodeError {
    /// The device was lost during encoding.
    DeviceLost,
    /// The backend rejected a command as invalid.
    ValidationFailure,
}

/// Queue submission failure.
#[derive(Debug, Clone)]
pub enum SubmitError {
    /// The queue was lost.
    QueueLost,
    /// A submitted handle was invalid.
    InvalidHandle,
}

/// Compositor-level failure (§4.5).
#[derive(Debug, Clone)]
pub enum CompositeError {
    /// Depth-buffer contention.
    DepthBufferContention,
    /// IR schema mismatch across merged graphs.
    IrSchemaMismatch,
}

/// Render-graph compiler failure (§4.3).
///
/// `PartialEq`/`Eq` are derived so tests can assert on the exact variant
/// returned by [`compile`]; all variants are unitary, so the derive is trivial.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    /// A barrier cycle was detected.
    BarrierCycle,
    /// An attachment-lifetime violation was detected.
    AttachmentLifetimeViolation,
    /// An invalid barrier edge was referenced.
    InvalidEdge,
}

/// Cross-module constraint escaped the dirty set (ADR 002).
#[derive(Debug, Clone)]
pub enum LocalityViolation {
    /// A constraint referenced a foreign module.
    CrossModuleConstraint {
        /// Source module of the constraint.
        source: ModuleId,
        /// Target module reached by the constraint.
        target: ModuleId,
    },
    /// A dependency fell outside the requesting module's scope.
    UnscopedDependency {
        /// The module owning the out-of-scope dependency.
        module: ModuleId,
    },
}

/// Top-level render error (§4.7).
///
/// On error the compositor retains the last-known-good frame and surfaces a
/// structured diagnostic to the unified trace (ADR 016).
#[derive(Debug, Clone)]
pub enum RenderError {
    /// Backend-level failure.
    Backend(BackendError),
    /// Render-graph compiler failure (barrier-cycle, attachment-lifetime violation).
    Compile(CompileError),
    /// Pipeline creation/lookup failure (WGSL compile or layout mismatch).
    Pipeline(PipelineError),
    /// Attachment/pool exhaustion.
    Allocation(AllocationError),
    /// Command encoding failure (device-lost, validation).
    Encode(EncodeError),
    /// Queue submission failure.
    Submit(SubmitError),
    /// Compositor failure (depth-buffer contention, IR schema mismatch).
    Composite(CompositeError),
    /// Locality violation — cross-module constraint escaped (ADR 002).
    Locality(LocalityViolation),
}

// ===========================================================================
// §6 text-stack glue (Wave 3 / task WAVE-W3)
// ===========================================================================
//
// Wave 3 wires the render crate to `alkalive-text`: the render side now
// accepts a fully-shaped `alkalive_text::ShapedRun` and converts it into
// placeholder draw calls. The `GlyphQuadBatch` type is re-exported from
// `alkalive-text` so downstream consumers see a single canonical batch
// carrier across the §4↔§6 boundary (the rendering-ABI ADR §4.7 will
// eventually unify the rest of the text-stack types the same way).

/// Batched glyph quads emitted by the text stack (§6.5).
///
/// Re-exported from `alkalive-text` so the render crate and the text crate
/// share a single canonical batch carrier. The compositor (ADR 003) batches
/// glyph quads across modules into a single instanced draw; that batching
/// logic lands in a later wave.
pub use alkalive_text::GlyphQuadBatch;

use alkalive_text::{GlyphAtlas, GlyphKey, ShapedRun};

/// Convert a shaped text run into placeholder draw calls (Wave 3 glue).
///
/// Each glyph in `shaped.glyph_ids` becomes one placeholder [`DrawCall`]:
/// a zero-handle pipeline, an empty bind-group set, and a single-instance
/// range `0..1`. The `atlas` is consulted via [`GlyphAtlas::slot`] (the
/// only `&self` method on the trait) so the residency check is exercised
/// and the parameter is meaningfully consumed; the result is not yet wired
/// into the placeholder [`DrawCall`] — real GPU submission (instanced
/// batching, atlas UV wiring, scissor tagging, pipeline selection) lands in
/// a later wave.
///
/// This is a glue function — keep it minimal. The signature accepts a
/// `&dyn GlyphAtlas` so callers can pass any atlas implementation without
/// needing a mutable borrow (the Wave 3 placeholder only reads).
///
/// # Panics
///
/// Never. A shape run with zero glyphs yields an empty `Vec<DrawCall>`.
pub fn glyph_run_to_draw_calls(shaped: &ShapedRun, atlas: &dyn GlyphAtlas) -> Vec<DrawCall> {
    shaped
        .glyph_ids
        .iter()
        .map(|&glyph_id| {
            // Build a read-only atlas lookup key. `size_px` is not carried
            // on `ShapedRun` (it lives on `ShapeContext`, which the shaper
            // consumes internally); the Wave 3 placeholder uses `0` since
            // the residency result is not yet wired into the DrawCall.
            let key = GlyphKey {
                font_id: shaped.font_id,
                glyph_id,
                phase: 0,
                size_px: 0,
            };
            // Touch the atlas so the parameter is used and the residency
            // check is exercised; the result is intentionally discarded —
            // the placeholder DrawCall carries no UV data yet.
            let _resident = atlas.slot(key).is_some();
            DrawCall {
                pipeline: PipelineHandle { value: 0 },
                vertices: VertexBinding,
                indices: None,
                bindings: Box::new([]),
                instances: 0..1,
                scissor: None,
            }
        })
        .collect()
}

// ===========================================================================
// Wave 5 tests (W5-T8)
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- Small constructors keep the fixtures below readable. ---
    fn pid(v: u64) -> PassId {
        PassId { value: v }
    }
    fn aid(v: u64) -> AttachmentId {
        AttachmentId { value: v }
    }
    fn ph(v: u64) -> PipelineHandle {
        PipelineHandle { value: v }
    }
    fn mid(v: u64) -> ModuleId {
        ModuleId(v)
    }

    /// Minimal render pass with no attachments and no draw calls.
    fn bare_pass(id: u64) -> RenderPass {
        RenderPass {
            id: pid(id),
            kind: PassType::Render,
            color_attachments: Box::new([]),
            depth_stencil: None,
            draw_calls: Box::new([]),
            dependencies: Box::new([]),
        }
    }

    /// A trivial colour attachment sized 64×64 whose lifetime is `(producer, producer)`.
    fn colour_attachment(id: u64, producer: PassId, last_consumer: PassId) -> Attachment {
        Attachment {
            id: aid(id),
            format: AttachmentFormat::Bgra8Unorm,
            size: ExtentOrRelative {
                absolute: Some((64, 64)),
                relative: None,
            },
            samples: SampleCount::X1,
            lifetime: (producer, last_consumer),
            clear_op: ClearOp::Clear,
        }
    }

    fn graph(
        passes: Vec<RenderPass>,
        attachments: Vec<Attachment>,
        edges: Vec<(PassId, PassId)>,
        source: ModuleId,
    ) -> RenderGraph {
        RenderGraph {
            passes: passes.into_boxed_slice(),
            attachments: attachments.into_boxed_slice(),
            draw_calls: Box::new([]),
            occlusion_cull: OcclusionCullPass,
            edges: edges.into_boxed_slice(),
            source_module: source,
        }
    }

    // --- compile(): single graph, no edges → Ok --------------------------------

    #[test]
    fn compile_single_graph_no_edges_ok() {
        let pass = bare_pass(1);
        let att = colour_attachment(10, pid(1), pid(1));
        let g = graph(vec![pass], vec![att], vec![], mid(0));

        let result = compile(&[g], &[], &DepthBuffer);
        assert!(result.is_ok(), "single graph with no edges must compile");
    }

    // --- compile(): empty input → Ok -------------------------------------------

    #[test]
    fn compile_empty_input_ok() {
        let result = compile(&[], &[], &DepthBuffer);
        assert!(result.is_ok(), "empty graph slice must compile to Ok");
        // An empty input must yield a zero-populated CompiledGraph: no sorted
        // passes, zero pass count, zero draw calls.
        let cg = result.unwrap();
        assert!(
            cg.sorted_passes.is_empty(),
            "empty input must yield no sorted passes"
        );
        assert_eq!(cg.pass_count, 0, "empty input must yield pass_count == 0");
        assert_eq!(
            cg.draw_call_count, 0,
            "empty input must yield draw_call_count == 0"
        );
    }

    // --- compile(): two graphs merged → Ok -------------------------------------

    #[test]
    fn compile_two_graphs_merged_ok() {
        // g1 declares pass 1 in isolation; g2 declares pass 2 with a cross-graph
        // barrier 1 → 2. The merged compiler must resolve the cross-graph edge.
        let g1 = graph(vec![bare_pass(1)], vec![], vec![], mid(0));
        let g2 = graph(vec![bare_pass(2)], vec![], vec![(pid(1), pid(2))], mid(1));

        let result = compile(&[g1, g2], &[], &DepthBuffer);
        assert!(
            result.is_ok(),
            "two merged graphs with a cross-graph edge must compile"
        );
    }

    // --- compile(): non-empty input populates CompiledGraph (Wave B) -----------

    #[test]
    fn compile_populates_compiled_graph_for_non_empty_input() {
        // Two passes with a barrier 1 → 2 and one draw call carried by the
        // first graph. The merged `CompiledGraph` must surface:
        //   * a non-zero `pass_count` equal to the merged pass count,
        //   * a non-zero `draw_call_count` equal to the merged draw-call count,
        //   * a `sorted_passes` of the same length as `pass_count`, in an order
        //     that respects the barrier edge (pass 1 before pass 2).
        let dc = DrawCall {
            pipeline: ph(5),
            vertices: VertexBinding,
            indices: None,
            bindings: Box::new([]),
            instances: 0..1,
            scissor: None,
        };
        let g1 = RenderGraph {
            passes: Box::new([bare_pass(1), bare_pass(2)]),
            attachments: Box::new([]),
            draw_calls: Box::new([dc]),
            occlusion_cull: OcclusionCullPass,
            edges: Box::new([(pid(1), pid(2))]),
            source_module: mid(0),
        };

        let result = compile(&[g1], &[], &DepthBuffer);
        assert!(
            result.is_ok(),
            "non-empty graph with a valid edge must compile"
        );
        let cg = result.unwrap();

        assert_eq!(
            cg.pass_count, 2,
            "pass_count must equal the merged pass count"
        );
        assert_eq!(
            cg.draw_call_count, 1,
            "draw_call_count must equal the merged draw-call count"
        );
        assert_eq!(
            cg.sorted_passes.len(),
            cg.pass_count,
            "sorted_passes length must equal pass_count"
        );

        // Topological order must respect the 1 → 2 barrier edge: pass 1 must
        // appear before pass 2 in `sorted_passes`.
        let pos1 = cg
            .sorted_passes
            .iter()
            .position(|p| *p == pid(1))
            .expect("pass 1 must appear in sorted_passes");
        let pos2 = cg
            .sorted_passes
            .iter()
            .position(|p| *p == pid(2))
            .expect("pass 2 must appear in sorted_passes");
        assert!(
            pos1 < pos2,
            "barrier edge 1 -> 2 must be respected: got sorted_passes = {:?}",
            cg.sorted_passes
        );
    }

    // --- compile(): barrier cycle → Err(BarrierCycle) --------------------------

    #[test]
    fn compile_barrier_cycle_err() {
        // Cycle: 1 → 2 → 1. Neither pass has zero in-degree, so Kahn's algorithm
        // cannot make progress and `visited` falls short of the pass count.
        let g = graph(
            vec![bare_pass(1), bare_pass(2)],
            vec![],
            vec![(pid(1), pid(2)), (pid(2), pid(1))],
            mid(0),
        );

        let result = compile(&[g], &[], &DepthBuffer);
        assert!(
            matches!(result, Err(CompileError::BarrierCycle)),
            "barrier cycle must surface as BarrierCycle, got {result:?}"
        );
    }

    // --- compile(): self-loop cycle → Err(BarrierCycle) ------------------------

    #[test]
    fn compile_self_loop_cycle_err() {
        // A self-edge 1 → 1 is the minimal cycle: in-degree of pass 1 is 1, so it
        // never enters the queue.
        let g = graph(vec![bare_pass(1)], vec![], vec![(pid(1), pid(1))], mid(0));

        let result = compile(&[g], &[], &DepthBuffer);
        assert!(
            matches!(result, Err(CompileError::BarrierCycle)),
            "self-loop must surface as BarrierCycle, got {result:?}"
        );
    }

    // --- compile(): invalid attachment lifetime → Err(AttachmentLifetimeViolation)

    #[test]
    fn compile_invalid_attachment_lifetime_err() {
        let pass = bare_pass(1);
        // Lifetime references pass 999, which is not in the merged pass set.
        let att = colour_attachment(10, pid(1), pid(999));
        let g = graph(vec![pass], vec![att], vec![], mid(0));

        let result = compile(&[g], &[], &DepthBuffer);
        assert!(
            matches!(result, Err(CompileError::AttachmentLifetimeViolation)),
            "missing lifetime endpoint must surface as AttachmentLifetimeViolation, got {result:?}"
        );
    }

    // --- compile(): invalid edge → Err(InvalidEdge) ----------------------------

    #[test]
    fn compile_invalid_edge_err() {
        // Edge 1 → 999 references a non-existent pass.
        let g = graph(vec![bare_pass(1)], vec![], vec![(pid(1), pid(999))], mid(0));

        let result = compile(&[g], &[], &DepthBuffer);
        assert!(
            matches!(result, Err(CompileError::InvalidEdge)),
            "edge to a non-existent pass must surface as InvalidEdge, got {result:?}"
        );
    }

    // --- PipelineCache: miss → None; insert then get → Some --------------------

    #[test]
    fn pipeline_cache_miss_then_hit() {
        let mut cache = PipelineCache::default();
        let desc = PipelineDesc {
            shader_hash: 0xABCD_1234,
            layout_hash: 0x5678_9ABC,
            target_format: AttachmentFormat::Bgra8Unorm,
            sample_count: SampleCount::X1,
        };

        // Miss on the empty cache.
        assert_eq!(
            cache.get(0xABCD_1234, 0x5678_9ABC, AttachmentFormat::Bgra8Unorm),
            None,
            "empty cache must miss"
        );

        // Insert and hit.
        cache.insert(&desc, ph(7));
        assert_eq!(
            cache.get(0xABCD_1234, 0x5678_9ABC, AttachmentFormat::Bgra8Unorm),
            Some(ph(7)),
            "inserted entry must be found by its key triple"
        );

        // A differing component of the key triple must still miss.
        assert_eq!(
            cache.get(0xABCD_1234, 0x5678_9ABC, AttachmentFormat::Rgba8UnormSrgb),
            None,
            "different target_format must miss"
        );
        assert_eq!(
            cache.get(0xFFFF_FFFF, 0x5678_9ABC, AttachmentFormat::Bgra8Unorm),
            None,
            "different shader_hash must miss"
        );
        assert_eq!(
            cache.get(0xABCD_1234, 0x0000_0000, AttachmentFormat::Bgra8Unorm),
            None,
            "different layout_hash must miss"
        );
    }

    // --- PipelineCache: len / byte_size accessors track inserts --------------

    #[test]
    fn pipeline_cache_len_and_byte_size_track_inserts() {
        let mut cache = PipelineCache::default();
        let entry_size =
            std::mem::size_of::<PipelineDesc>() + std::mem::size_of::<PipelineHandle>();

        assert!(cache.is_empty(), "fresh cache must be empty");
        assert_eq!(cache.len(), 0, "fresh cache len must be 0");
        assert_eq!(cache.byte_size(), 0, "fresh cache byte_size must be 0");

        let desc = PipelineDesc {
            shader_hash: 1,
            layout_hash: 1,
            target_format: AttachmentFormat::Bgra8Unorm,
            sample_count: SampleCount::X1,
        };
        cache.insert(&desc, ph(1));
        assert!(!cache.is_empty(), "cache with one entry must not be empty");
        assert_eq!(cache.len(), 1, "len must be 1 after one insert");
        assert_eq!(
            cache.byte_size(),
            entry_size,
            "byte_size must equal one entry_size after one insert",
        );

        cache.insert(&desc, ph(2));
        assert_eq!(cache.len(), 2, "len must be 2 after two inserts");
        assert_eq!(
            cache.byte_size(),
            entry_size * 2,
            "byte_size must equal 2 * entry_size after two inserts",
        );
    }

    // --- PipelineCache: 64 MB LRU eviction (Gap #5) --------------------------
    //
    // Inserts enough entries to exceed MAX_CACHE_BYTES and verifies that:
    //   * eviction trimmed the cache below the inserted count,
    //   * byte_size stays at or under the cap,
    //   * the most-recently-inserted entry survives (LRU evicts oldest first),
    //   * byte_size stays an exact multiple of entry_size.
    //
    // The per-entry cost is computed from the live struct layout via
    // `size_of::<PipelineDesc>() + size_of::<PipelineHandle>()` (the
    // simplified ~64-byte estimate from the gap analysis yields ≥ 1,000,001
    // entries; the actual layout may be smaller, so the count is derived
    // from the real per-entry cost rather than hard-coded).

    #[test]
    fn pipeline_cache_lru_eviction_keeps_byte_size_under_cap() {
        let mut cache = PipelineCache::default();
        let entry_size =
            std::mem::size_of::<PipelineDesc>() + std::mem::size_of::<PipelineHandle>();

        // Insert enough entries to exceed MAX_CACHE_BYTES. With the
        // simplified ~64-byte estimate this is ≥ 1,000,001 entries; the
        // actual size_of-derived count may be larger.
        let n = PipelineCache::MAX_CACHE_BYTES / entry_size + 1;
        assert!(
            n.checked_mul(entry_size)
                .map(|bytes| bytes > PipelineCache::MAX_CACHE_BYTES)
                .unwrap_or(true),
            "n={} * entry_size={} must overflow MAX_CACHE_BYTES={}",
            n,
            entry_size,
            PipelineCache::MAX_CACHE_BYTES,
        );

        for i in 0..n as u64 {
            let desc = PipelineDesc {
                shader_hash: i,
                layout_hash: i,
                target_format: AttachmentFormat::Bgra8Unorm,
                sample_count: SampleCount::X1,
            };
            cache.insert(&desc, ph(i));
        }

        // Eviction must have trimmed the cache below `n`.
        assert!(
            cache.len() < n,
            "eviction must have trimmed the cache; len={}, n={}",
            cache.len(),
            n,
        );
        assert!(
            cache.byte_size() <= PipelineCache::MAX_CACHE_BYTES,
            "byte_size {} must not exceed MAX_CACHE_BYTES {}",
            cache.byte_size(),
            PipelineCache::MAX_CACHE_BYTES,
        );
        // Each entry contributes exactly `entry_size` bytes.
        assert_eq!(
            cache.byte_size(),
            cache.len() * entry_size,
            "byte_size must equal len * entry_size",
        );

        // LRU: oldest entries (smallest shader_hash) were evicted first;
        // the most-recently-inserted entry must still be findable.
        let last: u64 = (n - 1) as u64;
        assert_eq!(
            cache.get(last, last, AttachmentFormat::Bgra8Unorm),
            Some(ph(last)),
            "most-recently-inserted entry must survive eviction",
        );
        // The very first inserted entry must have been evicted.
        assert_eq!(
            cache.get(0, 0, AttachmentFormat::Bgra8Unorm),
            None,
            "oldest entry must have been evicted",
        );
    }

    // --- PassBuilder: finish returns incrementing DrawCallIds ------------------

    #[test]
    fn pass_builder_finish_increments() {
        // The counter is process-wide; assert strict increment regardless of the
        // absolute starting value (other tests may have bumped it first).
        let a = PassBuilder::default().finish();
        let b = PassBuilder::default().finish();
        let c = PassBuilder::default().finish();
        assert_eq!(
            b.value,
            a.value + 1,
            "second finish must be exactly one past the first"
        );
        assert_eq!(
            c.value,
            b.value + 1,
            "third finish must be exactly one past the second"
        );
    }

    // --- PassBuilder: records pipeline + draws ---------------------------------

    #[test]
    fn pass_builder_records_pipeline_and_draws() {
        let mut builder = PassBuilder::default();
        builder.bind_pipeline(ph(42)).draw(0..10).draw(10..20);

        // Private fields are visible to this child test module.
        assert_eq!(
            builder.pipeline,
            Some(ph(42)),
            "bound pipeline must be stored"
        );
        assert_eq!(builder.draws.len(), 2, "both draw ranges must be recorded");
        assert_eq!(
            builder.draws[0],
            0..10,
            "first draw range preserved in order"
        );
        assert_eq!(
            builder.draws[1],
            10..20,
            "second draw range preserved in order"
        );

        // finish consumes the builder and still yields a valid id.
        let id = builder.finish();
        // id.value only needs to be a valid u64; increment behaviour is covered above.
        let _ = id;
    }

    // --- glyph_run_to_draw_calls (Wave 3 / task WAVE-W3) -----------------------

    /// Build a minimal 3-glyph `ShapedRun` for the placeholder draw-call
    /// test. The render crate does not depend on HarfRust directly (it
    /// consumes the already-shaped `ShapedRun` from `alkalive-text`), so
    /// the fixture is constructed by hand from public `alkalive_text`
    /// types.
    fn shaped_run_3_glyphs() -> ShapedRun {
        use alkalive_text::{ClusterMap, Direction, FontId, RunMetrics};
        ShapedRun {
            glyph_ids: Box::new([1, 2, 3]),
            advances: Box::new([10.0, 20.0, 30.0]),
            offsets: Box::new([(0.0, 0.0), (0.0, 0.0), (0.0, 0.0)]),
            clusters: Box::new([0, 1, 2]),
            caret_map: ClusterMap::default(),
            metrics: RunMetrics {
                total_advance: 60.0,
                ..RunMetrics::default()
            },
            bidi_level: 0,
            font_id: FontId(0),
            direction: Direction::default(),
        }
    }

    /// `glyph_run_to_draw_calls` emits exactly one placeholder [`DrawCall`]
    /// per glyph in the shaped run, regardless of atlas residency (Wave 3).
    #[test]
    fn glyph_run_to_draw_calls_one_per_glyph() {
        let shaped = shaped_run_3_glyphs();
        // `MockGlyphAtlas::slot` always returns `None` — the placeholder
        // path must still emit a DrawCall per glyph.
        let atlas = alkalive_text::MockGlyphAtlas;
        let calls = glyph_run_to_draw_calls(&shaped, &atlas);

        assert_eq!(
            calls.len(),
            3,
            "expected one placeholder DrawCall per glyph, got {} calls",
            calls.len(),
        );
        // Every placeholder DrawCall carries the zero-handle pipeline, no
        // indices, an empty bind-group set, and a single-instance range —
        // the real GPU submission will replace these in a later wave.
        for (i, dc) in calls.iter().enumerate() {
            assert_eq!(
                dc.pipeline,
                PipelineHandle { value: 0 },
                "placeholder draw call {i} must use the zero-handle pipeline",
            );
            assert!(
                dc.indices.is_none(),
                "placeholder draw call {i} has no indices"
            );
            assert!(
                dc.bindings.is_empty(),
                "placeholder draw call {i} has no bind groups",
            );
            assert_eq!(
                dc.instances,
                0..1,
                "placeholder draw call {i} spans one instance"
            );
            assert!(
                dc.scissor.is_none(),
                "placeholder draw call {i} has no scissor"
            );
        }
    }

    /// `glyph_run_to_draw_calls` over an empty shaped run yields an empty
    /// `Vec<DrawCall>` — no panics, no placeholder entries (Wave 3).
    #[test]
    fn glyph_run_to_draw_calls_empty_run_yields_no_calls() {
        use alkalive_text::{ClusterMap, Direction, FontId, RunMetrics};
        let shaped = ShapedRun {
            glyph_ids: Box::new([]),
            advances: Box::new([]),
            offsets: Box::new([]),
            clusters: Box::new([]),
            caret_map: ClusterMap::default(),
            metrics: RunMetrics::default(),
            bidi_level: 0,
            font_id: FontId(0),
            direction: Direction::default(),
        };
        let atlas = alkalive_text::MockGlyphAtlas;
        let calls = glyph_run_to_draw_calls(&shaped, &atlas);
        assert!(
            calls.is_empty(),
            "empty shaped run must yield no draw calls, got {calls:?}",
        );
    }

    /// `GlyphQuadBatch` is re-exported from `alkalive-text` so the render
    /// crate and the text crate share one canonical batch carrier (Wave 3).
    #[test]
    fn glyph_quad_batch_re_export_constructs() {
        // If the re-export is broken (e.g. a future rename in
        // `alkalive-text`), this line fails to compile.
        let batch = GlyphQuadBatch::default();
        assert!(batch.quads.is_empty());
        assert!(batch.font_ids.is_empty());
    }
}
