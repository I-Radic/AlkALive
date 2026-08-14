# AlkALive Fine Draft — Critical Review (Task ID 4)

> **Reviewer:** Wave 4 Critical-Review subagent.
> **Scope:** Critical review of the integrated fine draft
> (`docs/alkalive-remaining-gaps-fine-draft.md`) and its two predecessors
> (`docs/alkalive-fine-draft-language.md` for gaps 1–5,
>  `docs/alkalive-fine-draft-rendering.md` for gaps 6–8).
> **Method:** Compared the fine draft against (a) ADRs 001, 003, 006, 007,
> 008, 009, 018, 021, 022; (b) `docs/technical-specification.md`;
> (c) the existing implementation files (`ast.rs`, `typechecker.rs`,
> `wasm_codegen.rs`, `alkalive-backend-wgpu/src/lib.rs`,
> `alkalive-render/src/lib.rs`, `schedule.rs`, `runtime-wasm/src/lib.rs`);
> (d) existing wave documentation.
>
> **Verdict:** The fine draft is *mostly sound* — the gap decomposition,
> dependency graph, and per-gap designs are coherent and traceable to ADRs.
> However, the review identified **30 findings** (1 Critical, 12 Major,
> 14 Minor, 3 Info) that the orchestrator should address before
> implementation waves begin. The Critical finding (CR-28) blocks the
> first iteration of Gap 8 entirely; several Major findings (CR-2, CR-14,
> CR-15, CR-19, CR-21) would cause implementation to stall or produce
> broken behaviour if not resolved.
>
> This review **documents** findings only — it does **not** fix them.

---

## 0. Review Checklist (all 11 categories covered)

| # | Category | Covered? | Finding count |
|---|----------|----------|---------------|
| 1 | Contradictions with ADRs / existing implementation | ✓ | CR-2, CR-9, CR-19, CR-21, CR-26 |
| 2 | Missing ADR requirements | ✓ | CR-6, CR-20, CR-21, CR-23, CR-30 |
| 3 | Circular dependencies between gaps | ✓ | CR-1 |
| 4 | Ambiguous semantics | ✓ | CR-3, CR-4, CR-5, CR-7, CR-8, CR-17, CR-20, CR-23 |
| 5 | Designs incompatible with existing compiler | ✓ | CR-10, CR-14, CR-15, CR-19, CR-28 |
| 6 | Duplicate responsibilities | ✓ | CR-5, CR-13, naming-collision (§3.1) |
| 7 | Unnecessary complexity | ✓ | CR-17, CR-18 |
| 8 | Performance risks on low-end hardware | ✓ | CR-13, CR-25, CR-28 |
| 9 | Hidden migration problems | ✓ | CR-2, CR-11, CR-12, CR-16, CR-24, CR-29 |
| 10 | Missing error behaviour | ✓ | CR-8, CR-10, CR-20, naming-mismatch (§3.2) |
| 11 | Missing testability | ✓ | CR-14, CR-15, CR-27 |

---

## 1. Critical Findings

### CR-1 — `RenderGraph` lacks `Serialize`/`Deserialize`; Gap 8 worker `postMessage` path is blocked

- **Severity:** Critical
- **Category:** Designs incompatible with existing implementation / Missing requirement
- **Description:** Gap 8's render worker (`docs/alkalive-fine-draft-rendering.md:2227-2243`) defines a `WorkerMessage` enum that derives `serde::Deserialize` and contains a `graph: alkalive_render::RenderGraph` field. The worker deserialises incoming messages via `serde_wasm_bindgen::from_value(data)`. However, the existing `RenderGraph` type (and every type it transitively contains — `RenderPass`, `Attachment`, `DrawCall`, `VertexBinding`, `IndexBinding`, `BindGroup`, `AttachmentId`, `PassId`, `DrawCallId`, `ModuleId`, `OcclusionCullPass`, `ExtentOrRelative`, `AttachmentFormat`, `SampleCount`, `ClearOp`, `PassType`, `PipelineHandle`, `Range<u32>`, `DirtyRect`, `Box<[T]>`) derives **only `Debug, Clone`** (verified at `crates/alkalive-render/src/lib.rs:182, 199, 216, 220, 225, 229, 246, 253`). The `alkalive-render` crate's `Cargo.toml` has no `serde` dependency. The structured-clone algorithm used by `postMessage` does **not** automatically handle arbitrary Rust structs — `serde_wasm_bindgen` requires `Serialize`/`Deserialize` derives.
- **Evidence:**
  - `crates/alkalive-render/src/lib.rs:253` — `#[derive(Debug, Clone)] pub struct RenderGraph { ... }` (no serde).
  - `crates/alkalive-render/Cargo.toml:12-14` — deps are only `alkalive-core` and `alkalive-text` (no `serde`).
  - `docs/alkalive-fine-draft-rendering.md:2222-2243` — `#[derive(serde::Deserialize)] struct WorkerMessage { ... graph: alkalive_render::RenderGraph ... }`.
  - `docs/alkalive-fine-draft-rendering.md:2183` — `serde_wasm_bindgen::from_value(data)`.
  - `docs/alkalive-fine-draft-rendering.md:8.5.5` (line 2466-2478) — the `to_bytes`/`from_bytes` SAB path is explicitly "future work".
- **Impact:** The first iteration of Gap 8 cannot compile. Either (a) serde derives must be retroactively added to ~15 public types in `alkalive-render` (a breaking API change to a `#![forbid(unsafe_code)]` crate that is consumed by both the compiler and runtime), or (b) the worker message protocol must be redesigned to send only primitive types (e.g. serialise `RenderGraph` to a `Vec<u8>` on the main thread and send the bytes).
- **Recommendation:** Add a pre-requisite sub-task to Gap 8: "Add `serde = { version = "1", features = ["derive"] }` to `alkalive-render/Cargo.toml` and `#[derive(Serialize, Deserialize)]` to all render-graph IR types." Alternatively, specify the `to_bytes`/`from_bytes` flat-buffer encoding as part of Gap 8's first cut (not future work). The orchestrator should not approve Gap 8 implementation until this is resolved.

---

## 2. Major Findings

### CR-2 — Gap 2's "architectural inversion" contradicts technical-specification C10 / TD8 and breaks the existing deployment model

- **Severity:** Major
- **Category:** Contradiction / Hidden migration problem
- **Description:** The language fine draft (`docs/alkalive-fine-draft-language.md:735`) states: *"The WASM module exports a single `main` (or a small set of entry points). The runtime's `start()` calls `main` instead of `compile_to_wasm` directly — the `.alk` source is now compiled to WASM ahead-of-time and the runtime loads the resulting binary. (This is the architectural inversion flagged in the Wave 0 audit §10.2; Gap 2 is the moment it happens.)"*
  This directly contradicts:
  - `docs/technical-specification.md:697-698` (TD8): *"The `examples/hello.alk` source is `include_str!`-ed into the WASM binary at build time (`alkalive-runtime-wasm/src/lib.rs` line 52). The scene is fixed at build time; there is no runtime scene loading."*
  - `docs/technical-specification.md:680` (C10): *"Single-threaded WASM (no `SharedArrayBuffer`, no workers in the current phase)."*
  - The actual implementation at `crates/alkalive-runtime-wasm/src/lib.rs:81` (`const HELLO_ALK_SRC: &str = include_str!("../../../examples/hello.alk");`) and `:216` (`let (scheduled, dep_graph) = alkalive_compiler::compile_with_deps(HELLO_ALK_SRC)`).
- **Impact:** The inversion changes (a) the deployment model (ship `.alk` source → ship pre-compiled `.wasm` binary), (b) the runtime's role (compiler+runtime → runtime-only that loads an external user-WASM), (c) the build pipeline (cargo build → cargo build + AlkALive AOT compile step), (d) startup cost (compile `.alk` at startup → instantiate pre-compiled `.wasm`), and (e) the `start()` function's internal behaviour (currently synchronous; would need to asynchronously load and instantiate a second WASM module via `WebAssembly.instantiate`). The fine draft does not address how the existing `deploy/index.html` (which loads a single `alkalive_runtime_wasm_bg.wasm`) would transition, what happens to `compile_with_deps` / `SignalStore` / `DependencyGraph` / `ScheduledScene` (are they still produced at runtime, or pre-computed at AOT time?), or whether a fallback to the current `include_str!` model is preserved.
- **Evidence:** `docs/alkalive-fine-draft-language.md:735`; `docs/technical-specification.md:680, 697-698`; `crates/alkalive-runtime-wasm/src/lib.rs:81, 216`; `deploy/index.html:13-14` (loads a single WASM).
- **Recommendation:** Either (a) defer the architectural inversion to a separate wave with its own ADR amendment and migration plan, or (b) explicitly enumerate every consumer of the current `compile_with_deps(HELLO_ALK_SRC)` path and specify how each transitions. The current wording ("Gap 2 is the moment it happens") buries a Critical-severity architectural change in a single bullet of §2.5.

### CR-3 — Crate dependency cycle between `alkalive-render` and `alkalive-backend-wgpu`

- **Severity:** Major
- **Category:** Circular dependency
- **Description:** The rendering fine draft (`docs/alkalive-fine-draft-rendering.md:3058-3076`, §9 Appendix B) explicitly acknowledges a cycle: *"edges 1, 2, 3 form a cycle (`alkalive-render` ↔ `alkalive-backend-wgpu`)"*. The proposed mitigation is to define a `SceneData` trait in `alkalive-render` and have `alkalive-backend-wgpu` implement it for `TextSceneData`, so `schedule_to_render_graph` accepts `&dyn SceneData` instead of `&TextSceneData`. However, the trait's method set is unspecified — the lowering function reads `scene.background_normalized()`, `scene.text_color`, `scene.rotation_speed`, `scene.input_text`, `scene.font_size` (per §6.5.5 `lower_pass_kind`). Each of these must be a trait method. The draft says only "the trait is defined in `alkalive-render`, and `alkalive-backend-wgpu` implements it" without listing the methods.
- **Impact:** If the trait method set is incomplete or wrong, the cycle returns. If `TextSceneData` gains a new field (e.g. for ADR-004 layout output), the trait must be amended — a cross-crate coordinated change. The "small wart" framing understates the maintenance cost.
- **Evidence:** `docs/alkalive-fine-draft-rendering.md:3058-3076`; `crates/alkalive-render/Cargo.toml` (no dep on `alkalive-backend-wgpu` today); `crates/alkalive-backend-wgpu/src/lib.rs:53-110` (`TextSceneData` definition).
- **Recommendation:** Fully specify the `SceneData` trait (method signatures + semantics) in the fine draft. Alternatively, move `TextSceneData` to a new tiny `alkalive-scene-data` crate (the draft mentions this as the "follow-up cleanup" at line 3074-3076 — promote it to the first-cut design). The orchestrator should not approve Gap 6 implementation until the cycle is structurally broken, not trait-papered-over.

### CR-4 — `DrawCall` lacks an `id` field; first iteration of Gap 6 cannot resolve draw calls

- **Severity:** Major
- **Category:** Designs incompatible with existing implementation / Missing requirement
- **Description:** The existing `DrawCall` struct (`crates/alkalive-render/src/lib.rs:230-243`) has no `id` field. The rendering fine draft's `schedule_to_render_graph` lowering produces `DrawCallId(i)` for each draw call (§6.5.5, line 585) and stores the ID in the `RenderPass.draw_calls` slice. The renderer's `execute_pass` (§6.5.6, line 787-790) does `graph.draw_calls.iter().find(|d| d.id_for_lookup() == dc_id)` — but `id_for_lookup()` is a placeholder trait method that **always returns `DrawCallId(0)`** (§6.5.6, line 851-857). The draft acknowledges this at line 861-865: *"The DrawCall struct currently has no id field; the lowering populates a parallel draw_call_kinds: Box<[DrawCallKind]> field on RenderGraph (side table). A follow-up edit adds pub id: DrawCallId and pub kind: DrawCallKind directly to DrawCall, eliminating the side table and the lookup helper. The two-phase edit keeps the diff reviewable."*
  However, the *first* phase is broken: `find` with a constant `DrawCallId(0)` always returns the first draw call, regardless of which pass is executing. Every pass would render the first draw call's content. The Hello World scene (5 passes, 5 draw calls) would render the Clear draw call 5 times.
- **Impact:** Gap 6's first iteration produces a broken renderer. The "two-phase edit" framing assumes the second phase lands in the same PR — but the draft explicitly says "follow-up commit", implying the first commit is intentionally broken.
- **Evidence:** `crates/alkalive-render/src/lib.rs:230-243` (no `id` field); `docs/alkalive-fine-draft-rendering.md:851-857` (placeholder `id_for_lookup`); `docs/alkalive-fine-draft-rendering.md:861-865` (acknowledged wart).
- **Recommendation:** Require the `pub id: DrawCallId` and `pub kind: DrawCallKind` fields to be added to `DrawCall` **in the same PR** as the rest of Gap 6. The "two-phase edit" should not be permitted to land a broken intermediate state.

### CR-5 — wgpu `render_compiled` hardcodes `LoadOp::Clear(BLACK)`, ignoring `DrawCallKind::Clear { color }`

- **Severity:** Major
- **Category:** Designs incompatible / Missing requirement (correctness bug)
- **Description:** The rendering fine draft's wgpu `render_compiled` (§7.5.4, line 1673-1686) creates a single render pass with `let clear_color = wgpu::LoadOp::Clear(wgpu::Color::BLACK);` — a hardcoded black clear. The lowering's `DrawCallKind::Clear { color: [r, g, b, 1.0] }` (§6.5.5, line 639-641) is supposed to drive the clear color, but the wgpu path ignores it. The raw-WebGL2 path (§6.5.6, line 805-809) correctly reads `color` from the `DrawCallKind` and calls `gl.clear_color(color[0], ...)`. So the two backends disagree.
  The Hello World scene's `background: #000000` happens to be black, so the bug is invisible — but any scene with a non-black background would render with a black background on the wgpu/WebGPU path.
- **Impact:** Correctness regression on the wgpu path. Hidden by the Hello World demo's choice of black background.
- **Evidence:** `docs/alkalive-fine-draft-rendering.md:1674` (`wgpu::Color::BLACK`); `docs/alkalive-fine-draft-rendering.md:639-641` (`DrawCallKind::Clear { color }`); `docs/alkalive-fine-draft-rendering.md:805-809` (raw-WebGL2 path reads `color`).
- **Recommendation:** The wgpu path's `render_compiled` must read the first pass's `DrawCallKind::Clear { color }` and use it as the `LoadOp::Clear` value. If no Clear draw call is present, fall back to black with a warning.

### CR-6 — `render_frame_with_dirty` is removed without a working replacement; ADR-025 incremental path regresses

- **Severity:** Major
- **Category:** Contradiction / Missing requirement / Regression
- **Description:** The rendering fine draft (§6.11 R6.5, line 1076-1082) states: *"The `render_frame_with_dirty` variant is removed (its `dirty_passes` parameter is replaced by per-pass dirty info plumbed through `compile()`'s `dirty` parameter)."* However:
  - The runtime currently calls `render_frame_with_dirty` at `crates/alkalive-runtime-wasm/src/lib.rs:679-684` (verified).
  - The `compile()` function's `dirty: &[DirtyRect]` parameter is **currently ignored** (per `crates/alkalive-render/src/lib.rs:454` — `let _ = (dirty, depth);`), and the rendering fine draft §6.6 point 2 (line 940-942) acknowledges: *"the dirty-pass info is plumbed through `compile()`'s `dirty: &[DirtyRect]` parameter (currently ignored — line 454 of `alkalive-render`)"*.
  - The replacement (per-pass dirty info via `compile()`) is therefore non-functional.
  - ADR-025 (incremental computation) is **implemented** (per `docs/technical-specification.md:746-754` and `crates/alkalive-runtime-wasm/src/signal_store.rs`). Removing `render_frame_with_dirty` without a working replacement breaks the ADR-025 incremental path — the runtime would fall back to full re-render every frame.
- **Impact:** Regression of an already-implemented ADR (ADR-025). The Hello World demo's perf would not regress (it's a trivial scene), but any larger scene using incremental computation would.
- **Evidence:** `docs/alkalive-fine-draft-rendering.md:1076-1082` (removal); `crates/alkalive-runtime-wasm/src/lib.rs:679` (current caller); `crates/alkalive-render/src/lib.rs:454` (`dirty` ignored); `docs/alkalive-fine-draft-rendering.md:940-942` (acknowledged).
- **Recommendation:** Either (a) keep `render_frame_with_dirty` until the `compile()` dirty parameter is actually implemented, or (b) specify the exact `compile()` dirty-parameter semantics in Gap 6 and require it to be functional before `render_frame_with_dirty` is removed. The current "plumbed through but ignored" state is a regression.

### CR-7 — `call_indirect` vtable semantics are ambiguous: "vtable_ptr" is a table index, not a pointer

- **Severity:** Major
- **Category:** Ambiguity / Designs incompatible with existing compiler
- **Description:** The language fine draft §1.4.3 (Object representation) says: *"the first 4 bytes hold a pointer to the class's vtable"*. §1.4.4 (Virtual dispatch) shows:
  ```wasm
  local.get $obj
  i32.load offset=0          ;; vtable_ptr (table index)
  call_indirect (type $foo_type)
  ```
  The comment says "vtable_ptr (table index)" — but the field is named "vtable_ptr" and the layout diagram labels it "vtable_ptr (i32)". This is contradictory: a WASM `call_indirect` instruction takes a **table index** (i32) on the stack, not a pointer to funcrefs in linear memory. If the field is a table index, it should be named `vtable_index` or `table_base`; if it's a pointer to funcrefs in linear memory, the dispatch sequence is wrong (you'd need `table.get` from reference types, not `call_indirect`).
  Additionally, the proposed dispatch uses a single table index per object — but the vtable has multiple method slots. The draft says "i32.const <slot>; call_indirect" in one variant (line 348) and "i32.load offset=0; call_indirect" in another (line 360). These are different dispatch schemes:
  - Scheme A: `i32.const <table_base + slot>; call_indirect` — the compiler computes the absolute table index at compile time.
  - Scheme B: `local.get $obj; i32.load offset=0; call_indirect` — the object stores its own table index (single method per table?).
  The draft presents both without choosing.
- **Impact:** An implementer would have to guess the dispatch scheme. The wrong choice produces a vtable that doesn't work (e.g., all methods dispatch to slot 0).
- **Evidence:** `docs/alkalive-fine-draft-language.md:316-326` (layout diagram says "vtable_ptr"); `docs/alkalive-fine-draft-language.md:343-362` (two conflicting dispatch sequences).
- **Recommendation:** Pick one scheme and specify it unambiguously. The standard C++/V8 scheme is: each class has a `vtable_base` global (an i32 constant = the table index where this class's vtable starts); each object stores its class's `vtable_base` in its first 4 bytes; dispatch is `local.get $obj; i32.load offset=0; i32.const <slot>; i32.add; call_indirect (type $foo_type)`. Rename "vtable_ptr" to "vtable_base" throughout. Specify whether the table is the MVP default table (index 0) or a declared table.

### CR-8 — Tree-shaking runs after typechecking; unreachable `pub fn` with type errors breaks the build

- **Severity:** Major
- **Category:** Contradiction / Designs incompatible
- **Description:** The language fine draft §2.4.4 (Tree-shaking) says: *"After typechecking, the compiler performs a reachability pass over the module graph."* The typechecker (per §3.4.2 `collect_signatures`) walks every `ItemDecl::Fn` and `ItemDecl::Class` to build the `FnSigTable`, then checks every function body. This means a `pub fn unused_helper() -> i32 { return true; }` (type error: `bool` returned where `i32` declared) would fail the build even though the function is unreachable from `main` and would be tree-shaken away.
  ADR-018 (line 621) promises *"compile-time tree-shaking"* — the user expectation is that unused code is dropped, not that it fails the build. Rust allows dead code with warnings; the fine draft's design fails the build.
- **Impact:** Library modules that export many `pub fn`s (only some of which are used by a given app) would fail to compile if any unused `pub fn` has a type error. This contradicts the "library" use case implied by `pub`.
- **Evidence:** `docs/alkalive-fine-draft-language.md:688-704` (tree-shaking after typecheck); `docs/alkalive-fine-draft-language.md:919-955` (`collect_signatures` walks all fns); `docs/adr/ADR.md:621` (ADR-018 tree-shaking promise).
- **Recommendation:** Specify one of: (a) typecheck only reachable functions (requires running tree-shaking *before* typechecking — but tree-shaking needs imported signatures, which need the resolver, which needs parsed modules — feasible but reorder the pipeline); (b) typecheck all functions but only error on reachable ones (collect errors, filter by reachability, report only reachable errors); (c) explicitly document that unused `pub fn`s must be type-correct (Rust's stance for `pub` items in libraries). The current draft is silent.

### CR-9 — Tree-shaking doesn't handle virtual dispatch; class methods are unsoundly dropped

- **Severity:** Major
- **Category:** Missing requirement / Ambiguity
- **Description:** The language fine draft §2.4.4 (Tree-shaking, line 695-697) says the reachability pass walks `Call`, `MethodCall`, `StaticCall`, `PathCall`, `Object` literal, and field access. For `Expr::MethodCall` on a `BaseType::Named` receiver, the actual function called is determined at runtime via the vtable (Gap 1 §1.4.4). The tree-shaker cannot know which method is called without conservative analysis.
  The draft says "mark the referenced function/class/let as reachable" — but for a virtual call `obj.foo()`, "referenced" is ambiguous: is `foo` on every subclass reachable, or only on the static type's class? If the tree-shaker marks only the static type's `foo`, then a derived class's override would be dropped — but the vtable's `elem` entry points to the override, so the dispatch would call a dropped function (undefined behaviour in WASM: `call_indirect` with a null funcref traps).
- **Impact:** Soundness hole. A class hierarchy with overrides could crash at runtime when the tree-shaker drops the override.
- **Evidence:** `docs/alkalive-fine-draft-language.md:695-697` (reachability walk); `docs/alkalive-fine-draft-language.md:341-362` (virtual dispatch via vtable); `docs/alkalive-fine-draft-language.md:698-701` (tree-shaking drops unreachable `pub` items).
- **Recommendation:** Specify the conservative rule: "if any instance of class C is constructed (via `Object` literal or `ClassName::new`), every method of C and every method of every subclass of C is reachable." This is the standard C++/LTO rule for virtual dispatch. Document the cost (less aggressive tree-shaking for OO code).

### CR-10 — Monotone/antitone qualifiers on class fields have unspecified assignment semantics; soundness hole

- **Severity:** Major
- **Category:** Missing requirement / Designs incompatible / Missing error behaviour
- **Description:** The OO model (Gap 1 §1.4.2) allows class fields of any `Type`, including `monotone Vec<i32>` (the `Type` struct already carries a `Qualifier` field — `crates/alkalive-compiler/src/ast.rs:388-393`). However:
  - The proposed `Stmt::Assign { target: Expr, value: Expr, ... }` (§1.4.2, line 296-302) does not specify whether field reassignment checks the qualifier.
  - A `monotone Vec<i32>` field `self.items` could be reassigned via `self.items = Vec::new();` — this replaces the Vec with a fresh empty one, violating the monotonicity invariant (the original Vec could only grow; the new one is empty, which is "smaller").
  - The existing `check_method_op` (`typechecker.rs:449-477`) only checks method calls on the receiver's qualifier — it does not check field assignment.
  - The proposed `Stmt::Assign` codegen (§1.5, line 392) says "compiles to `local.get obj; <value>; i32.store offset=<field_offset>`" — no qualifier check.
- **Impact:** A `monotone Vec<T>` field can be silently reset to empty via assignment, defeating ADR-027 Phase 2's monotonicity guarantee. This is a soundness hole in the type system.
- **Evidence:** `docs/alkalive-fine-draft-language.md:222-230` (`FieldDecl.ty: Type` — includes qualifier); `docs/alkalive-fine-draft-language.md:296-302` (`Stmt::Assign` — no qualifier check); `crates/alkalive-compiler/src/typechecker.rs:449-477` (`check_method_op` — only method calls); `docs/adr/ADR.md:261-266` (ADR-027 Phase 2: qualifiers enforced by type checker).
- **Recommendation:** Specify one of: (a) field reassignment is forbidden on `monotone`/`antitone` fields (compile-time error); (b) field reassignment is allowed but the new value's qualifier must be a subtype of the field's qualifier (`monotone Vec<T>` field can be reassigned only with a `monotone Vec<T>` value); (c) fields cannot carry qualifiers (only `let` bindings and parameters can). Option (c) is simplest but loses expressiveness. The current draft is silent.

### CR-11 — `class Component { fn render(self) -> RenderGraph }` contract is not specified in either fine draft

- **Severity:** Major
- **Category:** Missing requirement / Contradiction
- **Description:** The integrated fine draft §3.1 (line 88-90) specifies the cross-domain contract: *"The OO model (Gap 1) defines `class Component { fn render(self) -> RenderGraph }`. The render-graph IR (Gap 6) must accept `RenderGraph` objects produced by OO methods."* However:
  - The language fine draft §1 (OO Model) does not mention `RenderGraph` as a type. The `BaseType` enum (`crates/alkalive-compiler/src/ast.rs:423-436`) has no `RenderGraph` variant. The `FnSig.return_type: Option<Type>` cannot express "returns a RenderGraph".
  - The rendering fine draft §6 (Render-Graph IR) does not mention `class Component` or OO integration.
  - The integrated fine draft §3.1 tries to bridge this with a contract, but neither predecessor draft actually implements the contract.
  - The runtime's frame loop (per §3.1) is supposed to "call this method on the root component each frame and pass the result to the renderer" — but neither fine draft specifies how the runtime discovers the root component, how it calls a WASM method on an object (the vtable dispatch from §1.4.4 is for in-WASM calls, not for host→WASM calls), or how the returned `RenderGraph` is extracted from WASM linear memory.
- **Impact:** The central ADR-007 promise ("module objects ARE the render objects") is not actually implemented by either fine draft. The OO model produces class instances; the render-graph IR consumes `RenderGraph` structs; the bridge between them is unspecified.
- **Evidence:** `docs/alkalive-remaining-gaps-fine-draft.md:88-90` (contract); `docs/alkalive-fine-draft-language.md:141-181` (OO grammar — no `RenderGraph` type); `crates/alkalive-compiler/src/ast.rs:423-436` (`BaseType` — no `RenderGraph`); `docs/adr/ADR.md:186` (ADR-007: "module objects ARE the render objects").
- **Recommendation:** Either (a) defer the OO↔render-graph bridge to a future wave and remove the §3.1 contract (acknowledge that Gap 1 produces class instances but does not yet integrate with Gap 6), or (b) add a `BaseType::RenderGraph` variant to the AST and specify the host-side mechanism for calling `Component::render()` and extracting the result. Option (a) is more honest about the current scope.

### CR-12 — `next.config.ts` and `Caddyfile` do not exist in the repo; deployment assumptions unverified

- **Severity:** Major
- **Category:** Hidden migration problem / Missing requirement
- **Description:** The rendering fine draft §8.5.4 (line 2348-2379) proposes adding COOP/COEP headers via a `Caddyfile` (at repo root) and a `next.config.ts`. Neither file exists in the repo (verified via `LS` on `/home/z/my-project/AlkALive` — no `Caddyfile`, no `next.config.ts`). The actual deployment is a static `deploy/index.html` that loads `./pkg/alkalive_runtime_wasm.js` (verified at `deploy/index.html:13`). There is no Next.js, no Caddy, no server-side rendering.
  The fine draft assumes a specific deployment stack (Caddy + Next.js) that the project does not use. The COOP/COEP header configuration would need to be applied to whatever static server hosts `deploy/` — which could be `python -m http.server`, `npx serve`, GitHub Pages, or any other static host.
- **Impact:** The COOP/COEP configuration instructions are non-actionable for the actual deployment. An implementer following the fine draft would create `Caddyfile` and `next.config.ts` files that have no effect on the deployed app.
- **Evidence:** `LS /home/z/my-project/AlkALive` (no `Caddyfile`, no `next.config.ts`); `deploy/index.html:13` (static file load); `docs/alkalive-fine-draft-rendering.md:2348-2379` (Caddyfile + next.config.ts).
- **Recommendation:** Replace the Caddy/Next.js configuration with instructions for the actual deployment stack. If the deployment is "static files via any HTTP server", document the headers as response headers that must be set by whatever server is chosen (with examples for `python -m http.server` is insufficient — it can't set headers; recommend `npx serve --cors` with a `serve.json` or a minimal Caddy/nginx config as an *example*, not the canonical deployment).

### CR-13 — Per-frame `compile()` call in `render_frame` is redundant and a performance risk

- **Severity:** Major (downgraded from Minor — see evidence)
- **Category:** Duplicate responsibilities / Performance risk
- **Description:** The rendering fine draft §6.5.6 (line 737-752) defines `render_frame(&mut self, graph: &RenderGraph, time: f32)` that internally calls `alkalive_render::compile(std::slice::from_ref(graph), &[], &Default::default())` — a topological sort over the graph's passes. The runtime is supposed to use `render_compiled` (§6.5.6, line 757) which takes a pre-compiled `&CompiledGraph`. The runtime's `init_runtime` (§6.5.8, line 915-919) calls `compile()` once at startup and caches the result.
  However, the draft's `render_frame` method exists as a "convenience" that re-compiles every frame. For the Hello World scene (5 passes), the compile cost is O(5) — negligible. But the draft does not specify a per-frame compile-cost budget, and the `compile()` function's merge phase iterates all passes + edges + attachments (per `crates/alkalive-render/src/lib.rs:456-540`). For a 1000-pass scene, this is O(1000) per frame — a real perf cliff.
  The draft §6.6 point 7 (line 966-969) acknowledges the *clone* cost (~600 bytes) but not the *compile* cost.
- **Impact:** Performance regression at scale. The convenience `render_frame` method invites misuse (callers using it instead of `render_compiled`).
- **Evidence:** `docs/alkalive-fine-draft-rendering.md:737-752` (`render_frame` calls `compile()`); `docs/alkalive-fine-draft-rendering.md:915-919` (runtime caches `compiled`); `crates/alkalive-render/src/lib.rs:447-540` (`compile()` does merge + topo sort).
- **Recommendation:** Either (a) remove the convenience `render_frame` method (force callers to manage the `CompiledGraph` cache), or (b) add a per-frame compile-cost budget (e.g. "compile() must complete in <100µs for graphs up to 1000 passes") and a benchmark test. The current draft leaves the perf cliff implicit.

---

## 3. Minor Findings

### CR-14 — `Vec::new()` "expected-type inference" doesn't actually happen

- **Severity:** Minor
- **Category:** Ambiguity
- **Description:** The language fine draft §3.4.5 (line 1078-1087) says `Vec::new()` returns `None` and *"The expected type flows in from the let-binding context."* But the typechecker's `check_expr` (verified at `crates/alkalive-compiler/src/typechecker.rs:347`) returns `Option<Type>` with no expected-type parameter. The `let` binding's declared type comes from `LetDecl.ty` (stored in `TypeEnv`), not from inferring the init expression. So `let v: Vec<i32> = Vec::new();` typechecks because the env stores `v: Vec<i32>` from the declaration, not because `Vec::new()` was inferred.
  The draft's wording ("expected-type inference") suggests a bidirectional inference mechanism that does not exist. §3.9 (line 1167-1169) acknowledges this as "expected-type inference, not bottom-up inference" but the algorithm in §3.4.3-3.4.5 doesn't implement it — it just returns `None` and relies on the env.
- **Evidence:** `docs/alkalive-fine-draft-language.md:1078-1087`; `crates/alkalive-compiler/src/typechecker.rs:347-445` (`check_expr` signature has no expected-type param).
- **Recommendation:** Reword §3.4.5 to say: "`Vec::new()` returns `None` (no inferable element type). The let-binding's declared type is used instead — the typechecker stores `v: Vec<i32>` from the `let` declaration, and downstream uses of `v` are checked against that type." This is accurate to the implemented algorithm.

### CR-15 — `Attribute` on classes: parser integration unspecified

- **Severity:** Minor
- **Category:** Missing requirement / Ambiguity
- **Description:** The proposed `ClassDecl` struct (§1.4.2, line 209-220) includes `attrs: Vec<Attribute>`. The `Attribute` type exists (`crates/alkalive-compiler/src/ast.rs:60-71`) and is parsed for nodes, scenes, and lets. But the fine draft §1.5 (Compiler implications, line 388) says only "New keywords: `class`, `pub`, `priv`, `self`, `Self`, `new`" — it does not mention extending `parse_leading_attributes` to accept `@ident` before `class`. The semantics of `@monotone` on a class are also unspecified (does it apply to the class itself? to all fields? to collection-typed fields only?).
- **Evidence:** `docs/alkalive-fine-draft-language.md:209-220` (`ClassDecl.attrs`); `docs/alkalive-fine-draft-language.md:388` (lexer changes — no attribute mention); `crates/alkalive-compiler/src/ast.rs:60-71` (`Attribute` type).
- **Recommendation:** Either (a) specify that `parse_class` calls `parse_leading_attributes` first (consistent with `parse_fn`/`parse_let`), and that `@monotone` on a class is a parse error (qualifiers apply to fields, not classes), or (b) remove `attrs` from `ClassDecl` if class-level attributes are not supported in this wave.

### CR-16 — Method-name-based classification vs class methods: name collision risk

- **Severity:** Minor
- **Category:** Duplicate responsibilities / Ambiguity
- **Description:** The typechecker's `check_method_op` (`typechecker.rs:449-477`) classifies methods by name: `push`/`extend`/`insert`/`append` are grow ops, `remove`/`truncate`/`clear`/`swap_remove`/`drain` are shrink ops. This is method-name-based, not type-aware. If a user class defines `fn push(self, x: i32)` (a non-Vec method), the typechecker would not fire `check_method_op` (because the receiver isn't `Vec<T>`) — but the name `push` is still in the grow-ops list. Gap 3 §3.4.4 proposes `class_method_return_type` for class-typed receivers, which bypasses `check_method_op`. So the name collision is benign for soundness, but could confuse readers: "why is `push` classified as a grow op if it's a user-defined method?"
- **Evidence:** `crates/alkalive-compiler/src/typechecker.rs:136-139` (`GROW_OPS`/`SHRINK_OPS`); `crates/alkalive-compiler/src/typechecker.rs:449-477` (`check_method_op`); `docs/alkalive-fine-draft-language.md:1030-1057` (Gap 3 §3.4.4 `MethodCall` checking).
- **Recommendation:** Add a comment in `check_method_op` clarifying that the classification is only consulted for `Vec<T>` receivers; user-class methods are checked by `class_method_return_type` (Gap 1) and are not subject to monotonicity rules. Document this in the language fine draft §3.4.4.

### CR-17 — `__alk_alloc` host import not in §5.4.1's ABI table

- **Severity:** Minor
- **Category:** Missing requirement
- **Description:** The language fine draft §1.4.3 (line 328-332) specifies `__alk_alloc(size: i32) -> i32` as a host function for object allocation. §6.2 (line 1848-1850) says `__alk_alloc` is added to the same `host_imports` list as the `vec_*` functions. But the import-section ABI table in §5.4.1 (line 1521-1531) enumerates only 9 functions (`vec_new` through `vec_get`) — `__alk_alloc` is not listed. §5.10 Open Question 1 (line 1792-1794) asks whether to add `vec_set` — but doesn't mention `__alk_alloc`.
- **Evidence:** `docs/alkalive-fine-draft-language.md:328-332` (`__alk_alloc`); `docs/alkalive-fine-draft-language.md:1521-1531` (ABI table — no `__alk_alloc`); `docs/alkalive-fine-draft-language.md:1848-1850` (cross-gap contract mentions it).
- **Recommendation:** Add `__alk_alloc(size: i32) -> i32` to the §5.4.1 ABI table (making it 10 host imports) when Gap 1 lands, or explicitly note in §1.4.3 that Gap 1 extends the import section by one entry. The current draft splits the responsibility across two sections, risking an implementer missing it.

### CR-18 — `Stmt::Assign` doesn't specify compound assignment or chained field access

- **Severity:** Minor
- **Category:** Missing error behaviour / Ambiguity
- **Description:** The proposed `Stmt::Assign { target: Expr, value: Expr, ... }` (§1.4.2, line 296-302) takes a `target: Expr` that "must be `Expr::Field`". The grammar (§1.4.1, line 177-180) specifies `FieldAccess := Expr '.' Ident` and `ObjectLiteral := 'Self' '{' ... '}'`. But:
  - Compound assignment (`self.counter += 1`) is not mentioned — is it desugared to `self.counter = self.counter + 1`, or is it a parse error?
  - Chained field access (`a.b.c = 5`) — is the target `Expr::Field { receiver: Expr::Field { receiver: a, field: "b" }, field: "c" }`? The grammar allows it, but the codegen (§1.5, line 392) says "compiles to `local.get obj; <value>; i32.store offset=<field_offset>`" — this assumes a single `i32.load` to get the receiver. For `a.b.c`, you'd need `local.get a; i32.load offset=<b_offset>; i32.store offset=<c_offset>` (load `a.b`, then store `c` into it).
- **Evidence:** `docs/alkalive-fine-draft-language.md:296-302` (`Stmt::Assign`); `docs/alkalive-fine-draft-language.md:177-180` (grammar); `docs/alkalive-fine-draft-language.md:392` (codegen — single-level).
- **Recommendation:** Either (a) specify that compound assignment is a parse error in this wave (defer to future), and that chained field assignment is supported via recursive `Expr::Field` receivers (with updated codegen), or (b) restrict `Stmt::Assign.target` to `Expr::Field` with a `Var` receiver only (no nested fields). The current draft is silent on both.

### CR-19 — Gap 8 gates worker path on COOP/COEP even though the first cut uses `postMessage` (no SAB)

- **Severity:** Minor
- **Category:** Unnecessary complexity / Ambiguity
- **Description:** The rendering fine draft §8.5.4 (line 2396-2417) defines `should_use_render_worker()` which checks `is_cross_origin_isolated()` (COOP/COEP) and falls back to single-threaded if false. But §8.7 (line 2524-2528) acknowledges: *"COOP/COEP headers required for `SharedArrayBuffer`. Without them, the worker can still be spawned (Web Workers don't require cross-origin isolation), but `SharedArrayBuffer` is unavailable. The first cut uses `postMessage` (no SAB); the future SAB path requires the headers."*
  So the first cut of Gap 8 doesn't actually need COOP/COEP — but `should_use_render_worker()` gates on it anyway. This means the worker path is unavailable unless the deployment sets COOP/COEP headers, even though the first cut would work without them via `postMessage`.
- **Evidence:** `docs/alkalive-fine-draft-rendering.md:2396-2417` (`should_use_render_worker` checks `is_cross_origin_isolated`); `docs/alkalive-fine-draft-rendering.md:2524-2528` (acknowledgement that `postMessage` doesn't need COOP/COEP).
- **Recommendation:** Remove the `is_cross_origin_isolated()` check from `should_use_render_worker()` for the first cut (Gap 8 v1). Re-add it when the SAB path (§8.5.5) is implemented. This unlocks the worker path on more deployments without changing the design.

### CR-20 — `monotone`/`antitone` field cannot be passed to functions expecting `unrestricted Vec<T>`

- **Severity:** Minor
- **Category:** Missing requirement / Ambiguity
- **Description:** Per ADR-027 Phase 2 (`docs/adr/ADR.md:256-257`): *"monotone / antitone are not subtypes of unrestricted (a qualified value cannot escape to a context that might violate its invariant)."* The `type_is_subtype` function (`typechecker.rs:179-195`) enforces this: `monotone Vec<T>` is not a subtype of `unrestricted Vec<T>`.
  If a class has a `monotone Vec<i32>` field (per CR-10), then `self.items` has type `monotone Vec<i32>`. Passing `self.items` to a function `fn process(v: Vec<i32>)` (unrestricted parameter) would be a type error — `monotone` is not a subtype of `unrestricted`.
  This means a class with a `monotone Vec<T>` field cannot pass that field to any function that takes `Vec<T>` (unrestricted). The field is usable only within the class's own methods (which can call `push` etc. on it directly).
- **Impact:** Usability regression. The monotonicity guarantee is sound, but the ergonomics are poor — users would avoid `monotone` fields because they can't be passed to helper functions.
- **Evidence:** `docs/adr/ADR.md:256-257` (qualifier subtyping); `crates/alkalive-compiler/src/typechecker.rs:179-195` (`type_is_subtype`); `docs/alkalive-fine-draft-language.md:222-230` (`FieldDecl.ty: Type` — includes qualifier).
- **Recommendation:** Either (a) document this as an intentional trade-off (monotonicity is a strong guarantee; users who need to pass the field should declare the parameter as `monotone Vec<T>`), or (b) introduce a "read-only view" mechanism (future wave) that allows unrestricted read access without write access. The current draft doesn't acknowledge the ergonomic cost.

### CR-21 — `compile()` signature change is undocumented

- **Severity:** Minor
- **Category:** Hidden migration problem
- **Description:** The rendering fine draft §6.5.6 (line 738-742) calls `alkalive_render::compile(std::slice::from_ref(graph), &[], &Default::default())`. The actual `compile()` signature (verified at `crates/alkalive-render/src/lib.rs:447-451`) is `compile(graphs: &[RenderGraph], dirty: &[DirtyRect], depth: &DepthBuffer) -> Result<CompiledGraph, CompileError>`. The third argument is `&DepthBuffer` (which implements `Default` — verified at `crates/alkalive-render/src/lib.rs:422-423`). The fine draft's `&Default::default()` works because `DepthBuffer: Default`, but the draft doesn't document that the third arg is `DepthBuffer` or that the call site relies on `DepthBuffer::default()`.
- **Evidence:** `docs/alkalive-fine-draft-rendering.md:738-742`; `crates/alkalive-render/src/lib.rs:447-451, 422-423`.
- **Recommendation:** Add a note in §6.5.6: "The third argument is `&DepthBuffer` (currently `Default::default()` — a placeholder until the occlusion-cull pass is implemented per W5-T3)."

### CR-22 — `CompileError::CycleDetected` in fine draft vs `CompileError::BarrierCycle` in actual code

- **Severity:** Minor
- **Category:** Missing error behaviour (naming mismatch)
- **Description:** The rendering fine draft §6.8 (line 992) lists `CompileError::CycleDetected` as an error class. The actual enum (verified at `crates/alkalive-render/src/lib.rs:534, 823-829`) has `CompileError::BarrierCycle` — there is no `CycleDetected` variant. The draft's error-handling table would not compile against the actual enum.
- **Evidence:** `docs/alkalive-fine-draft-rendering.md:992` (`CycleDetected`); `crates/alkalive-render/src/lib.rs:534, 823-829` (`BarrierCycle`).
- **Recommendation:** Rename the draft's `CycleDetected` to `BarrierCycle` to match the actual enum.

### CR-23 — `RenderPass` naming collision between `alkalive-compiler::schedule` and `alkalive-render`

- **Severity:** Minor
- **Category:** Ambiguity / Duplicate responsibilities
- **Description:** The name `RenderPass` is used in two different crates with two different structs:
  - `alkalive_compiler::schedule::RenderPass` (`crates/alkalive-compiler/src/schedule.rs:56-68`): fields `node_indices: Vec<usize>`, `shader: ShaderId`, `batching: BatchingStrategy`, `rotation: bool`, `kind: PassKind`.
  - `alkalive_render::RenderPass` (`crates/alkalive-render/src/lib.rs:200-213`): fields `id: PassId`, `kind: PassType`, `color_attachments`, `depth_stencil`, `draw_calls`, `dependencies`.
  The rendering fine draft §6.5.5 `schedule_to_render_graph` (line 578-614) converts the former to the latter. Both are called `RenderPass`. The draft refers to "the existing `RenderPass` type" without disambiguating.
- **Evidence:** `crates/alkalive-compiler/src/schedule.rs:56`; `crates/alkalive-render/src/lib.rs:200`; `docs/alkalive-fine-draft-rendering.md:296-307` (references the latter but doesn't note the collision).
- **Recommendation:** Disambiguate in the fine draft: refer to `alkalive_compiler::schedule::RenderPass` as "the author-facing schedule pass" and `alkalive_render::RenderPass` as "the GPU-layer render-graph pass". Consider renaming one in a future cleanup (e.g. `SchedulePass` vs `RenderPass`).

### CR-24 — COOP/COEP is a deployment breaking change for existing `deploy/index.html` consumers

- **Severity:** Minor
- **Category:** Hidden migration problem
- **Description:** The existing `deploy/index.html` (verified — 23 lines, no COOP/COEP headers) is served as a static file. Adding COOP/COEP headers (per §8.5.4) requires the serving infrastructure to set HTTP response headers. If AlkALive is embedded in a third-party page (iframe) or served from a static host that doesn't allow header configuration (e.g. GitHub Pages for raw HTML), the COOP/COEP requirement breaks the deployment.
  The fine draft §8.5.4 (line 2381-2417) provides a fallback (`should_use_render_worker()` returns false → single-threaded), but this means the worker path is opt-in via deployment configuration — not a transparent upgrade.
- **Evidence:** `deploy/index.html` (no headers); `docs/alkalive-fine-draft-rendering.md:2381-2417` (fallback path).
- **Recommendation:** Document explicitly in §8.5.4 that COOP/COEP is a deployment-side breaking change and provide a migration checklist for existing deployment hosts. The fallback path ensures correctness but loses the perf benefit.

### CR-25 — No binary-size or startup-time budget specified for low-end hardware

- **Severity:** Minor
- **Category:** Performance risk
- **Description:** The rendering fine draft §7.11 R7.3 (line 1844-1848) acknowledges WASM binary grows to 1.4-1.6 MB post-Gap-7. §8.11 R8.2 (line 2604-2608) acknowledges worker startup adds 100-300 ms. ADR-017 (`docs/adr/ADR.md:600`) requires "streaming-compile" — the WASM binary must be decoded while downloading. A 1.6 MB binary on a 3G connection (1.5 Mbps) takes ~8 seconds to download. The fine draft doesn't specify a binary-size budget, a startup-time budget, or a target hardware profile (e.g. "must start in <3 s on a 2020 mid-range Android phone over 4G").
- **Evidence:** `docs/alkalive-fine-draft-rendering.md:1844-1848, 2604-2608`; `docs/adr/ADR.md:600-611` (ADR-017).
- **Recommendation:** Add a "Performance Budget" section to the integrated fine draft specifying: max WASM binary size, max startup time, target hardware profile, and a benchmark test that enforces the budget in CI.

### CR-26 — `compile()`'s `dirty` parameter is ignored; ADR-025 integration is incomplete

- **Severity:** Minor
- **Category:** Missing requirement
- **Description:** The rendering fine draft §6.6 point 2 (line 940-942) acknowledges: *"the dirty-pass info is plumbed through `compile()`'s `dirty: &[DirtyRect]` parameter (currently ignored — line 454 of `alkalive-render`)"*. The `compile()` function (verified at `crates/alkalive-render/src/lib.rs:454`) does `let _ = (dirty, depth);`. So even after Gap 6 lands, the dirty-pass fast path (ADR-025) is non-functional at the `compile()` level. The draft treats this as "future work" but ADR-025 is already implemented (per `docs/technical-specification.md:746-754`).
- **Evidence:** `docs/alkalive-fine-draft-rendering.md:940-942`; `crates/alkalive-render/src/lib.rs:454`; `docs/technical-specification.md:746-754` (ADR-025 implemented).
- **Recommendation:** Either (a) implement the `dirty` parameter in Gap 6 (the `compile()` function skips passes whose dirty rects don't intersect the pass's attachments), or (b) explicitly state in §6.6 that ADR-025's dirty-pass fast path is non-functional after Gap 6 and will be re-enabled in a future wave. The current "plumbed through but ignored" framing is misleading.

### CR-27 — `lower_pass_kind` emits placeholder bounds `(0,0,0,0)` for `DrawRect`; IR is not self-contained

- **Severity:** Minor
- **Category:** Missing requirement / Missing testability
- **Description:** The rendering fine draft §6.5.5 (line 643-657) acknowledges: *"For the lowering, we emit a placeholder bounds (0,0,0,0); the renderer overwrites it from its cached `input_field_bounds` field at draw time. This is a known wart."* The `DrawCallKind::DrawRect { bounds, color }` field's `bounds` is always `(0,0,0,0)` in the IR. The renderer's `execute_draw_call` (§6.5.6, line 816) calls `self.real_rect_bounds(bounds)` to get the real bounds.
  This violates ADR-001's "render-graph IR is the atomic rendering primitive" — the IR is not self-contained; it depends on renderer-side cached state. Testing the lowering in isolation (without the renderer) would show all rects as `(0,0,0,0)`.
- **Evidence:** `docs/alkalive-fine-draft-rendering.md:643-657` (placeholder bounds); `docs/alkalive-fine-draft-rendering.md:816` (renderer overwrites); `docs/adr/ADR.md:55` (ADR-001: render-graph IR is atomic).
- **Recommendation:** The draft already acknowledges this and attributes it to ADR-004 (layout) not being implemented. Acceptable as a known wart, but add a `#[test]` that asserts the placeholder bounds are `(0,0,0,0)` so the wart is visible in the test suite until ADR-004 lands.

### CR-28 — Capability vocabulary of 7 is under-justified

- **Severity:** Minor
- **Category:** Missing requirement
- **Description:** The language fine draft §2.4.1 (line 595) proposes 7 capabilities: `Render, Gpu, Net, Fs, Time, Rand, Ipc`. ADR-018 (`docs/adr/ADR.md:621`) mentions "capability-sandboxed least-privilege grants" but doesn't enumerate them. The fine draft says "Rationale: a closed vocab is auditable" but doesn't justify *these 7*. Notably:
  - `Render` vs `Gpu` — what's the distinction? `Render` grants text-drawing; `Gpu` grants raw GPU access?
  - `Compute` (ADR-006's compute passes) — not in the list.
  - `Text` (HarfRust text shaping, ADR-022) — not in the list.
  - `Worker` (spawning on-demand workers, ADR-021) — not in the list.
- **Evidence:** `docs/alkalive-fine-draft-language.md:595`; `docs/adr/ADR.md:621` (ADR-018).
- **Recommendation:** Add a table mapping each capability to the ADR that requires it, and clarify `Render` vs `Gpu`. If `Compute`/`Text`/`Worker` are folded into existing capabilities (e.g. `Gpu` covers compute, `Render` covers text), document the folding.

### CR-29 — Dead `wgpu-backend = []` feature kept for "backward compat" with no external consumers

- **Severity:** Info
- **Category:** Unnecessary complexity
- **Description:** The rendering fine draft §7.5.1 (line 1346-1348) keeps the existing `wgpu-backend = []` feature: *"Kept for backward compat with any external consumers that probe the feature."* But AlkALive is a new project with no external consumers (the workspace is self-contained). Keeping a dead feature is confusing.
- **Evidence:** `docs/alkalive-fine-draft-rendering.md:1346-1348`; `crates/alkalive-backend-wgpu/Cargo.toml` (the `wgpu-backend` feature exists).
- **Recommendation:** Remove the `wgpu-backend` feature in the same PR that adds the real `wgpu` dependency. If external consumers appear later, they can probe for `wgpu = "23"` in `Cargo.toml` directly.

### CR-30 — `lower_pass_kind` takes `algorithm: &AlgorithmIR` but doesn't use it (dead parameter)

- **Severity:** Info
- **Category:** Unnecessary complexity
- **Description:** The rendering fine draft §6.5.5 (line 632-689) defines `fn lower_pass_kind(kind: PassKind, scene: &TextSceneData, algorithm: &AlgorithmIR) -> DrawCallKind`. The function body (line 638-688) reads `scene.background_normalized()`, `scene.text_color`, `scene.rotation_speed`, `scene.input_text` — but never reads `algorithm`. The parameter is dead.
- **Evidence:** `docs/alkalive-fine-draft-rendering.md:632-689`.
- **Recommendation:** Remove the `algorithm` parameter, or document that it's reserved for future use (e.g. when ADR-004 layout consumes algorithm nodes).

---

## 4. Info Findings

### CR-31 — `DrawCallKind::DrawCustom { vertices: Vec<u8>, uniforms: Vec<u8> }` lives in safe crate but consumption requires `unsafe`

- **Severity:** Info
- **Category:** Hidden migration problem (acceptable)
- **Description:** The rendering fine draft §6.5.3 (line 401-412) defines `DrawCallKind::DrawCustom { vertices: Vec<u8>, uniforms: Vec<u8> }` in `alkalive-render` (which is `#![forbid(unsafe_code)]`). The renderer (`alkalive-backend-wgpu`, which is `#![allow(unsafe_code)]`) would need to interpret these raw bytes as vertex/uniform data via `bytemuck::cast_slice` or similar. This is fine — the safe crate defines the data shape, the unsafe crate does the casting — but it's a subtle invariant worth noting.
- **Evidence:** `docs/alkalive-fine-draft-rendering.md:401-412`; `crates/alkalive-render/src/lib.rs` (`#![forbid(unsafe_code)]`); `crates/alkalive-backend-wgpu/src/lib.rs:44` (`#![allow(unsafe_code)]`).
- **Recommendation:** Add a comment in `alkalive-render` near `DrawCallKind::DrawCustom` noting that consumption requires `unsafe` casting in the backend.

### CR-32 — `call_indirect` is MVP-compatible (no reference-types proposal required)

- **Severity:** Info
- **Category:** N/A (verified, no issue)
- **Description:** The language fine draft §1.4.4 (line 353-354) says *"The compiler emits the simpler `call_indirect` form (no reference-types proposal required)"*. Verified: `call_indirect` with the default funcref table is in the WASM MVP. The `elem` segment with function indices (not funcrefs) is also MVP. The proposed vtable design (one table, `elem` segment seeding function indices, `call_indirect` dispatch) is MVP-compatible. No issue.
- **Evidence:** WASM spec (MVP includes `call_indirect` and `elem` segments with function indices).
- **Recommendation:** No action needed. (Listed for completeness — the review verified this is not a finding.)

### CR-33 — Wave/phase terminology is inconsistent between fine drafts

- **Severity:** Info
- **Category:** Ambiguity (documentation)
- **Description:** The language fine draft uses "Wave 1" / "Wave 10-14" (§7, line 1882-1889). The rendering fine draft uses "Wave 2" / "Wave 2a/2b/2c" (§6.1, line 2897-2902). The integrated fine draft uses "Phase A/B/C/D/E" (§2.4, line 62-80). The three documents use three different naming schemes for the same concept (implementation sequencing).
- **Evidence:** `docs/alkalive-fine-draft-language.md:1882-1889`; `docs/alkalive-fine-draft-rendering.md:2897-2902`; `docs/alkalive-remaining-gaps-fine-draft.md:62-80`.
- **Recommendation:** Pick one terminology (recommend "Phase A/B/C/D/E" from the integrated draft) and use it consistently across all three documents. The orchestrator should map waves to phases explicitly.

---

## 5. Summary Table

| ID | Severity | Category | Gap | One-line summary |
|----|----------|----------|-----|------------------|
| CR-1 | Critical | Incompatible / Missing | 8 | `RenderGraph` lacks serde derives; worker `postMessage` blocked |
| CR-2 | Major | Contradiction / Migration | 2 | Gap 2's AOT inversion contradicts tech-spec C10/TD8 |
| CR-3 | Major | Circular dependency | 6 | `alkalive-render` ↔ `alkalive-backend-wgpu` cycle; trait under-specified |
| CR-4 | Major | Incompatible / Missing | 6 | `DrawCall` lacks `id`; first iteration can't resolve draw calls |
| CR-5 | Major | Incompatible / Correctness | 7 | wgpu `render_compiled` hardcodes black clear, ignores `DrawCallKind::Clear` |
| CR-6 | Major | Contradiction / Regression | 6 | `render_frame_with_dirty` removed; ADR-025 incremental path regresses |
| CR-7 | Major | Ambiguity / Incompatible | 1 | `call_indirect` vtable semantics ambiguous; "vtable_ptr" is a table index |
| CR-8 | Major | Contradiction / Incompatible | 2 | Tree-shaking after typecheck; unreachable `pub fn` breaks build |
| CR-9 | Major | Missing / Ambiguity | 2 | Tree-shaking doesn't handle virtual dispatch; overrides dropped |
| CR-10 | Major | Missing / Incompatible / Error | 1 | Monotone field assignment semantics unspecified; soundness hole |
| CR-11 | Major | Missing / Contradiction | 1+6 | `class Component::render() -> RenderGraph` contract not implemented |
| CR-12 | Major | Migration / Missing | 8 | `Caddyfile`/`next.config.ts` don't exist; deployment assumptions wrong |
| CR-13 | Major | Duplicate / Perf | 6 | Per-frame `compile()` in `render_frame` redundant; perf cliff at scale |
| CR-14 | Minor | Ambiguity | 3 | `Vec::new()` "expected-type inference" doesn't actually happen |
| CR-15 | Minor | Missing / Ambiguity | 1 | `Attribute` on classes: parser integration unspecified |
| CR-16 | Minor | Duplicate / Ambiguity | 3 | Method-name classification collides with user-class method names |
| CR-17 | Minor | Missing | 1+5 | `__alk_alloc` not in §5.4.1 ABI table |
| CR-18 | Minor | Missing / Ambiguity | 1 | `Stmt::Assign` doesn't specify compound/chained assignment |
| CR-19 | Minor | Unnecessary / Ambiguity | 8 | Worker path gated on COOP/COEP though first cut uses `postMessage` |
| CR-20 | Minor | Missing / Ambiguity | 1 | Monotone field can't be passed to `unrestricted Vec<T>` params |
| CR-21 | Minor | Migration | 6 | `compile()` 3rd arg is `DepthBuffer`; undocumented |
| CR-22 | Minor | Error (naming) | 6 | `CycleDetected` in draft vs `BarrierCycle` in code |
| CR-23 | Minor | Ambiguity / Duplicate | 6 | `RenderPass` naming collision across two crates |
| CR-24 | Minor | Migration | 8 | COOP/COEP is a deployment breaking change |
| CR-25 | Minor | Perf | 7+8 | No binary-size / startup-time budget for low-end hardware |
| CR-26 | Minor | Missing | 6 | `compile()`'s `dirty` parameter ignored; ADR-025 incomplete |
| CR-27 | Minor | Missing / Testability | 6 | `lower_pass_kind` emits placeholder bounds; IR not self-contained |
| CR-28 | Minor | Missing | 2 | Capability vocabulary of 7 under-justified |
| CR-29 | Info | Unnecessary | 7 | Dead `wgpu-backend = []` feature kept |
| CR-30 | Info | Unnecessary | 6 | `lower_pass_kind` has dead `algorithm` parameter |
| CR-31 | Info | Migration | 6+7 | `DrawCustom` bytes require `unsafe` casting in backend |
| CR-32 | Info | N/A | 1 | `call_indirect` is MVP-compatible (verified, no issue) |
| CR-33 | Info | Ambiguity | all | Wave/phase terminology inconsistent across drafts |

---

## 6. Recommendations for the Orchestrator

1. **Block Gap 8 implementation on CR-1.** The serde-derive gap is a hard blocker; resolve it before approving Gap 8's PR.
2. **Block Gap 6 implementation on CR-3, CR-4, CR-5, CR-6.** These four Major findings would produce a broken renderer if not resolved. CR-4 (DrawCall.id) and CR-5 (hardcoded black clear) are correctness bugs; CR-6 (dirty-pass regression) breaks ADR-025.
3. **Block Gap 1 implementation on CR-7, CR-10.** CR-7 (vtable semantics) would cause implementation to stall; CR-10 (monotone field assignment) is a soundness hole.
4. **Block Gap 2 implementation on CR-2, CR-8, CR-9.** CR-2 (architectural inversion) is a Critical-severity architectural change buried in a bullet point; CR-8 and CR-9 (tree-shaking vs typecheck/virtual dispatch) are soundness issues.
5. **Resolve CR-11 (OO↔render-graph contract) before claiming ADR-007 compliance.** The integrated fine draft's §3.1 contract is not implemented by either predecessor.
6. **Address CR-12 (deployment assumptions) before Gap 8.** The Caddy/Next.js configuration is non-actionable for the actual deployment.
7. **The Minor findings (CR-14 through CR-28) can be addressed during implementation** but should be tracked as issues. The orchestrator should assign them to the respective gap implementers.
8. **The Info findings (CR-29 through CR-33) are documentation/cleanup nits.** Address them opportunistically.
9. **Overall:** the fine draft is *architecturally sound* — the gap decomposition, dependency graph, and per-gap designs are coherent and traceable to ADRs. The findings above are implementation-level issues that, if left unaddressed, would cause the implementation waves to stall or produce broken behaviour. None of the findings invalidate the fine draft's overall design; they refine it.

---

## 7. DoD Checklist for this Critical Review

- [x] Critical review saved to `docs/alkalive-fine-draft-critical-review.md`.
- [x] All 11 review categories checked (§0 table).
- [x] Findings documented with severity, evidence, and recommendation (§1-§4).
- [x] Findings cover both fine drafts (language gaps 1-5 + rendering gaps 6-8 + integrated).
- [x] Findings cross-referenced to ADRs and existing implementation (file:line evidence).
- [x] No fixes applied — findings documented only for orchestrator decision.
- [x] Summary table (§5) for orchestrator triage.
- [x] Recommendations (§6) for orchestrator action.

---

*End of critical review.*
