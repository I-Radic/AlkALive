# TODO Resolution Plan

**Goal:** Eliminate all 47 `todo!()` calls and produce a fully compilable, testable codebase.

**Strategy:** Most `todo!()` calls are trait default methods. The approach is:
1. Remove `todo!()` defaults → make methods required (no default body) where a concrete backend is needed
2. Provide real default implementations where sensible
3. Create concrete stub/mock implementations for all traits

**Wave breakdown:**

| Wave | Crates | todo!() count | Strategy |
|------|--------|---------------|----------|
| W1 | alkalive-style | 11 | Replace trait defaults with real impls; make Style/Theme required |
| W2 | alkalive-text | 21 | Make trait methods required; expand MockTextStack to cover all; replace NoopAtlas with MockGlyphAtlas |
| W3 | alkalive-layout | 7 | Replace MeasuredRun/LayoutSolver defaults with required methods; CassowarySolver already implements |
| W4 | alkalive-input | 8 | Make GrabHandle/GestureState required; provide concrete stubs |
| W5 | alkalive-error + alkalive-core | 2+2 | Implement RecoveryStrategy stubs; implement Signal with observer list |
| W6 | Final verification | 0 | cargo test --workspace, cargo build --target wasm32, zero todo!() |
