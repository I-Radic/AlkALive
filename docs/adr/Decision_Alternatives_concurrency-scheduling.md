# Decision Alternatives: Concurrency and Scheduling Model

## Decision Point

Select the concurrency and scheduling model for the WASM UI language runtime — the mechanism by which layout, rendering, input (a11y/IME), and user-authored logic are multiplexed within each frame tick.

## Why Uncertain

The rough draft selected cooperative coroutines fused with a retain-mode loop, but evidence (P4.3 [23,24,48,50]) underdetermines the choice. Each candidate model satisfies the core constraints (determinism, traceability, no data races) yet differs sharply in scheduling semantics, WASM-feature surface area, and authoring ergonomics. No empirical benchmark within this project resolves the trade-off, so the decision remains low-confidence pending a spike.

## Approach A: Unified cooperative coroutines integrated with a retain-mode render loop (single scheduler drives layout/render/a11y/IME each tick)

**Pros.** One scheduler, one priority order, one trace timeline — aligns directly with ADR_016's per-tick observability contract. Deterministic by construction: no preemption points outside scheduler control. Coroutines give authors structured concurrency for animations/streams without exposing raw threads, consistent with ADR_008's safe-language goals. Minimal WASM feature set (no `threads`/`shared`).
**Cons.** Long-running coroutine misbehavior stalls the whole frame; requires cooperative yield discipline or budget enforcement. Single-thread ceiling on throughput. Mismatch with ADR_003 compositor threading if compositing runs off-thread — extra marshalling.

## Approach B: Preemptive WASM threads + message-passing (actor model) over SharedArrayBuffer

**Pros.** Natural fit with ADR_003's multi-threaded compositor; parallel layout/paint feasible. Isolation via actors eliminates shared mutable state, satisfying ADR_008 safety goals differently. Backpressure built into mailboxes.
**Cons.** `SharedArrayBuffer` requires COOP/COEP headers — deployment fragility. Preemption breaks deterministic replay unless scheduling is externally serialized (ADR_016 tension). Higher implementation complexity; larger WASM surface; harder debugging.

## Approach C: Pure retain-mode single-thread loop with explicit async tasks (no coroutines)

**Pros.** Simplest mental model; closest to mainstream retained-mode frameworks. Trivially deterministic and traceable (ADR_016). No coroutine state machine in the runtime. Smallest code surface.
**Cons.** Authors must manually decompose long work into callbacks/promises — ergonomic regression vs. A. No structured cancellation or parent-child task trees. Implicit priority coupling between app logic and system passes unless carefully staged.

## Recommended Approach

**Approach A**, conditional on a 2-week spike measuring (1) frame-budget enforcement cost and (2) ADR_003 compositor-marshalling overhead. A best balances ADR_008 (safe authoring), ADR_016 (single-timeline traceability), and WASM-feature parsimony. Fallback to **C** if the coroutine state-machine proves to exceed the runtime complexity budget; escalate to **B** only if profiling shows single-thread layout/paint as a frame-time bottleneck that A cannot amortize.
