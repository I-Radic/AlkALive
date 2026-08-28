//! Text rendering stack — forked in-WASM HarfRust (ADR 022).
//!
//! Trait surface with **required** methods (no `todo!()` defaults). Wave 1
//! ships the default HarfRust-backed implementations: [`HarfRustFontRegistry`]
//! (§6.2) and [`HarfRustTextShaper`] (§6.3). Wave 2 adds the default
//! [`HarfRustGlyphAtlas`] (§6.4) — a real glyph atlas that rasterises glyph
//! outlines into a CPU-side texture atlas via the vendored `rasterizer`
//! crate. The remaining trait surface (editing ops, `TextStack`
//! measure/rasterize/a11y/ime adapters per §6.5–6.9) still uses
//! [`MockTextStack`] as a non-panicking stub; [`MockGlyphAtlas`] is retained
//! as a test-only fallback.
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
use rasterizer::Rasterizer;
use read_fonts::model::pen::{ControlBoundsPen, OutlinePen, PathElement};
use read_fonts::tables::glyf::{CurvePoint, Glyph};
use read_fonts::types::{GlyphId, NameId};
use read_fonts::TableProvider;
use std::collections::HashMap;
use std::sync::Arc;

// ============================================================================
// Security limits (SEC-03 / SEC-04)
// ============================================================================

/// Hard upper bound on a single font bundle's byte length accepted by
/// [`HarfRustFontRegistry::load_bundle`] (SEC-03).
///
/// 50 MiB is well above any realistic single-face OpenType bundle (the
/// largest legitimate fonts — e.g. CJK mega-families — are a few MiB),
/// while capping unbounded WASM-heap allocations from untrusted input.
/// Inputs exceeding this are rejected with
/// [`FontLoadError::TableDecodeFailed`] carrying a synthetic `SIZE` tag
/// before any parser touches the bytes.
pub const MAX_FONT_SIZE: usize = 50 * 1024 * 1024;

/// Hard upper bound on the byte length of a text run accepted by
/// [`HarfRustTextShaper::shape`] (SEC-04).
///
/// 1 MiB is far above any plausible single shaped run (a full novel is
/// ~1–2 MiB of UTF-8), while preventing resource exhaustion in HarfRust's
/// shaping buffer. Inputs exceeding this are rejected with
/// [`ShapeError::InvalidUtf8`] (reused — there is no `TooLong` variant and
/// adding one would change the public enum ABI) before shaping begins.
pub const MAX_TEXT_LENGTH: usize = 1024 * 1024;

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
    /// Family name extracted from the font's `name` table (Gap #8).
    ///
    /// Prefers the Typographic Family Name (name ID 16) and falls back to
    /// the legacy Family Name (name ID 1); defaults to `"Unknown"` when
    /// neither is decodable. Used by
    /// [`resolve`](HarfRustFontRegistry::resolve) for family matching.
    family: String,
    /// Weight class extracted from the font's `OS/2` table (Gap #8).
    ///
    /// CSS-scale value (100–900); defaults to `400` (regular) when the
    /// `OS/2` table is missing or undecodable. Used by
    /// [`resolve`](HarfRustFontRegistry::resolve) for weight matching.
    weight: u16,
}

/// HarfRust-backed [`FontRegistry`] — the default implementation (ADR 022).
///
/// Stores parsed OpenType tables on the WASM heap via [`harfrust::font::Font`]
/// (internally `Arc`-reference-counted, `Send + Sync`). Each [`FontId`] maps
/// to a single face entry. Wave 4 (Gap #8) closes the family/weight matching
/// loop: each loaded entry now carries its family name (extracted from the
/// `name` table) and weight (extracted from the `OS/2` table), and
/// [`resolve`](FontRegistry::resolve) follows the cascade
/// exact-family+exact-weight → exact-family+nearest-weight-within-±100 →
/// generic alias (`serif`/`sans`/`mono`) → `FamilyNotFound`.
/// [`fallback_chain`](FontRegistry::fallback_chain) still always returns
/// `&[]`; real fallback chains land with the font-config integration.
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

    /// Returns the `(family, weight)` extracted at load time for the font
    /// at `id`, or `None` if no such font is registered (Gap #8). Useful for
    /// callers that need to construct a matching [`FontRequest`] against an
    /// already-loaded face.
    pub fn family_and_weight(&self, id: FontId) -> Option<(&str, u16)> {
        self.entries
            .iter()
            .find(|e| e.id == id)
            .map(|e| (e.family.as_str(), e.weight))
    }
}

impl FontRegistry for HarfRustFontRegistry {
    fn resolve(&mut self, req: &FontRequest) -> Result<FontId, FontLoadError> {
        // Empty registry → RegistryEmpty (preserved Wave 1 behaviour for
        // the "nothing loaded yet" path; distinct from FamilyNotFound which
        // indicates a populated registry with no match).
        if self.entries.is_empty() {
            return Err(FontLoadError::RegistryEmpty);
        }

        // A blank requested family never matches a loaded family — skip
        // straight to alias matching / FamilyNotFound.
        let family_matches: Vec<&HarfRustFontEntry> = if req.family.is_empty() {
            Vec::new()
        } else {
            self.entries
                .iter()
                .filter(|e| e.family.eq_ignore_ascii_case(&req.family))
                .collect()
        };

        if !family_matches.is_empty() {
            // a. Exact family + exact weight.
            if let Some(entry) = family_matches.iter().find(|e| e.weight == req.weight) {
                return Ok(entry.id);
            }
            // b. Exact family + nearest weight within ±100 (Gap #8).
            let nearest = family_matches
                .iter()
                .filter(|e| (e.weight as i32 - req.weight as i32).abs() <= 100)
                .min_by_key(|e| (e.weight as i32 - req.weight as i32).abs());
            if let Some(entry) = nearest {
                return Ok(entry.id);
            }
            // Family matched but no acceptable weight — fall through to
            // FamilyNotFound. (Wave 4: WeightUnavailable is intentionally
            // not surfaced here; the spec's fallback chain collapses a
            // weight miss into the same terminal FamilyNotFound path so
            // callers handle a single miss code.)
        } else {
            // c. No family match — try generic alias mapping (Gap #8):
            //   "serif"       → first font with "serif" in its family name
            //   "sans-serif"  → first font with "sans" in its family name
            //   "sans"        → (same as "sans-serif")
            //   "monospace"   → first font with "mono" in its family name
            //   "mono"        → (same as "monospace")
            // Matching is case-insensitive on both the request and the
            // loaded family name.
            let req_lower = req.family.to_ascii_lowercase();
            let alias_needle: Option<&str> = match req_lower.as_str() {
                "serif" => Some("serif"),
                "sans-serif" | "sans" => Some("sans"),
                "monospace" | "mono" => Some("mono"),
                _ => None,
            };
            if let Some(needle) = alias_needle {
                if let Some(entry) = self
                    .entries
                    .iter()
                    .find(|e| e.family.to_ascii_lowercase().contains(needle))
                {
                    return Ok(entry.id);
                }
            }
        }

        // d. No match across the cascade.
        Err(FontLoadError::FamilyNotFound)
    }

    fn face(&self, id: FontId) -> &DecodedFace {
        self.entries
            .iter()
            .find(|e| e.id == id)
            .map(|e| &e.face)
            .expect("HarfRustFontRegistry::face: FontId not registered")
    }

    fn load_bundle(&mut self, bytes: &[u8]) -> Result<FontId, FontLoadError> {
        // SEC-03: reject oversized bundles before parsing to cap WASM-heap
        // allocation from untrusted input. The synthetic `SIZE` tag makes
        // the rejection distinguishable from a genuine `sfnt` parse failure
        // in downstream diagnostics.
        if bytes.len() > MAX_FONT_SIZE {
            return Err(FontLoadError::TableDecodeFailed {
                font_id: FontId::default(),
                table: Tag(*b"SIZE"),
            });
        }
        let font = Font::new(bytes.to_vec(), 0).map_err(|_| FontLoadError::TableDecodeFailed {
            font_id: FontId::default(),
            table: Tag(*b"sfnt"),
        })?;
        let id = FontId(self.entries.len() as u32);
        let face = decode_face(&font, id);
        let (family, weight) = extract_family_and_weight(&font);
        self.entries.push(HarfRustFontEntry {
            id,
            font,
            face,
            family,
            weight,
        });
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

/// Extract the family name and weight class from a [`Font`]'s `name` and
/// `OS/2` tables (Gap #8).
///
/// Family extraction prefers the Typographic Family Name (name ID 16) and
/// falls back to the legacy Family Name (name ID 1); only Unicode-encoded
/// records are read. Weight is read from `OS/2::us_weight_class`. Any
/// failure (missing table, undecodable string, empty string) falls back to
/// the documented defaults of `"Unknown"` / `400`.
fn extract_family_and_weight(font: &Font) -> (String, u16) {
    let tables = font.tables();

    // Family: prefer Typographic Family Name (ID 16) over legacy Family
    // Name (ID 1). Only Unicode-platform name records are decoded.
    let family = tables
        .name()
        .ok()
        .and_then(|name_table| {
            let storage = name_table.string_data();
            let records = name_table.name_record();
            for &target_id in &[NameId::TYPOGRAPHIC_FAMILY_NAME, NameId::FAMILY_NAME] {
                for record in records.iter() {
                    if record.name_id() == target_id && record.is_unicode() {
                        if let Ok(name_string) = record.string(storage) {
                            let collected: String = name_string.chars().collect();
                            if !collected.is_empty() {
                                return Some(collected);
                            }
                        }
                    }
                }
            }
            None
        })
        .unwrap_or_else(|| "Unknown".to_string());

    // Weight: OS/2::us_weight_class (CSS scale 100–900).
    let weight = tables
        .os2()
        .ok()
        .map(|os2| os2.us_weight_class())
        .unwrap_or(400);

    (family, weight)
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
        // SEC-04: reject oversized text runs before shaping to prevent
        // resource exhaustion in HarfRust's shaping buffer. `InvalidUtf8`
        // is reused (there is no `TooLong` variant and adding one would
        // change the public enum ABI); the rejection fires on byte length,
        // so any valid UTF-8 ≤ MAX_TEXT_LENGTH still shapes normally.
        if run.len() > MAX_TEXT_LENGTH {
            return Err(ShapeError::InvalidUtf8);
        }
        shape_run(&self.registry, run, ctx.font, ctx.size_px, ctx.direction)
    }

    fn reshape_with_font(&self, run: &str, font: FontId) -> Result<ShapedRun, ShapeError> {
        shape_run(&self.registry, run, font, DEFAULT_SHAPE_SIZE_PX, None)
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
    let bidi_level: u8 = if out_direction == Direction::Rtl {
        1
    } else {
        0
    };

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
// HarfRust glyph atlas (Wave 2 — ADR 022 §6.4)
// ============================================================================

/// Side length of a square atlas page in pixels (§6.4).
const ATLAS_SIZE: usize = 512;

/// One-pixel padding between packed glyphs to prevent texture bleeding
/// when the compositor bilinearly filters the atlas.
const ATLAS_PADDING: usize = 1;

/// HarfRust-backed [`GlyphAtlas`] — the default implementation (ADR 022).
///
/// Rasterises glyph outlines from the registered [`HarfRustFontRegistry`]
/// into a CPU-side grayscale texture atlas using the vendored
/// [`Rasterizer`]. Slots are addressed by [`GlyphKey`]; a miss triggers
/// outline extraction (via `read_fonts`' `glyf`/`loca` tables), scaling,
/// rasterisation, and shelf-packing into the current atlas page.
///
/// Wave 2 limitations (per task WAVE-W2):
/// - [`evict_lru`](GlyphAtlas::evict_lru) is a no-op returning
///   [`EvictionStats::default()`]; real LRU eviction lands in a later wave.
/// - [`invalidate`](GlyphAtlas::invalidate) clears the entire cache and
///   resets the packer; per-module dirty-rect locality lands in a later wave.
/// - Composite glyphs are not recursively outlined; they fall back to a
///   filled rectangle from the `glyf` header bbox (or a zero-size slot if
///   the header bbox is degenerate).
/// - Anti-aliasing is 4x vertical sub-sampling with horizontal coverage;
///   no hinting, no subpixel positioning.
pub struct HarfRustGlyphAtlas {
    /// Read-only share of the font registry.
    registry: Arc<HarfRustFontRegistry>,
    /// Cached glyph slots, keyed by [`GlyphKey`].
    cache: HashMap<GlyphKey, AtlasSlot>,
    /// Atlas texture pages (each `ATLAS_SIZE x ATLAS_SIZE` grayscale).
    pages: Vec<Vec<u8>>,
    /// Shelf-pack cursor: x position within the current row.
    pack_x: usize,
    /// Shelf-pack cursor: y position of the current row's top.
    pack_y: usize,
    /// Height of the current shelf (max glyph height in the row).
    pack_row_height: usize,
}

impl HarfRustGlyphAtlas {
    /// Create a new atlas backed by `registry`. The registry should have
    /// all fonts loaded before the atlas is constructed (the atlas takes
    /// a read-only [`Arc`] share, matching [`HarfRustTextShaper::new`]).
    pub fn new(registry: Arc<HarfRustFontRegistry>) -> Self {
        let pages = vec![vec![0u8; ATLAS_SIZE * ATLAS_SIZE]];
        Self {
            registry,
            cache: HashMap::new(),
            pages,
            pack_x: 0,
            pack_y: 0,
            pack_row_height: 0,
        }
    }

    /// Number of allocated atlas pages.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Read-only access to the raw pixel data of atlas page `idx`.
    ///
    /// Returns `None` if `idx` is out of range. Each page is a flat
    /// `ATLAS_SIZE * ATLAS_SIZE` byte buffer of grayscale alpha values.
    pub fn page_data(&self, idx: usize) -> Option<&[u8]> {
        self.pages.get(idx).map(Vec::as_slice)
    }

    /// Number of cached glyph slots.
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Returns `true` if no glyphs are cached.
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Clear every cached glyph slot and reset the shelf packer to a
    /// single fresh page.
    ///
    /// This is the atlas-clear mechanism the persistent (long-lived)
    /// renderer atlas uses as its safety valve: when a long typing
    /// session accumulates more distinct glyphs than fit one page — or on
    /// a scene transition — the caller resets the atlas and re-rasterizes
    /// only the glyphs the current frame needs. Semantically identical to
    /// [`GlyphAtlas::invalidate`] (which delegates here), but callable
    /// without constructing a [`ModuleId`] / [`DirtyRect`] pair.
    pub fn reset(&mut self) {
        self.cache.clear();
        self.reset_pages();
    }

    /// Shelf-pack a `(w x h)` rectangle into the current atlas page,
    /// allocating a new page if necessary. Returns `(page, x, y)`.
    fn pack(&mut self, w: usize, h: usize) -> (u16, usize, usize) {
        if w == 0 || h == 0 {
            // Degenerate rectangles get a zero-size slot at the origin of
            // the current page; no space is consumed.
            return ((self.pages.len() - 1) as u16, 0, 0);
        }
        let needed_w = w + ATLAS_PADDING;
        let needed_h = h + ATLAS_PADDING;
        loop {
            // Wrap to the next shelf if the current row is full.
            if self.pack_x + needed_w > ATLAS_SIZE {
                self.pack_y += self.pack_row_height;
                self.pack_x = 0;
                self.pack_row_height = 0;
            }
            // Allocate a new page if the current one is full.
            if self.pack_y + needed_h > ATLAS_SIZE {
                self.pages.push(vec![0u8; ATLAS_SIZE * ATLAS_SIZE]);
                self.pack_x = 0;
                self.pack_y = 0;
                self.pack_row_height = 0;
                continue;
            }
            let page_idx = (self.pages.len() - 1) as u16;
            let x = self.pack_x;
            let y = self.pack_y;
            self.pack_x += needed_w;
            self.pack_row_height = self.pack_row_height.max(needed_h);
            return (page_idx, x, y);
        }
    }

    /// Reset the shelf packer and clear all atlas pages back to a single
    /// fresh page.
    fn reset_pages(&mut self) {
        self.pages.clear();
        self.pages.push(vec![0u8; ATLAS_SIZE * ATLAS_SIZE]);
        self.pack_x = 0;
        self.pack_y = 0;
        self.pack_row_height = 0;
    }

    /// Rasterise `key` into the atlas. Returns `None` if the glyph has no
    /// outline (e.g. space) or the font is unavailable; the caller maps
    /// `None` to a zero-size [`AtlasSlot`].
    fn rasterize_glyph(&mut self, key: &GlyphKey) -> Option<AtlasSlot> {
        let font = self.registry.font(key.font_id)?;
        let tables = font.tables();

        let head = tables.head().ok()?;
        let units_per_em = head.units_per_em();
        if units_per_em == 0 {
            return None;
        }
        let scale = key.size_px as f32 / units_per_em as f32;

        let glyf = tables.glyf().ok()?;
        // `loca(None)` auto-detects the index-to-loc-format from `head`.
        let loca = tables.loca(None).ok()?;
        let glyph = loca.get_glyf(GlyphId::new(key.glyph_id), &glyf).ok()??;

        // Collect the scaled outline into a path buffer.
        let mut path: Vec<PathElement> = Vec::new();
        draw_glyph_outline(&glyph, scale, &mut path);

        // Compute the bounding box in scaled pixel coords.
        let (x_min, y_min, x_max, y_max) = if path.is_empty() {
            // Fallback for empty paths (e.g. composite glyphs): use the
            // glyf header bbox and rasterise a filled rectangle.
            let hdr_x_min = glyph.x_min() as f32 * scale;
            let hdr_y_min = glyph.y_min() as f32 * scale;
            let hdr_x_max = glyph.x_max() as f32 * scale;
            let hdr_y_max = glyph.y_max() as f32 * scale;
            if hdr_x_max <= hdr_x_min || hdr_y_max <= hdr_y_min {
                return None;
            }
            (hdr_x_min, hdr_y_min, hdr_x_max, hdr_y_max)
        } else {
            // Use ControlBoundsPen to compute the control-point bbox.
            let mut bounds_pen = ControlBoundsPen::new();
            replay_path(&path, (0.0, 0.0), &mut bounds_pen);
            let bbox = bounds_pen.bounding_box()?;
            (bbox.x_min, bbox.y_min, bbox.x_max, bbox.y_max)
        };

        // Compute bitmap dimensions with floor/ceil margins so the entire
        // outline fits.
        let x_min_floor = x_min.floor();
        let y_min_floor = y_min.floor();
        let x_max_ceil = x_max.ceil();
        let y_max_ceil = y_max.ceil();
        let width = (x_max_ceil - x_min_floor).max(0.0) as usize;
        let height = (y_max_ceil - y_min_floor).max(0.0) as usize;
        if width == 0 || height == 0 {
            return None;
        }

        // Rasterise the outline (or fallback rectangle) into the bitmap.
        let mut rast = Rasterizer::new(width, height);
        if path.is_empty() {
            // Filled rectangle fallback for composite glyphs.
            rast.move_to(0.0, 0.0);
            rast.line_to(width as f32, 0.0);
            rast.line_to(width as f32, height as f32);
            rast.line_to(0.0, height as f32);
            rast.close();
        } else {
            // Replay the path with X offset (-x_min_floor) and Y flip
            // (bitmap_y = y_max_ceil - path_y) so font-space Y-up
            // coordinates become bitmap-space Y-down coordinates.
            replay_path_y_flipped(&path, -x_min_floor, y_max_ceil, &mut rast);
        }
        let bitmap = rast.rasterize();

        // Shelf-pack the bitmap into the atlas.
        let (page, px, py) = self.pack(width, height);
        // Copy the bitmap into the atlas page (clipped to page bounds).
        let page_buf = &mut self.pages[page as usize];
        for row in 0..height {
            let dy = py + row;
            if dy >= ATLAS_SIZE {
                break;
            }
            let dst_row = dy * ATLAS_SIZE;
            let src_row = row * width;
            for col in 0..width {
                let dx = px + col;
                if dx >= ATLAS_SIZE {
                    break;
                }
                page_buf[dst_row + dx] = bitmap[src_row + col];
            }
        }

        let atlas_f = ATLAS_SIZE as f32;
        let uv = Rect {
            x: px as f32 / atlas_f,
            y: py as f32 / atlas_f,
            w: width as f32 / atlas_f,
            h: height as f32 / atlas_f,
        };
        // Bearing in scaled pixel coords (Y-up, FreeType convention):
        // bearing.0 = horizontal offset from pen to bitmap left edge.
        // bearing.1 = vertical offset from baseline to bitmap top (up = +).
        let bearing = (x_min_floor, y_max_ceil);
        let size = (width as f32, height as f32);

        Some(AtlasSlot {
            page,
            uv,
            bearing,
            size,
        })
    }
}

impl GlyphAtlas for HarfRustGlyphAtlas {
    fn ensure(&mut self, key: GlyphKey) -> AtlasSlot {
        if let Some(&slot) = self.cache.get(&key) {
            return slot;
        }
        let slot = self.rasterize_glyph(&key).unwrap_or_else(zero_slot);
        self.cache.insert(key, slot);
        slot
    }

    fn slot(&self, key: GlyphKey) -> Option<AtlasSlot> {
        self.cache.get(&key).copied()
    }

    fn invalidate(&mut self, _module_id: ModuleId, _rect: DirtyRect) {
        // Wave 2: simplified — clear the entire cache and reset the packer.
        // Per-module dirty-rect locality lands in a later wave.
        self.reset();
    }

    fn evict_lru(&mut self, _keep: &PinSet) -> EvictionStats {
        // Wave 2: eviction is a no-op; real LRU eviction lands later.
        EvictionStats::default()
    }
}

/// Construct a zero-size [`AtlasSlot`] for glyphs with no outline
/// (e.g. space) or when the font is unavailable.
fn zero_slot() -> AtlasSlot {
    AtlasSlot {
        page: 0,
        uv: Rect::default(),
        bearing: (0.0, 0.0),
        size: (0.0, 0.0),
    }
}

/// Draw a glyph's outline (scaled by `scale`) into `pen` as path commands.
///
/// For [`Glyph::Simple`], iterates each contour and emits `move_to`/
/// `line_to`/`quad_to`/`close` commands, handling TrueType's implicit
/// on-curve point convention (consecutive off-curve points have an
/// implicit on-curve point at their midpoint).
///
/// For [`Glyph::Composite`], nothing is emitted (Wave 2 limitation; the
/// atlas falls back to a filled rectangle from the `glyf` header bbox).
fn draw_glyph_outline<P: OutlinePen>(glyph: &Glyph<'_>, scale: f32, pen: &mut P) {
    match glyph {
        Glyph::Simple(simple) => {
            let end_pts = simple.end_pts_of_contours();
            if end_pts.is_empty() {
                return;
            }
            let points: Vec<CurvePoint> = simple.points().collect();
            if points.is_empty() {
                return;
            }
            let mut start = 0usize;
            for end_be in end_pts {
                let end = end_be.get() as usize;
                if end >= points.len() {
                    break;
                }
                draw_contour(&points[start..=end], scale, pen);
                start = end + 1;
            }
        }
        Glyph::Composite(_) => {
            // Wave 2: composite glyphs not outlined; caller falls back to
            // a filled rectangle from the glyf header bbox.
        }
    }
}

/// Draw a single TrueType contour (a closed sequence of [`CurvePoint`]s)
/// into `pen`, scaled by `scale`.
///
/// Handles the three TrueType contour start cases:
/// 1. First point on-curve: start there with `move_to`.
/// 2. First point off-curve, some later point on-curve: rotate the contour
///    so the on-curve point is first.
/// 3. All points off-curve: insert an implicit on-curve point at the
///    midpoint of the last and first points, then emit a quad for each
///    point with the midpoint to the next as the endpoint.
fn draw_contour<P: OutlinePen>(points: &[CurvePoint], scale: f32, pen: &mut P) {
    let n = points.len();
    if n == 0 {
        return;
    }

    // Find the first on-curve point to start the contour from.
    let first_on = (0..n).find(|&i| points[i].on_curve);

    let start_idx = match first_on {
        Some(i) => i,
        None => {
            // All off-curve: emit an implicit on-curve point at the
            // midpoint of the last and first points, then a quad for each
            // point with the midpoint to the next as the endpoint.
            let first = &points[0];
            let last = &points[n - 1];
            let mid_x = (last.x as f32 + first.x as f32) * 0.5;
            let mid_y = (last.y as f32 + first.y as f32) * 0.5;
            pen.move_to(mid_x * scale, mid_y * scale);
            for i in 0..n {
                let cur = &points[i];
                let next = &points[(i + 1) % n];
                let nmid_x = (cur.x as f32 + next.x as f32) * 0.5;
                let nmid_y = (cur.y as f32 + next.y as f32) * 0.5;
                pen.quad_to(
                    cur.x as f32 * scale,
                    cur.y as f32 * scale,
                    nmid_x * scale,
                    nmid_y * scale,
                );
            }
            pen.close();
            return;
        }
    };

    let start_pt = &points[start_idx];
    pen.move_to(start_pt.x as f32 * scale, start_pt.y as f32 * scale);

    let mut prev_off: Option<CurvePoint> = None;
    let mut i = (start_idx + 1) % n;
    while i != start_idx {
        let p = &points[i];
        if p.on_curve {
            match prev_off {
                Some(off) => {
                    pen.quad_to(
                        off.x as f32 * scale,
                        off.y as f32 * scale,
                        p.x as f32 * scale,
                        p.y as f32 * scale,
                    );
                    prev_off = None;
                }
                None => {
                    pen.line_to(p.x as f32 * scale, p.y as f32 * scale);
                }
            }
        } else if let Some(off) = prev_off {
            // Two consecutive off-curve points: emit quad with implicit
            // on-curve midpoint.
            let mid_x = (off.x as f32 + p.x as f32) * 0.5;
            let mid_y = (off.y as f32 + p.y as f32) * 0.5;
            pen.quad_to(
                off.x as f32 * scale,
                off.y as f32 * scale,
                mid_x * scale,
                mid_y * scale,
            );
            prev_off = Some(*p);
        } else {
            prev_off = Some(*p);
        }
        i = (i + 1) % n;
    }

    // Close the contour back to the start, handling any trailing
    // off-curve point.
    if let Some(off) = prev_off {
        pen.quad_to(
            off.x as f32 * scale,
            off.y as f32 * scale,
            start_pt.x as f32 * scale,
            start_pt.y as f32 * scale,
        );
    }
    pen.close();
}

/// Replay a recorded path into any [`OutlinePen`], applying a flat
/// `(ox, oy)` offset to every coordinate.
fn replay_path<P: OutlinePen>(path: &[PathElement], offset: (f32, f32), pen: &mut P) {
    for el in path {
        match *el {
            PathElement::MoveTo { x, y } => pen.move_to(x + offset.0, y + offset.1),
            PathElement::LineTo { x, y } => pen.line_to(x + offset.0, y + offset.1),
            PathElement::QuadTo { cx0, cy0, x, y } => {
                pen.quad_to(cx0 + offset.0, cy0 + offset.1, x + offset.0, y + offset.1)
            }
            PathElement::CurveTo {
                cx0,
                cy0,
                cx1,
                cy1,
                x,
                y,
            } => pen.curve_to(
                cx0 + offset.0,
                cy0 + offset.1,
                cx1 + offset.0,
                cy1 + offset.1,
                x + offset.0,
                y + offset.1,
            ),
            PathElement::Close => pen.close(),
        }
    }
}

/// Replay a recorded path into a [`Rasterizer`], applying an X offset and
/// a Y flip (`bitmap_y = y_flip - path_y`) so font-space Y-up coordinates
/// become bitmap-space Y-down coordinates.
fn replay_path_y_flipped(path: &[PathElement], offset_x: f32, y_flip: f32, rast: &mut Rasterizer) {
    for el in path {
        match *el {
            PathElement::MoveTo { x, y } => rast.move_to(x + offset_x, y_flip - y),
            PathElement::LineTo { x, y } => rast.line_to(x + offset_x, y_flip - y),
            PathElement::QuadTo { cx0, cy0, x, y } => {
                rast.quad_to(cx0 + offset_x, y_flip - cy0, x + offset_x, y_flip - y)
            }
            PathElement::CurveTo { .. } => {
                // TrueType outlines never produce cubic curves; skip.
            }
            PathElement::Close => rast.close(),
        }
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

    /// Second embedded TTF — PT Sans Caption (full, non-subset). Used by
    /// the Gap #8 family-matching tests alongside [`TEST_FONT_TTF`] to
    /// exercise multi-family resolution. PT Sans Caption has a distinct
    /// family name from OpenSans, so the two are suitable for verifying
    /// that [`HarfRustFontRegistry::resolve`] dispatches on family.
    const TEST_FONT_TTF_2: &[u8] = include_bytes!(
        "../../../vendor/harfrust/harfrust/tests/fonts/rb_custom/PT_Sans-Caption-Web-Regular.ttf"
    );

    // -- HarfRust-backed tests ---------------------------------------------

    /// Glyph ID of the only glyph in `TEST_FONT_TTF` that has a real
    /// outline. The OpenSans subset font ships with exactly 2 glyphs:
    /// `gid 0` (`.notdef`, no outline) and `gid 1` (1 contour,
    /// bbox `(150, -28, 388, 233)` in font units). The font's `cmap` does
    /// not map any codepoint, so shaping any character returns `gid 0`;
    /// atlas tests that need a non-zero bitmap use this constant directly.
    const TEST_GLYPH_WITH_OUTLINE: u32 = 1;

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

    /// SEC-03: a bundle strictly larger than [`MAX_FONT_SIZE`] is rejected
    /// before the parser is ever invoked, with a synthetic `SIZE` tag so
    /// the failure is distinguishable from a genuine `sfnt` decode error.
    #[test]
    fn load_bundle_rejects_oversized_font() {
        let mut registry = HarfRustFontRegistry::new();
        let oversized = vec![0u8; MAX_FONT_SIZE + 1];
        let result = registry.load_bundle(&oversized);
        assert!(result.is_err());
        assert!(
            matches!(
                result,
                Err(FontLoadError::TableDecodeFailed { ref table, .. })
                    if table.0 == *b"SIZE",
            ),
            "oversized bundle should produce TableDecodeFailed with SIZE tag, got {:?}",
            result
        );
        assert!(
            registry.is_empty(),
            "registry should remain empty on oversized rejection"
        );
    }

    #[test]
    fn harfrust_registry_resolve_returns_first_font() {
        let mut reg = HarfRustFontRegistry::new();
        // Empty registry → RegistryEmpty.
        assert!(matches!(
            reg.resolve(&FontRequest::default()),
            Err(FontLoadError::RegistryEmpty)
        ));
        // Load a font and read back its extracted family/weight (Gap #8).
        let id = reg.load_bundle(TEST_FONT_TTF).unwrap();
        let (family, weight) = reg
            .family_and_weight(id)
            .expect("loaded font must expose family_and_weight");
        // Resolving by the loaded font's actual family + weight must hit
        // the exact-match path and return the same FontId.
        let req = FontRequest {
            family: family.to_string(),
            weight,
            style: "normal",
        };
        let resolved = reg.resolve(&req).expect("resolve must be Ok");
        assert_eq!(resolved, id);
    }

    /// Wave 4 (Gap #8): a request for a family that is not loaded (and is
    /// not a generic alias) returns [`FontLoadError::FamilyNotFound`].
    #[test]
    fn harfrust_registry_resolve_unknown_family_returns_family_not_found() {
        let mut reg = HarfRustFontRegistry::new();
        reg.load_bundle(TEST_FONT_TTF).unwrap();
        let req = FontRequest {
            family: String::from("This Font Does Not Exist"),
            weight: 400,
            style: "normal",
        };
        assert!(matches!(
            reg.resolve(&req),
            Err(FontLoadError::FamilyNotFound)
        ));
    }

    /// Wave 4 (Gap #8): loading two fonts with different families lets
    /// [`resolve`](FontRegistry::resolve) return the correct [`FontId`] for
    /// each, by exact family + exact weight match.
    #[test]
    fn harfrust_registry_resolve_matches_family_and_weight() {
        let mut reg = HarfRustFontRegistry::new();
        let id_a = reg.load_bundle(TEST_FONT_TTF).unwrap();
        let id_b = reg.load_bundle(TEST_FONT_TTF_2).unwrap();
        assert_eq!(id_a, FontId(0));
        assert_eq!(id_b, FontId(1));
        assert_ne!(id_a, id_b);

        // Read back each font's extracted family/weight as owned values so
        // the borrows from `reg` end before the `&mut self` resolve calls
        // (Gap #8).
        let (family_a, weight_a) = reg
            .family_and_weight(id_a)
            .expect("font A must expose family_and_weight");
        let (family_b, weight_b) = reg
            .family_and_weight(id_b)
            .expect("font B must expose family_and_weight");
        let family_a = family_a.to_string();
        let family_b = family_b.to_string();

        // The two fonts must have different families for this test to be
        // meaningful; if they don't, the registry's family extraction is
        // broken.
        assert_ne!(
            family_a, family_b,
            "the two test fonts must have different family names for Gap #8 coverage"
        );

        // Resolve each family → correct FontId.
        let req_a = FontRequest {
            family: family_a.clone(),
            weight: weight_a,
            style: "normal",
        };
        let req_b = FontRequest {
            family: family_b.clone(),
            weight: weight_b,
            style: "normal",
        };
        assert_eq!(
            reg.resolve(&req_a).expect("resolve family A must be Ok"),
            id_a
        );
        assert_eq!(
            reg.resolve(&req_b).expect("resolve family B must be Ok"),
            id_b
        );

        // Case-insensitive family match: requesting "FAMILY_A" in upper
        // case must still resolve to id_a (Gap #8 case-insensitivity).
        let req_a_upper = FontRequest {
            family: family_a.to_ascii_uppercase(),
            weight: weight_a,
            style: "normal",
        };
        assert_eq!(
            reg.resolve(&req_a_upper)
                .expect("resolve family A (upper) must be Ok"),
            id_a
        );
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
        let font_id = reg
            .load_bundle(TEST_FONT_TTF)
            .expect("load_bundle must be Ok");

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
        assert_eq!(run.caret_map.caret_to_glyph.len(), run.glyph_ids.len() + 1);
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

    /// SEC-04: a text run whose byte length strictly exceeds
    /// [`MAX_TEXT_LENGTH`] is rejected before shaping begins. The error
    /// reuses [`ShapeError::InvalidUtf8`] (no `TooLong` variant exists;
    /// adding one would change the public enum ABI).
    #[test]
    fn shape_rejects_oversized_text() {
        let registry = HarfRustFontRegistry::new();
        let shaper = HarfRustTextShaper::new(Arc::new(registry));
        let oversized = "e".repeat(MAX_TEXT_LENGTH + 1);
        let result = shaper.shape(&oversized, &ShapeContext::default());
        assert!(result.is_err());
        assert!(
            matches!(result, Err(ShapeError::InvalidUtf8)),
            "oversized text should produce InvalidUtf8, got {:?}",
            result
        );
    }

    #[test]
    fn harfrust_registry_multiple_fonts() {
        let mut reg = HarfRustFontRegistry::new();
        let id0 = reg.load_bundle(TEST_FONT_TTF).unwrap();
        let id1 = reg.load_bundle(TEST_FONT_TTF).unwrap();
        assert_eq!(id0, FontId(0));
        assert_eq!(id1, FontId(1));
        assert_eq!(reg.len(), 2);

        // Wave 4 (Gap #8): resolve now dispatches on family/weight rather
        // than returning the first loaded font. Two copies of the same
        // font share the same family, so an exact-family+exact-weight
        // request resolves to the first copy (id0). See
        // `harfrust_registry_resolve_matches_family_and_weight` for the
        // multi-family case.
        let (family, weight) = reg
            .family_and_weight(id0)
            .expect("loaded font must expose family_and_weight");
        let req = FontRequest {
            family: family.to_string(),
            weight,
            style: "normal",
        };
        let resolved = reg.resolve(&req).expect("resolve must be Ok");
        assert_eq!(resolved, id0);

        // Both faces should be accessible.
        assert_eq!(reg.face(id0).id, id0);
        assert_eq!(reg.face(id1).id, id1);
    }

    // -- HarfRust glyph atlas tests (Wave 2) -------------------------------

    /// Helper: load the test font into a registry, wrap in `Arc`, and
    /// return `(Arc<registry>, font_id)`.
    fn registry_with_test_font() -> (Arc<HarfRustFontRegistry>, FontId) {
        let mut reg = HarfRustFontRegistry::new();
        let font_id = reg
            .load_bundle(TEST_FONT_TTF)
            .expect("load_bundle must be Ok");
        (Arc::new(reg), font_id)
    }

    #[test]
    fn harfrust_glyph_atlas_ensure_returns_nonzero_uv() {
        let (registry, font_id) = registry_with_test_font();

        let mut atlas = HarfRustGlyphAtlas::new(Arc::clone(&registry));
        let key = GlyphKey {
            font_id,
            glyph_id: TEST_GLYPH_WITH_OUTLINE,
            phase: 0,
            size_px: 32,
        };
        let slot = atlas.ensure(key);

        // The outlined glyph must produce a non-zero UV rect.
        assert!(
            slot.uv.w > 0.0 && slot.uv.h > 0.0,
            "UV rect must be non-zero for outlined glyph, got {:?}",
            slot.uv
        );
        assert!(
            slot.size.0 > 0.0 && slot.size.1 > 0.0,
            "size must be non-zero for outlined glyph, got {:?}",
            slot.size
        );
        assert_eq!(slot.page, 0, "first glyph should be on page 0");
    }

    #[test]
    fn harfrust_glyph_atlas_writes_nonzero_pixels_to_page() {
        let (registry, font_id) = registry_with_test_font();

        let mut atlas = HarfRustGlyphAtlas::new(Arc::clone(&registry));
        let key = GlyphKey {
            font_id,
            glyph_id: TEST_GLYPH_WITH_OUTLINE,
            phase: 0,
            size_px: 32,
        };
        let slot = atlas.ensure(key);
        assert!(slot.uv.w > 0.0 && slot.uv.h > 0.0);

        // The atlas page must contain at least one non-zero pixel.
        let page = atlas.page_data(0).expect("page 0 must exist");
        let nonzero_count = page.iter().filter(|&&a| a > 0).count();
        assert!(
            nonzero_count > 0,
            "atlas page should have at least one non-zero pixel after rasterizing"
        );
    }

    #[test]
    fn harfrust_glyph_atlas_caches_slots() {
        let (registry, font_id) = registry_with_test_font();

        let mut atlas = HarfRustGlyphAtlas::new(Arc::clone(&registry));
        let key = GlyphKey {
            font_id,
            glyph_id: TEST_GLYPH_WITH_OUTLINE,
            phase: 0,
            size_px: 16,
        };

        let slot1 = atlas.ensure(key);
        assert_eq!(
            atlas.len(),
            1,
            "cache should have one entry after first ensure"
        );
        let slot2 = atlas.ensure(key);
        assert_eq!(
            atlas.len(),
            1,
            "cache should still have one entry after second ensure (hit)"
        );
        assert_eq!(slot1, slot2, "second ensure should return the cached slot");
        assert_eq!(atlas.slot(key), Some(slot1), "slot lookup should hit");
    }

    #[test]
    fn harfrust_glyph_atlas_reset_clears_cache_and_pages() {
        // TD1 safety valve: reset() must return the atlas to a pristine
        // single-page state with an empty cache, so a persistent
        // (long-lived) renderer atlas can gracefully recover from page
        // overflow by re-rasterizing only the current frame's glyphs.
        let (registry, font_id) = registry_with_test_font();
        let mut atlas = HarfRustGlyphAtlas::new(Arc::clone(&registry));

        // Fill with a couple of small glyphs.
        for gid in [TEST_GLYPH_WITH_OUTLINE, TEST_GLYPH_WITH_OUTLINE + 1] {
            atlas.ensure(GlyphKey {
                font_id,
                glyph_id: gid,
                phase: 0,
                size_px: 16,
            });
        }
        assert!(!atlas.is_empty());
        assert_eq!(atlas.page_count(), 1);

        atlas.reset();

        assert!(atlas.is_empty(), "reset must clear the cache");
        assert_eq!(atlas.len(), 0);
        assert_eq!(
            atlas.page_count(),
            1,
            "reset must collapse back to a single fresh page"
        );
        // The fresh page must be all zeros.
        let page = atlas.page_data(0).expect("page 0 must exist");
        assert!(page.iter().all(|&a| a == 0), "reset page must be zeroed");
        // Re-ensuring the same glyph re-rasterizes onto the fresh page.
        let slot = atlas.ensure(GlyphKey {
            font_id,
            glyph_id: TEST_GLYPH_WITH_OUTLINE,
            phase: 0,
            size_px: 16,
        });
        assert_eq!(atlas.len(), 1);
        assert_eq!(slot.page, 0);
    }

    #[test]
    fn harfrust_glyph_atlas_overflow_allocates_pages_then_reset_recovers() {
        // The test font has a single outlined glyph, so overflow is forced
        // via the SAME glyph at many distinct sizes (each size is a
        // distinct GlyphKey → a distinct packed slot). Large sizes fill
        // the 512×512 page and force a second; reset() must recover to
        // one page.
        let (registry, font_id) = registry_with_test_font();
        let mut atlas = HarfRustGlyphAtlas::new(Arc::clone(&registry));

        let mut size: u16 = 64;
        let mut tried = 0;
        while atlas.page_count() == 1 && tried < 400 {
            atlas.ensure(GlyphKey {
                font_id,
                glyph_id: TEST_GLYPH_WITH_OUTLINE,
                phase: 0,
                size_px: size,
            });
            size = size.saturating_add(2);
            tried += 1;
        }
        assert!(
            atlas.page_count() > 1,
            "distinct large sizes must eventually overflow the 512×512 page (tried {})",
            tried
        );
        assert!(tried > 1, "overflow must come from many distinct keys");

        atlas.reset();
        assert_eq!(atlas.page_count(), 1);
        assert!(atlas.is_empty());
    }

    #[test]
    fn harfrust_glyph_atlas_reset_matches_invalidate_semantics() {
        // GlyphAtlas::invalidate delegates to reset() — both must produce
        // the same pristine state.
        let (registry, font_id) = registry_with_test_font();
        let mut a = HarfRustGlyphAtlas::new(Arc::clone(&registry));
        let mut b = HarfRustGlyphAtlas::new(Arc::clone(&registry));
        for atlas in [&mut a, &mut b] {
            atlas.ensure(GlyphKey {
                font_id,
                glyph_id: TEST_GLYPH_WITH_OUTLINE,
                phase: 0,
                size_px: 16,
            });
        }
        a.invalidate(alkalive_core::ModuleId(1), DirtyRect::default());
        b.reset();
        assert_eq!(a.len(), b.len());
        assert_eq!(a.page_count(), b.page_count());
        assert!(a.is_empty() && b.is_empty());
    }

    #[test]
    fn harfrust_glyph_atlas_packs_multiple_glyphs_at_different_sizes() {
        let (registry, font_id) = registry_with_test_font();

        let mut atlas = HarfRustGlyphAtlas::new(Arc::clone(&registry));
        // The same glyph at two different sizes produces two distinct
        // slots (the GlyphKey includes size_px).
        let key_small = GlyphKey {
            font_id,
            glyph_id: TEST_GLYPH_WITH_OUTLINE,
            phase: 0,
            size_px: 16,
        };
        let key_large = GlyphKey {
            font_id,
            glyph_id: TEST_GLYPH_WITH_OUTLINE,
            phase: 0,
            size_px: 48,
        };
        let slot_small = atlas.ensure(key_small);
        let slot_large = atlas.ensure(key_large);
        assert_eq!(atlas.len(), 2);
        // Both should be non-zero and on the same page.
        assert!(slot_small.uv.w > 0.0 && slot_small.uv.h > 0.0);
        assert!(slot_large.uv.w > 0.0 && slot_large.uv.h > 0.0);
        assert_eq!(slot_small.page, slot_large.page);
        // The two slots should not overlap (different x or y).
        assert!(
            slot_small.uv.x != slot_large.uv.x || slot_small.uv.y != slot_large.uv.y,
            "two glyphs should be packed at different positions"
        );
    }

    #[test]
    fn harfrust_glyph_atlas_glyph_with_no_outline_returns_zero_slot() {
        let (registry, font_id) = registry_with_test_font();

        let mut atlas = HarfRustGlyphAtlas::new(Arc::clone(&registry));
        // Glyph ID 0 is .notdef with no outline in the test font.
        let key = GlyphKey {
            font_id,
            glyph_id: 0,
            phase: 0,
            size_px: 16,
        };
        let slot = atlas.ensure(key);
        assert_eq!(slot.uv, Rect::default(), ".notdef should have zero UV");
        assert_eq!(slot.size, (0.0, 0.0), ".notdef should have zero size");
        // The zero slot is still cached.
        assert_eq!(atlas.slot(key), Some(slot));
    }

    #[test]
    fn harfrust_glyph_atlas_missing_glyph_returns_zero_slot() {
        let (registry, font_id) = registry_with_test_font();

        let mut atlas = HarfRustGlyphAtlas::new(Arc::clone(&registry));
        // Glyph ID 99999 is out of range — should return a zero-size slot.
        let key = GlyphKey {
            font_id,
            glyph_id: 99999,
            phase: 0,
            size_px: 16,
        };
        let slot = atlas.ensure(key);
        assert_eq!(
            slot.uv,
            Rect::default(),
            "missing glyph should have zero UV"
        );
        assert_eq!(slot.size, (0.0, 0.0), "missing glyph should have zero size");
        // The zero slot is still cached.
        assert_eq!(atlas.slot(key), Some(slot));
    }

    #[test]
    fn harfrust_glyph_atlas_unknown_font_returns_zero_slot() {
        let (registry, _font_id) = registry_with_test_font();

        let mut atlas = HarfRustGlyphAtlas::new(Arc::clone(&registry));
        let key = GlyphKey {
            font_id: FontId(999), // not registered
            glyph_id: 0,
            phase: 0,
            size_px: 16,
        };
        let slot = atlas.ensure(key);
        assert_eq!(slot.uv, Rect::default());
        assert_eq!(slot.size, (0.0, 0.0));
    }

    #[test]
    fn harfrust_glyph_atlas_invalidates_cache() {
        let (registry, font_id) = registry_with_test_font();

        let mut atlas = HarfRustGlyphAtlas::new(Arc::clone(&registry));
        let key = GlyphKey {
            font_id,
            glyph_id: TEST_GLYPH_WITH_OUTLINE,
            phase: 0,
            size_px: 16,
        };
        let _slot = atlas.ensure(key);
        assert!(!atlas.is_empty(), "cache should be non-empty after ensure");
        assert_eq!(atlas.page_count(), 1);

        atlas.invalidate(ModuleId(0), DirtyRect);
        assert!(atlas.is_empty(), "cache should be empty after invalidate");
        assert_eq!(
            atlas.page_count(),
            1,
            "invalidate should reset to a single fresh page"
        );

        // The page data should be all zeros after invalidation.
        let page = atlas.page_data(0).expect("page 0 must exist");
        assert!(
            page.iter().all(|&a| a == 0),
            "page should be cleared after invalidate"
        );

        // Re-ensuring still works (re-rasterizes into the fresh page).
        let slot = atlas.ensure(key);
        assert!(slot.uv.w > 0.0 && slot.uv.h > 0.0);
    }

    #[test]
    fn harfrust_glyph_atlas_evict_lru_is_noop() {
        let (registry, font_id) = registry_with_test_font();

        let mut atlas = HarfRustGlyphAtlas::new(Arc::clone(&registry));
        let key = GlyphKey {
            font_id,
            glyph_id: TEST_GLYPH_WITH_OUTLINE,
            phase: 0,
            size_px: 16,
        };
        let slot = atlas.ensure(key);

        let stats = atlas.evict_lru(&PinSet::default());
        assert_eq!(
            stats,
            EvictionStats::default(),
            "evict_lru should be a no-op"
        );
        // The slot should still be cached after eviction.
        assert_eq!(atlas.slot(key), Some(slot), "slot should survive evict_lru");
    }

    #[test]
    fn harfrust_glyph_atlas_size_scales_with_size_px() {
        let (registry, font_id) = registry_with_test_font();

        let mut atlas = HarfRustGlyphAtlas::new(Arc::clone(&registry));
        let key_small = GlyphKey {
            font_id,
            glyph_id: TEST_GLYPH_WITH_OUTLINE,
            phase: 0,
            size_px: 16,
        };
        let key_large = GlyphKey {
            font_id,
            glyph_id: TEST_GLYPH_WITH_OUTLINE,
            phase: 0,
            size_px: 64,
        };
        let slot_small = atlas.ensure(key_small);
        let slot_large = atlas.ensure(key_large);
        // Larger size should produce a larger bitmap.
        assert!(
            slot_large.size.0 > slot_small.size.0 && slot_large.size.1 > slot_small.size.1,
            "larger size_px should produce a larger bitmap: small={:?} large={:?}",
            slot_small.size,
            slot_large.size
        );
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
