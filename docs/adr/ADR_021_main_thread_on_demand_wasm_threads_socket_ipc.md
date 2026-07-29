# ADR 021: Main Thread + On-Demand WASM Threads with Socket IPC

> **Canonical location:** This ADR is also recorded in [`ADR.md`](ADR.md#adr-021-main-thread--on-demand-wasm-threads-with-socket-ipc). This standalone file is provided for direct linking.

## Context
`Decision_Alternatives_concurrency-scheduling.md` recorded three candidate models (cooperative coroutines / preemptive actor threads / pure single-thread loop) for the WASM UI runtime scheduler and recommended Approach A (cooperative coroutines + retain-mode loop) pending a spike. The project owner has made a non-negotiable choice that resolves this without a spike: a **hybrid not present in the file's A/B/C options** — one main thread plus on-demand WASM threads with socket IPC.

## Decision
Adopt a **main thread + on-demand WASM threads** model. The main thread runs the retain-mode render loop (layout, rendering, hit-testing, input dispatch) and owns the GPUDevice per [ADR 003](ADR.md#adr-003-single-gpudevice-render-thread--sabcoop-coep-compositor). Additional WASM threads are spawned **on demand** for asynchronous tasks (asset decoding, compute, IO). Inter-process communication (IPC) between threads uses **WASM sockets** over `SharedArrayBuffer` (via `wasm-sockets` or a similar mechanism).

## Status
Proposed.

## Consequences
- **Positive:** the main thread stays deterministic for the render loop per [ADR 016](ADR.md#adr-016-unified-author-owned-trace-with-split-determinism); on-demand threads handle async work without polluting the frame timeline; socket IPC is a structured, typed channel (better than ad-hoc shared-memory races).
- **Negative:** `SharedArrayBuffer` still requires COOP/COEP per [ADR 003](ADR.md#adr-003-single-gpudevice-render-thread--sabcoop-coep-compositor) (deployment constraint); on-demand thread spawn cost must be amortized; socket IPC adds a serialization surface to cross-thread data.
- **Cross-references:** [ADR 003](ADR.md#adr-003-single-gpudevice-render-thread--sabcoop-coep-compositor) (compositor threading — the GPUDevice-owner render thread is the persistent main thread or a dedicated non-on-demand worker); [ADR 008](ADR.md#adr-008-statically-typed-moduleoo-language-compiling-to-wasm) (language exposes the thread/IPC primitives); [ADR 016](ADR.md#adr-016-unified-author-owned-trace-with-split-determinism) (per-tick trace determinism preserved on the main thread); [ADR 017](ADR.md#adr-017-compiled-wasm-binary--webgpu-pipeline-precompilation) (binary now bundles thread runtime + IPC shim).

## Confidence
**High.** The owner's choice is non-negotiable and resolves the previously open P4.3 decision point without requiring the recommended spike.
