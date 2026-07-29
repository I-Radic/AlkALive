//! AlkALive alkalive-render crate.
//!
//! Backend-abstracted render-graph IR, the render-graph compiler, the retained
//! render loop, and the compositor defined in `docs/SPECIFICATION.md`
//! §4.1–§4.7. This is a Wave 3 trait-definition skeleton: every domain method
//! body is `todo!()`; no behaviour is implemented yet.
//!
//! Per `IMPLEMENTATION_PLAN.md` task W3-T2 / Wave 3 DoD, the only function
//! bodies present are `todo!()`; `Debug`/`Clone`/`Default` are obtained via
//! `derive` (which generate, not hand-write, their impls).
//!
//! See `docs/SPECIFICATION.md` §4.1–§4.7 for the authoritative signatures.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use core::ops::Range;

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
pub fn compile(
    graphs: &[RenderGraph],
    dirty: &[DirtyRect],
    depth: &DepthBuffer,
) -> Result<CompiledGraph, CompileError> {
    let _ = (graphs, dirty, depth);
    todo!()
}

// ===========================================================================
// §4.4 Retained render loop & dirty-rect invalidation
// ===========================================================================

/// Builder returned by [`RenderLoop::begin_pass`].
#[derive(Debug, Clone, Default)]
pub struct PassBuilder;

impl PassBuilder {
    /// Bind a cached pipeline to the pass under construction.
    pub fn bind_pipeline(&mut self, handle: PipelineHandle) -> &mut Self {
        let _ = handle;
        todo!()
    }

    /// Record a draw over the given instance range.
    pub fn draw(&mut self, instances: Range<u32>) -> &mut Self {
        let _ = instances;
        todo!()
    }

    /// Finish the pass and return its assigned draw-call identifier.
    pub fn finish(self) -> DrawCallId {
        todo!()
    }
}

/// The retained-mode render loop trait (§4.4).
///
/// Scene-graph state persists across frames and only dirty rectangles or
/// per-object subsets are re-emitted (ADR 002).
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
#[derive(Debug, Clone, Default)]
pub struct PipelineCache;

impl PipelineCache {
    /// Look up a cached pipeline by its key triple.
    pub fn get(
        &self,
        shader_hash: u64,
        layout_hash: u64,
        format: AttachmentFormat,
    ) -> Option<PipelineHandle> {
        let _ = (shader_hash, layout_hash, format);
        todo!()
    }

    /// Insert a compiled pipeline into the cache.
    pub fn insert(&mut self, desc: &PipelineDesc, handle: PipelineHandle) {
        let _ = (desc, handle);
        todo!()
    }
}

/// The compositor trait (§3.6 / §4.5).
///
/// The main thread owns the lone `GPUDevice` (ADR 003 / ADR 021); on-demand
/// WASM workers never acquire it. The compositor merges, compiles, reorders,
/// batches, then submits.
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
#[derive(Debug, Clone)]
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
