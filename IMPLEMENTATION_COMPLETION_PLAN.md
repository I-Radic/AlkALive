# Implementation Completion Plan

**Derived from:** `UNFINISHED_IMPLEMENTATIONS.md` (8 TODO items, 6 deferred items)

## Wave 3: PipelineCache LRU + Trace Span Store
**Gaps:** #5 (PipelineCache LRU), #3 (TraceRecorder::exit), #4 (ErrorBoundary::report)
**DoD:** PipelineCache bounded at 64MB; TraceRecorder stores spans in a Vec; ErrorBoundary::report records failures.

Tasks:
- 3.1: Implement LRU eviction in PipelineCache (track byte size, evict oldest on cap).
- 3.2: Add span store (Vec<TraceSpan>) to AuthorTraceRecorder; implement exit() and report().
- 3.3: Add tests for LRU eviction and span recording.

## Wave 4: Animation Interpolation + Font Matching
**Gaps:** #6 (Animation keyframe interpolation), #8 (Font family matching)
**DoD:** Animation::tick interpolates between keyframes; HarfRustFontRegistry::resolve matches by family name.

Tasks:
- 4.1: Implement keyframe interpolation in Animation::tick (Linear/Step modes).
- 4.2: Implement real family/weight matching in HarfRustFontRegistry::resolve.
- 4.3: Add tests for interpolation and font matching.

## Wave 5: Signal Observer Registry + Panic Trapping
**Gaps:** #1 (Signal dispatch), #2 (Panic trapping)
**DoD:** Signal::emit dispatches to registered listeners; ErrorBoundary::trap catches panics.

Tasks:
- 5.1: Add observer registry to Signal (Vec<Listener>, emit dispatches to all).
- 5.2: Implement panic catching in ErrorBoundary::trap using catch_unwind.
- 5.3: Add tests for signal dispatch and panic recovery.

## Wave 6: Final Verification
**DoD:** All Medium TODOs resolved or formally deferred; full test suite passes.

Tasks:
- 6.1: Re-scan for remaining TODOs.
- 6.2: Run cargo test --workspace + cargo build --target wasm32-unknown-unknown.
- 6.3: Update README.
