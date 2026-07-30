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

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use core::ops::Range;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};

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

/// Module identifier; mirrors `alkalive_core::ModuleId`.
///
/// A local copy is kept so this crate compiles standalone under the Wave 3
/// "no external deps" rule; it will be replaced by a re-export once the
/// cross-crate dependency is wired in Wave 5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModuleId {
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
#[derive(Debug, Clone, Default)]
pub struct CompiledGraph;

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

    let mut visited: usize = 0;
    while let Some(i) = queue.pop_front() {
        visited += 1;
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

    // The merged, topologically-validated graph is the compiler's output.
    // Wave 5 keeps `CompiledGraph` as a unit struct; batching, barrier
    // insertion, and the occlusion-cull pass (W5-T2/T3) will populate it in a
    // follow-up without changing the public signature. The merge result is
    // retained here for validation; `_merged_draw_calls` is collected so the
    // batcher has a stable handoff point in the next task.
    let _ = (merged_passes, merged_attachments, merged_draw_calls, merged_edges);

    Ok(CompiledGraph)
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
/// Wave 5 (W5-T6) implements an unbounded linear-search cache. The 64 MB LRU
/// bound (§12.7) and the degraded-builtin fallback path land in a follow-up;
/// see the `TODO` on [`PipelineCache::insert`].
#[derive(Debug, Clone, Default)]
pub struct PipelineCache {
    /// Linear-search entries. A `HashMap` is intentionally avoided at this
    /// stage so that `PipelineDesc`'s derive set stays minimal; W5-T6 will
    /// introduce a hashed index bounded by the 64 MB cap.
    entries: Vec<(PipelineDesc, PipelineHandle)>,
}

impl PipelineCache {
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
    // TODO(W5-T6): bound the cache at 64 MB (§12.7) with LRU eviction; the
    // miss path should fall back to a degraded builtin pipeline and emit a
    // `PipelineError` to the unified trace. Until then this is an unbounded
    // append.
    pub fn insert(&mut self, desc: &PipelineDesc, handle: PipelineHandle) {
        self.entries.push((desc.clone(), handle));
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
        ModuleId { value: v }
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
    }

    // --- compile(): two graphs merged → Ok -------------------------------------

    #[test]
    fn compile_two_graphs_merged_ok() {
        // g1 declares pass 1 in isolation; g2 declares pass 2 with a cross-graph
        // barrier 1 → 2. The merged compiler must resolve the cross-graph edge.
        let g1 = graph(vec![bare_pass(1)], vec![], vec![], mid(0));
        let g2 = graph(vec![bare_pass(2)], vec![], vec![(pid(1), pid(2))], mid(1));

        let result = compile(&[g1, g2], &[], &DepthBuffer);
        assert!(result.is_ok(), "two merged graphs with a cross-graph edge must compile");
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

    // --- PassBuilder: finish returns incrementing DrawCallIds ------------------

    #[test]
    fn pass_builder_finish_increments() {
        // The counter is process-wide; assert strict increment regardless of the
        // absolute starting value (other tests may have bumped it first).
        let a = PassBuilder::default().finish();
        let b = PassBuilder::default().finish();
        let c = PassBuilder::default().finish();
        assert_eq!(b.value, a.value + 1, "second finish must be exactly one past the first");
        assert_eq!(c.value, b.value + 1, "third finish must be exactly one past the second");
    }

    // --- PassBuilder: records pipeline + draws ---------------------------------

    #[test]
    fn pass_builder_records_pipeline_and_draws() {
        let mut builder = PassBuilder::default();
        builder
            .bind_pipeline(ph(42))
            .draw(0..10)
            .draw(10..20);

        // Private fields are visible to this child test module.
        assert_eq!(builder.pipeline, Some(ph(42)), "bound pipeline must be stored");
        assert_eq!(builder.draws.len(), 2, "both draw ranges must be recorded");
        assert_eq!(builder.draws[0], 0..10, "first draw range preserved in order");
        assert_eq!(builder.draws[1], 10..20, "second draw range preserved in order");

        // finish consumes the builder and still yields a valid id.
        let id = builder.finish();
        // id.value only needs to be a valid u64; increment behaviour is covered above.
        let _ = id;
    }
}
