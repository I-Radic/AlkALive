//! Text rendering stack — forked in-WASM HarfRust (ADR 022).
//!
//! Trait surface with **required** methods (no `todo!()` defaults). Wave 1
//! ships the default HarfRust-backed implementations: [`HarfRustFontRegistry`]
//! (§6.2) and [`HarfRustTextShaper`] (§6.3). The remaining trait surface
//! (LRU glyph atlas, editing ops, `TextStack` measure/rasterize/a11y/ime
//! adapters per §6.4–6.9) still uses [`MockGlyphAtlas`] and [`MockTextStack`]
//! as non-panicking stubs; those mocks are retained as test-only fallbacks.
//!
//! # Cross-crate forward declarations
//!
//! This crate ships self-contained (no external deps, no workspace deps).
//! Three cross-crate types referenced by the spec are stubbed here:
//! - [`ModuleId`] — concrete newtype lives in `alkalive-layout` (ADR 002).
//! - [`DirtyRect`] — concrete struct lives in `alkalive-layout` (§4.4/§5.5).
//! - [`Rect`] — concrete struct lives in `alkalive-layout` (§5.2); used here
//!   only as an atlas UV box.
//!
//! All three are unified by the future rendering-ABI ADR (§4.7 / §5.4
//! shared-boundary note). The stubs exist only so the text crate compiles
//! standalone in Wave 3.
//!
//! # No DOM text path (ADR 020, ADR 022)
//!
//! There is no `<canvas.fillText>`, no hidden text nodes, no `measureText`
//! fallback. The text stack is the sole producer of glyph geometry; ADR 013's
//! hot-path integrity is preserved structurally because no WASM↔DOM boundary
//! exists for text.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub use alkalive_core::ModuleId;

use harfrust::font::{Font, FontInstance};
use harfrust::{
    shape as harfrust_shape, Direction as HarfRustDirection, ShapeOptions, UnicodeBuffer,
};
use read_fonts::TableProvider;
use std::sync::Arc;

// ============================================================================
// Forward-declared cross-crate placeholders
// ============================================================================

/// Placeholder for the layout crate's `DirtyRect` (§4.4 / §5.5).
///
/// Opaque in the text crate: `GlyphAtlas::invalidate` receives it from the
/// layout side; the rendering-ABI ADR (§4.7) will unify the concrete shape.
#[derive(Clone, Copy, Debug, Default)]
pub struct DirtyRect;

/// Forward-declared layout `Rect` used as an atlas UV box (§5.2 / §6.4).
///
/// The concrete struct lives in `alkalive-layout`; the rendering-ABI ADR
/// (§4.7) will unify it. Inlined here as a flat `(x, y, w, h)` carrier so
/// [`AtlasSlot`] and [`Quad`] compile standalone.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    /// X origin.
    pub x: f32,
    /// Y origin.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

// ============================================================================
// Opaque font / script identifiers
// ============================================================================

/// Stable handle for a decoded font face in the registry (§6.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct FontId(pub u32);

/// An OpenType 4-byte tag (e.g. `b"glyf"`, `b"cmap"`) — used by
/// [`FontLoadError::TableDecodeFailed`] (§6.2) and table lookup.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Tag(pub [u8; 4]);

/// A Unicode `Script` value (ISO 15924-style), used by
/// [`ShapeError::UnsupportedScript`] (§6.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Script(pub u32);

// ============================================================================
// Direction & affinity (§6.3, §6.6)
// ============================================================================

/// Paragraph / run direction (§6.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Direction {
    /// Left-to-right.
    #[default]
    Ltr,
    /// Right-to-left.
    Rtl,
}

/// Caret affinity (§6.6): which side of a soft break a caret rests on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Affinity {
    /// Caret leans upstream (towards the previous line).
    #[default]
    Upstream,
    /// Caret leans downstream (towards the next line).
    Downstream,
}

// ============================================================================
// Font loading (§6.2)
// ============================================================================

/// Family/weight/style triple resolved by [`FontRegistry::resolve`] (§6.2).
///
/// Resolution follows a fallback chain: requested family → generic family
/// alias (`serif`/`sans`/`mono`) → bundled default. A miss never aborts
/// layout; the registry returns [`FontLoadError::FallbackResolved`] carrying
/// the substituted [`FontId`] so the caller can re-shape against the fallback.
#[derive(Clone, Debug, Default)]
pub struct FontRequest {
    /// Requested family name.
    pub family: String,
    /// Weight (100–900 CSS scale).
    pub weight: u16,
    /// Style: `"normal"`, `"italic"`, or `"oblique"`.
    pub style: &'static str,
}

/// Parsed OpenType tables served from WASM-heap memory (§6.2).
#[derive(Clone, Debug, Default)]
pub struct DecodedFace {
    /// Resolved font id.
    pub id: FontId,
    /// Units-per-em from the `head` table.
    pub units_per_em: u16,
    /// Ascender (font units).
    pub ascender: i16,
    /// Descender (font units).
    pub descender: i16,
    /// Line gap (font units).
    pub line_gap: i16,
}

/// [`FontRegistry`] failure modes (§6.2).
#[derive(Clone, Debug)]
pub enum FontLoadError {
    /// No family matched and no alias resolved.
    FamilyNotFound,
    /// Family matched but requested weight unavailable.
    WeightUnavailable,
    /// An OpenType table failed to decode.
    TableDecodeFailed {
        /// Face whose table failed.
        font_id: FontId,
        /// Offending 4-byte table tag.
        table: Tag,
    },
    /// Soft failure: a fallback was substituted. Caller re-shapes against `actual`.
    FallbackResolved {
        /// The substituted font id.
        actual: FontId,
    },
    /// Registry has no fonts loaded.
    RegistryEmpty,
}

/// Font registry — resolves family/weight/style to a decoded face and
/// caches parsed OpenType tables for HarfRust (§6.2). Serves HarfRust
/// directly from WASM-heap memory.
///
/// Every method is required (no default body). The default implementation is
/// [`HarfRustFontRegistry`]; [`MockFontRegistry`] is retained as a
/// test-only stub.
pub trait FontRegistry {
    /// Resolve a [`FontRequest`] to a [`FontId`], following the fallback
    /// chain (requested → generic alias → bundled default).
    fn resolve(&mut self, req: &FontRequest) -> Result<FontId, FontLoadError>;

    /// Look up the cached [`DecodedFace`] for `id`.
    fn face(&self, id: FontId) -> &DecodedFace;

    /// Load a font bundle from raw bytes (WASM-heap).
    fn load_bundle(&mut self, bytes: &[u8]) -> Result<FontId, FontLoadError>;

    /// Return the fallback chain beginning at `id`.
    fn fallback_chain(&self, id: FontId) -> &[FontId];
}

// ============================================================================
// Shaping (§6.3)
// ============================================================================

/// BiDi-aware glyph-idx ↔ caret-offset map (§6.3).
#[derive(Clone, Debug, Default)]
pub struct ClusterMap {
    /// Per-glyph source-codepoint index.
    pub glyph_to_cluster: Vec<u32>,
    /// Per-caret glyph index.
    pub caret_to_glyph: Vec<u32>,
}

/// Per-run metrics emitted alongside a [`ShapedRun`] (§6.3).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RunMetrics {
    /// Ascent.
    pub ascent: f32,
    /// Descent (typically negative).
    pub descent: f32,
    /// Built-in line gap.
    pub line_gap: f32,
    /// Sum of advances.
    pub total_advance: f32,
}

/// Bundle of resolved font/size/direction fed into [`TextShaper::shape`]
/// (§6.3).
#[derive(Clone, Debug, Default)]
pub struct ShapeContext {
    /// Resolved font (fallback-aware).
    pub font: FontId,
    /// Pixel size.
    pub size_px: f32,
    /// Explicit direction; `None` = auto-detect from script.
    pub direction: Option<Direction>,
}

/// Immutable output of [`TextShaper::shape`] (§6.3).
///
/// A run is immutable after shaping; downstream consumers (layout's
/// measured-run contract per §5.4/ADR 004, the rasterizer, the hit-tester)
/// all read the same instance. Uncovered codepoints surface as `.notdef`
/// glyph IDs with real metrics (visible tofu) — the pipeline never aborts
/// on missing coverage.
#[derive(Clone, Debug)]
pub struct ShapedRun {
    /// HarfRust-shaped glyph IDs.
    pub glyph_ids: Box<[u32]>,
    /// Per-glyph x-advance (signed for RTL).
    pub advances: Box<[f32]>,
    /// Baseline-relative offset per glyph.
    pub offsets: Box<[(f32, f32)]>,
    /// Source-codepoint index per glyph.
    pub clusters: Box<[u32]>,
    /// Glyph-idx ↔ caret-offset map (BiDi-aware).
    pub caret_map: ClusterMap,
    /// Ascent/descent/line_gap/total_advance.
    pub metrics: RunMetrics,
    /// Unicode BiDi embedding level.
    pub bidi_level: u8,
    /// Resolved font (fallback-aware).
    pub font_id: FontId,
    /// Paragraph direction.
    pub direction: Direction,
}

/// [`TextShaper`] failure modes (§6.3).
#[derive(Clone, Debug)]
pub enum ShapeError {
    /// `FontId` not registered.
    FontUnresolved,
    /// Source string is not valid UTF-8.
    InvalidUtf8,
    /// BiDi embedding level overflowed.
    BidiOverflow {
        /// Offending level.
        level: u8,
    },
    /// Script not supported by any face in the fallback chain.
    UnsupportedScript {
        /// Offending script.
        script: Script,
    },
    /// Every glyph resolved to `.notdef`.
    NotdefOnly,
}

/// HarfRust-backed shaper — accepts a Unicode run plus resolved font/style/
/// language, performs BiDi segmentation and reordering in-WASM, and emits
/// an immutable [`ShapedRun`] (§6.3).
///
/// Every method is required (no default body). The default implementation is
/// [`HarfRustTextShaper`]; [`MockTextStack`] is retained as a test-only stub.
pub trait TextShaper {
    /// Shape `run` under the given context.
    fn shape(&self, run: &str, ctx: &ShapeContext) -> Result<ShapedRun, ShapeError>;

    /// Re-shape an existing run against a different font (fallback path).
    fn reshape_with_font(&self, run: &str, font: FontId) -> Result<ShapedRun, ShapeError>;
}

// ============================================================================
// Glyph atlas (§6.4)
// ============================================================================

/// Atlas lookup key: `(font_id, glyph_id, subpixel_phase, size_px)` (§6.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct GlyphKey {
    /// Owning font.
    pub font_id: FontId,
    /// Glyph index in the font.
    pub glyph_id: u32,
    /// Subpixel phase (0..3 typically).
    pub phase: u8,
    /// Pixel size.
    pub size_px: u16,
}

/// Atlas placement of a rasterised glyph (§6.4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AtlasSlot {
    /// Atlas page index.
    pub page: u16,
    /// UV rectangle inside the page.
    pub uv: Rect,
    /// Baseline-relative bearing.
    pub bearing: (f32, f32),
    /// Glyph pixel size.
    pub size: (f32, f32),
}

/// Set of [`GlyphKey`]s pinned by in-flight render-graph IR (§6.4); excluded
/// from LRU eviction.
#[derive(Clone, Debug, Default)]
pub struct PinSet {
    /// Pinned keys.
    pub keys: Vec<GlyphKey>,
}

/// Statistics returned by [`GlyphAtlas::evict_lru`] (§6.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EvictionStats {
    /// Number of slots evicted.
    pub evicted: u32,
    /// Number of slots retained (pinned).
    pub retained: u32,
    /// Bytes reclaimed.
    pub bytes_reclaimed: u64,
}

/// GPU-resident LRU glyph atlas (§6.4).
///
/// Slots are addressed by [`GlyphKey`]; a miss triggers HarfRust
/// rasterisation into a staging buffer, then a `queue.writeTexture` upload
/// into the next free tile. Eviction is LRU with a pin set held by in-flight
/// render-graph IR (§4). Invalidation follows per-module dirty-rect locality
/// (ADR 002): a re-shaped run only dirties its own atlas footprint, never
/// the whole atlas.
///
/// Every method is required (no default body); the real GPU-backed
/// implementation lands in a later wave. [`MockGlyphAtlas`] provides a
/// non-panicking stub for downstream consumers today.
pub trait GlyphAtlas {
    /// Rasterize-on-demand: ensure `key` is resident and return its slot.
    fn ensure(&mut self, key: GlyphKey) -> AtlasSlot;

    /// Cached-only lookup; `None` if not resident.
    fn slot(&self, key: GlyphKey) -> Option<AtlasSlot>;

    /// Invalidate the atlas footprint of `rect` in `module_id` (ADR 002).
    fn invalidate(&mut self, module_id: ModuleId, rect: DirtyRect);

    /// LRU eviction, keeping every key in `keep`. Returns eviction stats.
    fn evict_lru(&mut self, keep: &PinSet) -> EvictionStats;
}

// ============================================================================
// Rasterization IR (§6.5)
// ============================================================================

/// A single textured glyph quad (§6.5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quad {
    /// Screen-space position (x, y).
    pub position: (f32, f32),
    /// Screen-space size (w, h).
    pub size: (f32, f32),
    /// Atlas UV rectangle.
    pub uv: Rect,
    /// Atlas page index.
    pub page: u16,
}

/// Batched glyph quads emitted by [`TextStack::rasterize`] (§6.5),
/// referencing atlas UVs — no pixel work happens on the hot path beyond the
/// first-seen upload. The compositor (ADR 003) batches glyph quads across
/// modules into a single instanced draw.
#[derive(Clone, Debug, Default)]
pub struct GlyphQuadBatch {
    /// Quads in submission order.
    pub quads: Vec<Quad>,
    /// Source [`FontId`]s, parallel to `quads` for hit-testing.
    pub font_ids: Vec<FontId>,
}

// ============================================================================
// Editing primitives (§6.6)
// ============================================================================

/// A caret position: codepoint index + affinity (§6.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct CaretOffset {
    /// Codepoint index into the source string.
    pub cp_index: u32,
    /// Directional affinity.
    pub affinity: Affinity,
}

/// BiDi-aware anchor + active caret (§6.6). Anchor/active offsets are
/// codepoint indices into the source string.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct CaretSelection {
    /// Anchor (selection start).
    pub anchor: CaretOffset,
    /// Active end (caret position).
    pub active: CaretOffset,
}

/// Hit-testing and caret/selection geometry over a [`ShapedRun`] (§6.6).
///
/// Built atop HarfRust output (ADR 022 negative consequence: no DOM
/// contracts inherited). `hit_test` maps a point to the nearest caret via
/// the run's `caret_map`, honoring directional affinity.
///
/// Every method is required (no default body); the real HarfRust-backed
/// implementation lands in a later wave. [`MockTextStack`] provides a
/// non-panicking stub for downstream consumers today.
pub trait EditingOps {
    /// Map a screen-space point to the nearest caret offset.
    fn hit_test(&self, run: &ShapedRun, point: (f32, f32)) -> CaretOffset;

    /// Map a caret offset to its screen-space (x, y) position.
    fn caret_position(&self, run: &ShapedRun, offset: CaretOffset) -> (f32, f32);

    /// Build selection-highlight quads for `sel` over `run`.
    fn selection_quads(&self, run: &ShapedRun, sel: CaretSelection) -> Box<[Quad]>;
}

// ============================================================================
// IME (§6.7)
// ============================================================================

/// IME composition event (§6.7); acquisition mechanism is pluggable behind
/// the [`TextStack::ime_compose`] interface.
///
/// See `Spec_Tradeoff_Note_IME.md` — ADR 020 forbids DOM input elements;
/// no ADR commits a replacement for acquiring platform IME composition
/// events. Candidates: (a) WASM-native platform input API (no DOM);
/// (b) a narrowly-scoped hidden `<input>` carrying composition state only,
/// classified non-hot-path, requiring a formal ADR 020 exception;
/// (c) defer IME until a platform API matures.
#[derive(Clone, Debug)]
pub struct CompositionEvent {
    /// Composition string delta (UTF-8).
    pub text: String,
    /// Caret within the composition string, in codepoints.
    pub caret: u32,
    /// Replacement range in the host string, in codepoints.
    pub replace_range: (u32, u32),
}

/// State returned by [`TextStack::ime_compose`] (§6.7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImeState {
    /// Composition ongoing; `text` is the in-flight composition string.
    Composing {
        /// The in-flight composition string.
        text: String,
    },
    /// Composition committed to the buffer.
    Committed {
        /// The committed string.
        text: String,
    },
    /// Composition cancelled.
    Cancelled,
}

// ============================================================================
// A11y text exposure (§6.8)
// ============================================================================

/// Plain-text label carried on a render object (§10.5). The concrete label
/// string surfaced to the future a11y tree.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct TextLabel(pub String);

/// Placeholder payload returned by [`TextStack::expose_a11y_text`] (§6.8):
/// source text, caret/selection, run metrics — sufficient for any future
/// a11y derivation layer built against the render-object graph (ADR 007) to
/// consume without re-shaping.
///
/// Accessibility is deferred per ADR 019; no DOM a11y contracts are
/// inherited. This placeholder exists so un-deferral is additive, not
/// architectural.
#[derive(Clone, Debug, Default)]
pub struct A11yTextPlaceholder {
    /// Source text of the shaped run.
    pub text: String,
    /// Current caret/selection (BiDi-aware).
    pub selection: Option<CaretSelection>,
    /// Run metrics.
    pub metrics: RunMetrics,
    /// Optional a11y label.
    pub label: Option<TextLabel>,
}

// ============================================================================
// TextStack top-level interface (§6.9)
// ============================================================================

/// [`TextStack::measure`] output (§6.9): adapts [`ShapedRun`] to the
/// `GlyphMetrics`/`LineBreak` types expected by the layout solver's
/// `MeasuredRun` contract (§5.4 / §6.9 shared-boundary note).
#[derive(Clone, Debug, Default)]
pub struct MeasuredLines {
    /// Per-line total advance.
    pub line_advances: Vec<f32>,
    /// Per-line glyph index range `(start, end)`.
    pub line_ranges: Vec<(u32, u32)>,
    /// Aggregate metrics across all lines.
    pub metrics: RunMetrics,
}

/// Top-level text interface (§6.9). Composes [`TextShaper`] + [`EditingOps`]
/// and adds the `MeasuredRun`-contract adapter ([`measure`](TextStack::measure)),
/// the render-graph-IR rasterizer ([`rasterize`](TextStack::rasterize)),
/// the deferred-a11y placeholder
/// ([`expose_a11y_text`](TextStack::expose_a11y_text)), and the IME
/// composition hook ([`ime_compose`](TextStack::ime_compose)).
///
/// This is the canonical §5↔§6 boundary: §5 (layout) consumes the
/// `MeasuredRun` interface; §6 (text stack) implements it via
/// [`TextStack::measure`]. The two interfaces are semantically identical
/// (synchronous, HarfRust-backed, no DOM) and the rendering-ABI ADR (§4.7)
/// will unify their type signatures precisely.
///
/// Every method is required (no default body); the real HarfRust-backed
/// implementation lands in a later wave. [`MockTextStack`] provides a
/// non-panicking stub for downstream consumers today.
pub trait TextStack: TextShaper + EditingOps {
    /// Implements the `MeasuredRun` contract consumed by §5's
    /// `LayoutSolver`. Adapts [`ShapedRun`] output to `GlyphMetrics`/
    /// `LineBreak` types (ADR 004).
    fn measure(&self, run: &ShapedRun, max_width: f32) -> MeasuredLines;

    /// Walk `run`, query the atlas for each [`GlyphKey`], and emit a
    /// [`GlyphQuadBatch`] referencing atlas UVs (§6.5). No pixel work
    /// happens on the hot path beyond the first-seen upload.
    fn rasterize(&self, run: &ShapedRun, atlas: &mut dyn GlyphAtlas) -> GlyphQuadBatch;

    /// Deferred-a11y placeholder (§6.8): returns source text, caret/selection,
    /// and run metrics so any future derivation layer can consume it without
    /// reshaping.
    fn expose_a11y_text(&self, run: &ShapedRun) -> A11yTextPlaceholder;

    /// IME composition hook (§6.7); acquisition mechanism is pluggable behind
    /// this stable interface.
    fn ime_compose(&mut self, ev: CompositionEvent) -> ImeState;
}

// ============================================================================
// HarfRust-backed implementations (Wave 1 default — ADR 022)
// ============================================================================

/// Internal entry for a registered font face.
struct HarfRustFontEntry {
    /// Stable handle.
    id: FontId,
    /// Parsed, Arc-reference-counted font (cheap to clone for shaping).
    font: Font,
    /// Cached decoded metrics.
    face: DecodedFace,
}

/// HarfRust-backed [`FontRegistry`] — the default implementation (ADR 022).
///
/// Stores parsed OpenType tables on the WASM heap via [`harfrust::font::Font`]
/// (internally `Arc`-reference-counted, `Send + Sync`). Each [`FontId`] maps
/// to a single face entry. Wave 1 limitations:
/// - [`resolve`](FontRegistry::resolve) returns the first loaded font;
///   real family/weight/style matching is deferred.
/// - [`fallback_chain`](FontRegistry::fallback_chain) always returns `&[]`;
///   real fallback chains land with the font-config integration.
#[derive(Default)]
pub struct HarfRustFontRegistry {
    /// Registered font entries, indexed by `FontId(u32)` == position.
    entries: Vec<HarfRustFontEntry>,
}

impl HarfRustFontRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Returns a cheap clone of the underlying [`Font`] for `id`, if
    /// registered. Used by [`HarfRustTextShaper`] to build a fresh
    /// [`FontInstance`] per shape call.
    pub fn font(&self, id: FontId) -> Option<Font> {
        self.entries
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.font.clone())
    }

    /// Number of registered faces.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if no faces are registered.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl FontRegistry for HarfRustFontRegistry {
    fn resolve(&mut self, _req: &FontRequest) -> Result<FontId, FontLoadError> {
        // Wave 1: return the first loaded font. Real family/weight/style
        // matching lands with the font-config integration.
        self.entries
            .first()
            .map(|e| e.id)
            .ok_or(FontLoadError::RegistryEmpty)
    }

    fn face(&self, id: FontId) -> &DecodedFace {
        self.entries
            .iter()
            .find(|e| e.id == id)
            .map(|e| &e.face)
            .expect("HarfRustFontRegistry::face: FontId not registered")
    }

    fn load_bundle(&mut self, bytes: &[u8]) -> Result<FontId, FontLoadError> {
        let font = Font::new(bytes.to_vec(), 0).map_err(|_| FontLoadError::TableDecodeFailed {
            font_id: FontId::default(),
            table: Tag(*b"sfnt"),
        })?;
        let id = FontId(self.entries.len() as u32);
        let face = decode_face(&font, id);
        self.entries.push(HarfRustFontEntry { id, font, face });
        Ok(id)
    }

    fn fallback_chain(&self, _id: FontId) -> &[FontId] {
        &[]
    }
}

/// Decode the `head`/`hhea` metrics for a [`Font`] into a [`DecodedFace`].
///
/// Missing tables are tolerated — the corresponding metric defaults to `0`.
fn decode_face(font: &Font, id: FontId) -> DecodedFace {
    let tables = font.tables();
    let units_per_em = tables.head().map(|h| h.units_per_em()).unwrap_or(0);
    let (ascender, descender, line_gap) = tables
        .hhea()
        .map(|h| {
            (
                h.ascender().to_i16(),
                h.descender().to_i16(),
                h.line_gap().to_i16(),
            )
        })
        .unwrap_or((0, 0, 0));
    DecodedFace {
        id,
        units_per_em,
        ascender,
        descender,
        line_gap,
    }
}

/// Default size (px) used by [`HarfRustTextShaper::reshape_with_font`] when
/// no [`ShapeContext`] is available. The trait method
/// [`TextShaper::reshape_with_font`] only receives a [`FontId`], so a
/// reasonable default is required; `16.0` px matches common body-text size.
const DEFAULT_SHAPE_SIZE_PX: f32 = 16.0;

/// Fixed-point divisor for HarfRust's 16.16 position format. When a
/// [`FontInstance`] has a non-`None` size, HarfRust's free `shape` function
/// auto-sets the scale to `ppem * 65536`, producing positions in 16.16
/// fixed point. Dividing by `65536.0` recovers pixel-space `f32` values.
const HARF_POSITION_DIVISOR: f32 = 65536.0;

/// HarfRust-backed [`TextShaper`] — the default implementation (ADR 022).
///
/// Holds an [`Arc`] share of a [`HarfRustFontRegistry`] so multiple shapers
/// can share one registry. Each [`shape`](TextShaper::shape) call builds a
/// fresh [`FontInstance`] (which is neither `Clone` nor `Sync` — it holds
/// mutable shaping state) from the registry's [`Font`] clone.
///
/// Wave 1 limitations:
/// - BiDi is not implemented; the run is shaped as a single segment with
///   direction from [`ShapeContext::direction`] or auto-detected by
///   HarfRust's `guess_segment_properties`.
/// - [`reshape_with_font`](TextShaper::reshape_with_font) uses
///   [`DEFAULT_SHAPE_SIZE_PX`] (the trait doesn't pass a [`ShapeContext`]).
/// - `ShapeError::NotdefOnly` is never returned; uncovered codepoints
///   surface as `.notdef` glyphs with real metrics (visible tofu) per §6.3.
pub struct HarfRustTextShaper {
    /// Read-only share of the font registry.
    registry: Arc<HarfRustFontRegistry>,
}

impl HarfRustTextShaper {
    /// Create a shaper backed by `registry`. The registry should have all
    /// fonts loaded before the shaper is constructed (the shaper takes a
    /// read-only [`Arc`] share).
    pub fn new(registry: Arc<HarfRustFontRegistry>) -> Self {
        Self { registry }
    }
}

impl TextShaper for HarfRustTextShaper {
    fn shape(&self, run: &str, ctx: &ShapeContext) -> Result<ShapedRun, ShapeError> {
        shape_run(&self.registry, run, ctx.font, ctx.size_px, ctx.direction)
    }

    fn reshape_with_font(&self, run: &str, font: FontId) -> Result<ShapedRun, ShapeError> {
        shape_run(
            &self.registry,
            run,
            font,
            DEFAULT_SHAPE_SIZE_PX,
            None,
        )
    }
}

/// Core shaping routine shared by [`HarfRustTextShaper::shape`] and
/// [`HarfRustTextShaper::reshape_with_font`].
///
/// Builds a fresh [`FontInstance`], runs HarfRust's [`harfrust_shape`], and
/// converts the [`harfrust::GlyphBuffer`] into a [`ShapedRun`].
fn shape_run(
    registry: &HarfRustFontRegistry,
    text: &str,
    font_id: FontId,
    size_px: f32,
    direction: Option<Direction>,
) -> Result<ShapedRun, ShapeError> {
    let font = registry.font(font_id).ok_or(ShapeError::FontUnresolved)?;

    // FontInstance is neither Clone nor Sync — create a fresh one per call.
    let instance = FontInstance::builder(&font).size(Some(size_px)).build();

    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(text);
    if let Some(dir) = direction {
        buffer.set_direction(map_direction_to_harfrust(dir));
    }
    // Auto-detect script (and direction if not explicitly set).
    buffer.guess_segment_properties();
    let resolved_harfrust_dir = buffer.direction();

    let glyph_buffer = harfrust_shape(&instance, buffer, ShapeOptions::new());

    let infos = glyph_buffer.glyph_infos();
    let positions = glyph_buffer.glyph_positions();

    let n = infos.len();
    let mut glyph_ids = Vec::with_capacity(n);
    let mut advances = Vec::with_capacity(n);
    let mut offsets = Vec::with_capacity(n);
    let mut clusters = Vec::with_capacity(n);
    let mut total_advance = 0.0f32;

    for (info, pos) in infos.iter().zip(positions.iter()) {
        glyph_ids.push(info.glyph_id);
        let x_adv = pos.x_advance as f32 / HARF_POSITION_DIVISOR;
        advances.push(x_adv);
        offsets.push((
            pos.x_offset as f32 / HARF_POSITION_DIVISOR,
            pos.y_offset as f32 / HARF_POSITION_DIVISOR,
        ));
        clusters.push(info.cluster);
        total_advance += x_adv;
    }

    // Font metrics scaled from font units to pixels.
    let face = registry.face(font_id);
    let scale = if face.units_per_em > 0 {
        size_px / face.units_per_em as f32
    } else {
        0.0
    };
    let metrics = RunMetrics {
        ascent: face.ascender as f32 * scale,
        descent: face.descender as f32 * scale,
        line_gap: face.line_gap as f32 * scale,
        total_advance,
    };

    let out_direction = map_direction_from_harfrust(resolved_harfrust_dir);
    // Wave 1: BiDi is not implemented. Embedding level is derived from
    // direction only (0 for LTR, 1 for RTL).
    let bidi_level: u8 = if out_direction == Direction::Rtl { 1 } else { 0 };

    // Cluster map: glyph_to_cluster mirrors the per-glyph cluster index;
    // caret_to_glyph maps each caret boundary to the glyph index immediately
    // to its right (LTR) — for an N-glyph run there are N+1 carets.
    let glyph_to_cluster = clusters.clone();
    let caret_to_glyph: Vec<u32> = (0..=n).map(|i| i as u32).collect();

    Ok(ShapedRun {
        glyph_ids: glyph_ids.into_boxed_slice(),
        advances: advances.into_boxed_slice(),
        offsets: offsets.into_boxed_slice(),
        clusters: clusters.into_boxed_slice(),
        caret_map: ClusterMap {
            glyph_to_cluster,
            caret_to_glyph,
        },
        metrics,
        bidi_level,
        font_id,
        direction: out_direction,
    })
}

/// Map our [`Direction`] to HarfRust's [`HarfRustDirection`].
fn map_direction_to_harfrust(dir: Direction) -> HarfRustDirection {
    match dir {
        Direction::Ltr => HarfRustDirection::LeftToRight,
        Direction::Rtl => HarfRustDirection::RightToLeft,
    }
}

/// Map HarfRust's [`HarfRustDirection`] back to our [`Direction`].
///
/// Vertical and `Invalid` directions are folded to [`Direction::Ltr`] for
/// Wave 1; vertical text support lands in a later wave.
fn map_direction_from_harfrust(dir: HarfRustDirection) -> Direction {
    match dir {
        HarfRustDirection::RightToLeft => Direction::Rtl,
        _ => Direction::Ltr,
    }
}

// ============================================================================
// Wave-3 mock implementations (test-only — prefer HarfRust* defaults above)
// ============================================================================

/// Construct a minimal empty [`ShapedRun`] for the mock text stack.
///
/// All glyph buffers are empty, metrics are default, BiDi level is `0`,
/// the font is [`FontId::default`] (`FontId(0)`), and the direction is
/// [`Direction::Ltr`]. This is the smallest legal [`ShapedRun`] — sufficient
/// to exercise downstream consumers against the trait surface today.
fn empty_shaped_run() -> ShapedRun {
    ShapedRun {
        glyph_ids: Box::new([]),
        advances: Box::new([]),
        offsets: Box::new([]),
        clusters: Box::new([]),
        caret_map: ClusterMap::default(),
        metrics: RunMetrics::default(),
        bidi_level: 0,
        font_id: FontId(0),
        direction: Direction::Ltr,
    }
}

/// **Test-only** mock [`FontRegistry`] with stub implementations of every
/// method.
///
/// Returns neutral/empty outputs for every operation — sufficient to compile
/// downstream consumers against the trait surface. The default
/// HarfRust-backed implementation is [`HarfRustFontRegistry`]; this mock is
/// retained for tests and as a non-panicking fallback.
///
/// - [`resolve`](FontRegistry::resolve) always returns `Ok(FontId(0))`.
/// - [`face`](FontRegistry::face) always returns a default [`DecodedFace`].
/// - [`load_bundle`](FontRegistry::load_bundle) always returns `Ok(FontId(0))`.
/// - [`fallback_chain`](FontRegistry::fallback_chain) always returns `&[]`.
///
/// The mock holds a single default [`DecodedFace`] so `face` can return a
/// stable reference; it is otherwise stateless.
#[derive(Debug, Default, Clone)]
pub struct MockFontRegistry {
    /// Default face returned by [`FontRegistry::face`].
    default_face: DecodedFace,
}

impl FontRegistry for MockFontRegistry {
    fn resolve(&mut self, req: &FontRequest) -> Result<FontId, FontLoadError> {
        let _ = req;
        Ok(FontId(0))
    }

    fn face(&self, id: FontId) -> &DecodedFace {
        let _ = id;
        &self.default_face
    }

    fn load_bundle(&mut self, bytes: &[u8]) -> Result<FontId, FontLoadError> {
        let _ = bytes;
        Ok(FontId(0))
    }

    fn fallback_chain(&self, id: FontId) -> &[FontId] {
        let _ = id;
        &[]
    }
}

/// **Test-only** mock [`GlyphAtlas`] with stub implementations of every
/// method.
///
/// Returns neutral/empty outputs for every operation — sufficient to compile
/// downstream consumers against the trait surface today. The real GPU-backed
/// LRU atlas lands in a later wave per §6.4; until then, calling any
/// [`GlyphAtlas`] method on this mock never panics.
///
/// - [`ensure`](GlyphAtlas::ensure) returns a zeroed [`AtlasSlot`] (page `0`,
///   default UV, zero bearing/size).
/// - [`slot`](GlyphAtlas::slot) always returns `None` (nothing resident).
/// - [`invalidate`](GlyphAtlas::invalidate) is a no-op.
/// - [`evict_lru`](GlyphAtlas::evict_lru) returns
///   [`EvictionStats::default()`] (nothing evicted).
///
/// The mock holds no state: every call returns the same constant output.
#[derive(Debug, Default, Clone, Copy)]
pub struct MockGlyphAtlas;

impl GlyphAtlas for MockGlyphAtlas {
    fn ensure(&mut self, key: GlyphKey) -> AtlasSlot {
        let _ = key;
        AtlasSlot {
            page: 0,
            uv: Rect::default(),
            bearing: (0.0, 0.0),
            size: (0.0, 0.0),
        }
    }

    fn slot(&self, key: GlyphKey) -> Option<AtlasSlot> {
        let _ = key;
        None
    }

    fn invalidate(&mut self, module_id: ModuleId, rect: DirtyRect) {
        let _ = (module_id, rect);
    }

    fn evict_lru(&mut self, keep: &PinSet) -> EvictionStats {
        let _ = keep;
        EvictionStats::default()
    }
}

/// **Test-only** mock [`TextStack`] with stub implementations of every
/// method.
///
/// Returns empty/minimal outputs for every operation — sufficient to compile
/// downstream consumers against the trait surface today. The default
/// HarfRust-backed shaper is [`HarfRustTextShaper`]; this mock is retained
/// for tests and as a non-panicking fallback for the `TextStack`-level
/// methods (`measure`, `rasterize`, `expose_a11y_text`, `ime_compose`) that
/// are not yet implemented by [`HarfRustTextShaper`].
///
/// The mock holds no state: every call returns the same constant output.
/// `ime_compose` always reports [`ImeState::Cancelled`].
#[derive(Debug, Default, Clone, Copy)]
pub struct MockTextStack;

impl TextShaper for MockTextStack {
    fn shape(&self, run: &str, ctx: &ShapeContext) -> Result<ShapedRun, ShapeError> {
        let _ = (run, ctx);
        Ok(empty_shaped_run())
    }

    fn reshape_with_font(&self, run: &str, font: FontId) -> Result<ShapedRun, ShapeError> {
        let _ = (run, font);
        Ok(empty_shaped_run())
    }
}

impl EditingOps for MockTextStack {
    fn hit_test(&self, run: &ShapedRun, point: (f32, f32)) -> CaretOffset {
        let _ = (run, point);
        CaretOffset::default()
    }

    fn caret_position(&self, run: &ShapedRun, offset: CaretOffset) -> (f32, f32) {
        let _ = (run, offset);
        (0.0, 0.0)
    }

    fn selection_quads(&self, run: &ShapedRun, sel: CaretSelection) -> Box<[Quad]> {
        let _ = (run, sel);
        Box::new([])
    }
}

impl TextStack for MockTextStack {
    fn measure(&self, run: &ShapedRun, max_width: f32) -> MeasuredLines {
        let _ = (run, max_width);
        MeasuredLines::default()
    }

    fn rasterize(&self, run: &ShapedRun, atlas: &mut dyn GlyphAtlas) -> GlyphQuadBatch {
        let _ = (run, atlas);
        GlyphQuadBatch::default()
    }

    fn expose_a11y_text(&self, run: &ShapedRun) -> A11yTextPlaceholder {
        let _ = run;
        A11yTextPlaceholder::default()
    }

    fn ime_compose(&mut self, ev: CompositionEvent) -> ImeState {
        let _ = ev;
        ImeState::Cancelled
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal embedded TTF — OpenSans variable subset (3196 bytes).
    /// Covers U+0065 ('e') and exercises GSUB/GPOS, fvar, etc. Used for
    /// both the registry `load_bundle` test and the shaper `shape` test.
    const TEST_FONT_TTF: &[u8] = include_bytes!(
        "../../../vendor/harfrust/harfrust/tests/fonts/rb_custom/OpenSans.subset1.ttf"
    );

    // -- HarfRust-backed tests ---------------------------------------------

    #[test]
    fn harfrust_registry_load_bundle_returns_ok() {
        let mut reg = HarfRustFontRegistry::new();
        assert!(reg.is_empty());
        let id = reg
            .load_bundle(TEST_FONT_TTF)
            .expect("load_bundle must be Ok for a valid TTF");
        assert_eq!(id, FontId(0));
        assert_eq!(reg.len(), 1);
        assert!(!reg.is_empty());

        // The face should carry real metrics from head/hhea.
        let face = reg.face(id);
        assert_eq!(face.id, id);
        // OpenSans subset has 2048 units per em.
        assert_eq!(face.units_per_em, 2048);
        // Ascender should be positive, descender negative.
        assert!(face.ascender > 0, "ascender should be positive");
        assert!(face.descender < 0, "descender should be negative");
    }

    #[test]
    fn harfrust_registry_load_bundle_rejects_garbage() {
        let mut reg = HarfRustFontRegistry::new();
        let result = reg.load_bundle(b"not a font");
        assert!(
            matches!(result, Err(FontLoadError::TableDecodeFailed { .. })),
            "garbage bytes should produce TableDecodeFailed, got {:?}",
            result
        );
        assert!(reg.is_empty(), "registry should remain empty on failure");
    }

    #[test]
    fn harfrust_registry_resolve_returns_first_font() {
        let mut reg = HarfRustFontRegistry::new();
        // Empty registry → RegistryEmpty.
        assert!(matches!(
            reg.resolve(&FontRequest::default()),
            Err(FontLoadError::RegistryEmpty)
        ));
        // Load a font → resolve returns FontId(0).
        reg.load_bundle(TEST_FONT_TTF).unwrap();
        let id = reg.resolve(&FontRequest::default()).expect("resolve must be Ok");
        assert_eq!(id, FontId(0));
    }

    #[test]
    fn harfrust_registry_fallback_chain_is_empty() {
        let mut reg = HarfRustFontRegistry::new();
        let id = reg.load_bundle(TEST_FONT_TTF).unwrap();
        assert!(reg.fallback_chain(id).is_empty());
    }

    #[test]
    fn harfrust_shape_returns_non_empty_run() {
        let mut reg = HarfRustFontRegistry::new();
        let font_id = reg.load_bundle(TEST_FONT_TTF).expect("load_bundle must be Ok");

        let shaper = HarfRustTextShaper::new(Arc::new(reg));
        let ctx = ShapeContext {
            font: font_id,
            size_px: 16.0,
            direction: Some(Direction::Ltr),
        };
        // The subset font covers U+0065 ('e').
        let run = shaper.shape("e", &ctx).expect("shape must be Ok");

        assert!(
            !run.glyph_ids.is_empty(),
            "glyph_ids must be non-empty for a covered codepoint"
        );
        assert!(
            run.metrics.total_advance > 0.0,
            "total_advance must be non-zero, got {}",
            run.metrics.total_advance
        );
        assert_eq!(run.font_id, font_id);
        assert_eq!(run.direction, Direction::Ltr);
        assert_eq!(run.bidi_level, 0);
        // Parallel arrays must all have the same length.
        assert_eq!(run.glyph_ids.len(), run.advances.len());
        assert_eq!(run.advances.len(), run.offsets.len());
        assert_eq!(run.offsets.len(), run.clusters.len());
        // Caret map: N+1 carets for N glyphs.
        assert_eq!(run.caret_map.glyph_to_cluster.len(), run.glyph_ids.len());
        assert_eq!(
            run.caret_map.caret_to_glyph.len(),
            run.glyph_ids.len() + 1
        );
        // Ascent should be positive at non-zero size.
        assert!(
            run.metrics.ascent > 0.0,
            "ascent should be positive, got {}",
            run.metrics.ascent
        );
    }

    #[test]
    fn harfrust_shape_auto_detects_direction() {
        let mut reg = HarfRustFontRegistry::new();
        let font_id = reg.load_bundle(TEST_FONT_TTF).unwrap();
        let shaper = HarfRustTextShaper::new(Arc::new(reg));

        // direction = None → HarfRust auto-detects (Latin → LTR).
        let ctx = ShapeContext {
            font: font_id,
            size_px: 16.0,
            direction: None,
        };
        let run = shaper.shape("e", &ctx).expect("shape must be Ok");
        assert_eq!(run.direction, Direction::Ltr);
        assert_eq!(run.bidi_level, 0);
    }

    #[test]
    fn harfrust_shape_font_unresolved() {
        let mut reg = HarfRustFontRegistry::new();
        reg.load_bundle(TEST_FONT_TTF).unwrap();
        let shaper = HarfRustTextShaper::new(Arc::new(reg));

        let ctx = ShapeContext {
            font: FontId(999), // not registered
            size_px: 16.0,
            direction: Some(Direction::Ltr),
        };
        assert!(matches!(
            shaper.shape("e", &ctx),
            Err(ShapeError::FontUnresolved)
        ));
    }

    #[test]
    fn harfrust_reshape_with_font_uses_default_size() {
        let mut reg = HarfRustFontRegistry::new();
        let font_id = reg.load_bundle(TEST_FONT_TTF).unwrap();
        let shaper = HarfRustTextShaper::new(Arc::new(reg));

        let run = shaper
            .reshape_with_font("e", font_id)
            .expect("reshape_with_font must be Ok");
        assert!(!run.glyph_ids.is_empty());
        assert!(run.metrics.total_advance > 0.0);
        assert_eq!(run.font_id, font_id);
    }

    #[test]
    fn harfrust_shape_empty_string() {
        let mut reg = HarfRustFontRegistry::new();
        let font_id = reg.load_bundle(TEST_FONT_TTF).unwrap();
        let shaper = HarfRustTextShaper::new(Arc::new(reg));

        let ctx = ShapeContext {
            font: font_id,
            size_px: 16.0,
            direction: Some(Direction::Ltr),
        };
        let run = shaper.shape("", &ctx).expect("shape must be Ok");
        assert!(run.glyph_ids.is_empty());
        assert_eq!(run.metrics.total_advance, 0.0);
        // Even an empty run carries font metrics.
        assert!(run.metrics.ascent > 0.0);
    }

    #[test]
    fn harfrust_registry_multiple_fonts() {
        let mut reg = HarfRustFontRegistry::new();
        let id0 = reg.load_bundle(TEST_FONT_TTF).unwrap();
        let id1 = reg.load_bundle(TEST_FONT_TTF).unwrap();
        assert_eq!(id0, FontId(0));
        assert_eq!(id1, FontId(1));
        assert_eq!(reg.len(), 2);

        // resolve still returns the first font (Wave 1 behavior).
        let resolved = reg.resolve(&FontRequest::default()).unwrap();
        assert_eq!(resolved, FontId(0));

        // Both faces should be accessible.
        assert_eq!(reg.face(id0).id, id0);
        assert_eq!(reg.face(id1).id, id1);
    }

    // -- Mock tests (retained from Wave 3) ---------------------------------

    #[test]
    fn mock_shape_returns_ok_with_empty_run() {
        let stack = MockTextStack;
        let ctx = ShapeContext::default();
        let run = stack.shape("hello", &ctx).expect("shape must be Ok");
        assert!(run.glyph_ids.is_empty());
        assert!(run.advances.is_empty());
        assert!(run.offsets.is_empty());
        assert!(run.clusters.is_empty());
        assert_eq!(run.font_id, FontId(0));
        assert_eq!(run.direction, Direction::Ltr);
        assert_eq!(run.bidi_level, 0);
        assert_eq!(run.metrics, RunMetrics::default());
        assert!(run.caret_map.glyph_to_cluster.is_empty());
        assert!(run.caret_map.caret_to_glyph.is_empty());
    }

    #[test]
    fn mock_reshape_with_font_returns_empty_run() {
        let stack = MockTextStack;
        let run = stack
            .reshape_with_font("x", FontId(42))
            .expect("reshape must be Ok");
        // Mock ignores the requested font and returns the FontId(0) default.
        assert_eq!(run.font_id, FontId(0));
        assert!(run.glyph_ids.is_empty());
    }

    #[test]
    fn mock_measure_returns_default() {
        let stack = MockTextStack;
        let shaped = stack.shape("", &ShapeContext::default()).unwrap();
        let measured = stack.measure(&shaped, 1024.0);
        assert!(measured.line_advances.is_empty());
        assert!(measured.line_ranges.is_empty());
        assert_eq!(measured.metrics, RunMetrics::default());
    }

    #[test]
    fn mock_rasterize_returns_default_batch() {
        let stack = MockTextStack;
        let shaped = stack.shape("", &ShapeContext::default()).unwrap();
        // The mock atlas has non-panicking stubs, but `rasterize` emits zero
        // quads without ever querying it.
        let mut atlas = MockGlyphAtlas;
        let batch = stack.rasterize(&shaped, &mut atlas);
        assert!(batch.quads.is_empty());
        assert!(batch.font_ids.is_empty());
    }

    #[test]
    fn mock_expose_a11y_text_returns_default() {
        let stack = MockTextStack;
        let shaped = stack.shape("", &ShapeContext::default()).unwrap();
        let a11y = stack.expose_a11y_text(&shaped);
        assert!(a11y.text.is_empty());
        assert!(a11y.selection.is_none());
        assert_eq!(a11y.metrics, RunMetrics::default());
        assert!(a11y.label.is_none());
    }

    #[test]
    fn mock_ime_compose_returns_cancelled() {
        let mut stack = MockTextStack;
        let ev = CompositionEvent {
            text: String::from("a"),
            caret: 0,
            replace_range: (0, 0),
        };
        assert_eq!(stack.ime_compose(ev), ImeState::Cancelled);
    }

    #[test]
    fn mock_editing_ops_return_neutral_defaults() {
        let stack = MockTextStack;
        let shaped = stack.shape("", &ShapeContext::default()).unwrap();
        assert_eq!(
            stack.hit_test(&shaped, (10.0, 10.0)),
            CaretOffset::default()
        );
        assert_eq!(
            stack.caret_position(&shaped, CaretOffset::default()),
            (0.0, 0.0)
        );
        assert!(stack
            .selection_quads(&shaped, CaretSelection::default())
            .is_empty());
    }

    #[test]
    fn mock_font_registry_returns_neutral_defaults() {
        let mut reg = MockFontRegistry::default();
        let req = FontRequest::default();
        assert_eq!(reg.resolve(&req).unwrap(), FontId(0));
        assert_eq!(reg.load_bundle(&[]).unwrap(), FontId(0));
        let face = reg.face(FontId(0));
        assert_eq!(face.id, FontId(0));
        assert_eq!(face.units_per_em, 0);
        assert!(reg.fallback_chain(FontId(0)).is_empty());
    }

    #[test]
    fn mock_glyph_atlas_returns_non_panicking_defaults() {
        let mut atlas = MockGlyphAtlas;
        let slot = atlas.ensure(GlyphKey::default());
        assert_eq!(slot.page, 0);
        assert_eq!(slot.uv, Rect::default());
        assert_eq!(slot.bearing, (0.0, 0.0));
        assert_eq!(slot.size, (0.0, 0.0));
        assert_eq!(atlas.slot(GlyphKey::default()), None);
        atlas.invalidate(ModuleId(0), DirtyRect);
        assert_eq!(
            atlas.evict_lru(&PinSet::default()),
            EvictionStats::default()
        );
    }
}
