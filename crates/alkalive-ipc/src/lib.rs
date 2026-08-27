//! AlkALive alkalive-ipc crate.
//!
//! Concurrency & IPC trait surface — see `docs/SPECIFICATION.md` §11
//! (Concurrency & IPC). Realises ADR 021 (main thread + on-demand WASM
//! worker threads, socket IPC over `SharedArrayBuffer`) and ADR 003
//! (main-thread canonical `GPUDevice` ownership).
//!
//! Wave 3 skeleton: signatures only; every body is `todo!()`.
//! No cross-crate dependencies; all referenced host types are local
//! placeholders pending the IPC shim (ADR 021).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use core::cell::{Cell, RefCell};
use core::fmt;
use std::collections::VecDeque;

// ============================================================================
// Markers
// ============================================================================

/// Marker trait for types permitted to traverse an [`IPCSocket`].
///
/// Per §11.4, sockets are never `GPUDevice`-aware: only serialisable IR,
/// asset blobs, and command/result enums may cross the channel. Implementors
/// assert at the type level that they own no host GPU handle.
pub trait Serial: fmt::Debug {}

// ============================================================================
// Enums
// ============================================================================

/// Kind of off-frame worker task spawned via [`WorkerPool::spawn`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind {
    /// Asset decode (image, mesh, font, audio).
    AssetDecode,
    /// Pure compute (e.g. particle simulation).
    Compute,
    /// IO (fetch, store, network).
    IO,
    /// HarfRust shaping off the main thread.
    Shape,
}

/// Result of a worker task, propagated across the IPC boundary.
///
/// Worker panic is isolated: a trap resolves the [`TaskHandle`] to
/// `Err(TaskError::Panic)`; the pool reaps the worker and recycles the slot.
#[derive(Debug)]
pub enum TaskError {
    /// Worker trapped; carries the panic payload as a [`Blob`].
    Panic(Blob),
    /// Channel fault: closed / framing / underrun / backpressure.
    Channel(ChannelError),
    /// Handle dropped or deadline exceeded.
    Cancelled,
    /// Asset decode failure.
    Decode(DecodeError),
}

/// Fault on an [`IPCSocket`] ring buffer.
///
/// `Framing` quarantines the suspect ring slot and surfaces in the trace
/// (ADR 016). The render loop never blocks on a channel — `Underrun` and
/// `Backpressure` are non-fatal and resolved by yielding or dropping.
#[derive(Debug)]
pub enum ChannelError {
    /// Peer gone.
    Closed,
    /// Corrupt header / size mismatch; slot quarantined.
    Framing,
    /// Ring empty past deadline.
    Underrun,
    /// Ring full; sender must yield.
    Backpressure,
    /// (De)serialisation failure.
    Serialize(SerialError),
}

/// Poll state mirroring `core::task::Poll`, kept crate-local so the IPC
/// surface does not couple to `std::future`.
#[derive(Debug)]
pub enum Poll<T> {
    /// Value ready.
    Ready(T),
    /// Still pending; retry after the deadline.
    Pending,
}

// ============================================================================
// Structs
// ============================================================================

/// Opaque, owned byte payload used as a panic snapshot in [`TaskError::Panic`].
#[derive(Debug, Clone, Default)]
pub struct Blob(());

/// Monotonically increasing worker identifier issued by [`WorkerPool::spawn`].
///
/// The inner `u64` is exposed so in-process implementations (e.g.
/// [`LocalWorkerPool`]) can mint fresh identifiers without going through the
/// SAB-backed IPC shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskId(pub u64);

/// Snapshot of host GPU capabilities handed to workers.
///
/// Immutable snapshot only — never the `GPUDevice` itself (ADR 003 / ADR 021).
#[derive(Debug, Clone, Default)]
pub struct DeviceCaps(());

/// Monotonic clock shared between main thread and workers for ADR 016
/// trace correlation on the unified author-owned timeline.
#[derive(Debug, Clone, Copy, Default)]
pub struct MonotonicClock(());

/// A point in time read from a [`MonotonicClock`]; used as a deadline
/// argument to non-blocking polls and channel receives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Instant(());

/// Per-frame identifier issued by [`Scheduler::begin_frame`].
///
/// The inner `u64` is exposed so in-process implementations (e.g.
/// [`LocalScheduler`]) can return monotonically increasing frame ids from a
/// local counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameId(pub u64);

/// Outcome of [`Scheduler::commit`] — the main thread drains pending worker
/// IR at the frame-budget deadline and never blocks on a channel.
#[derive(Debug, Clone, Default)]
pub struct FrameResult(());

/// Asset decode error subtype of [`TaskError::Decode`].
#[derive(Debug, Clone)]
pub struct DecodeError(());

/// (De)serialisation error subtype of [`ChannelError::Serialize`].
#[derive(Debug, Clone)]
pub struct SerialError(());

/// Opaque placeholder for the WASM `SharedArrayBuffer`.
///
/// Crate-local stand-in pending the IPC shim (ADR 021). The real type will
/// back the `IPCSocket` ring buffer and IR staging area.
#[derive(Debug, Clone, Default)]
pub struct SharedArrayBuffer(());

/// Cross-thread state handed to a spawned worker.
///
/// Workers receive `SharedState` and never the `GPUDevice`. The SAB is the
/// IPC + IR staging area; `device_caps` is an immutable capability snapshot;
/// `clock` is the ADR 016 trace-correlation clock.
///
/// A [`Default`] implementation is provided so in-process worker pools (e.g.
/// [`LocalWorkerPool`]) can construct a stand-in `SharedState` without the
/// SAB-backed IPC shim. Real workers receive a populated `SharedState` from
/// the host once the ADR 021 IPC shim lands.
#[derive(Debug, Clone, Default)]
pub struct SharedState {
    /// IPC + IR staging ring buffer.
    pub sab: SharedArrayBuffer,
    /// Immutable GPU capability snapshot (never the device).
    pub device_caps: DeviceCaps,
    /// ADR 016 trace-correlation clock.
    pub clock: MonotonicClock,
}

// ----------------------------------------------------------------------------
// Wave-F: constructors for the opaque host placeholders
// ----------------------------------------------------------------------------
//
// The opaque host types (`Blob`, `DeviceCaps`, `MonotonicClock`,
// `SharedArrayBuffer`, `FrameResult`) wrap a private `()` pending the ADR 021
// IPC shim. In-process implementations (LocalWorkerPool / LocalScheduler) and
// their tests need to mint default values, so each derives `Default` (the
// private `()` stays private — only the derive, which is in-module, can
// construct it).

impl SharedState {
    /// Construct a default [`SharedState`] for in-process workers.
    ///
    /// Equivalent to [`SharedState::default`]; provided for call-site
    /// readability.
    pub fn new() -> Self {
        Self::default()
    }
}

// ============================================================================
// Traits
// ============================================================================

/// Handle to a spawned worker task; polled from the main thread.
///
/// A handle resolves to `Err(TaskError::Panic)` if the worker traps,
/// `Err(TaskError::Cancelled)` on explicit cancel or deadline exceed, or
/// `Ok(value)` on success. Polling never blocks the render loop.
pub trait TaskHandle<T> {
    /// Identifier matching the [`WorkerPool::spawn`] return.
    fn id(&self) -> TaskId;
    /// Non-blocking poll against `deadline`; never blocks the render loop.
    fn poll(&self, deadline: Instant) -> Poll<Result<T, TaskError>>;
    /// Signal cancellation; the worker resolves to
    /// [`TaskError::Cancelled`] if not already done.
    fn cancel(&self) -> Result<(), ChannelError>;
}

/// On-demand worker pool (ADR 021).
///
/// Workers never acquire `GPUDevice` and never mutate render-path state.
/// A panicked worker is reaped and its slot recycled; the main thread's
/// frame timeline is unaffected.
pub trait WorkerPool {
    /// Spawn an off-frame task of `kind`; the worker receives
    /// [`SharedState`] only. Returns a polled [`TaskHandle`].
    fn spawn<T, F>(&self, kind: TaskKind, task: F) -> Box<dyn TaskHandle<T>>
    where
        F: FnOnce(SharedState) -> Result<T, TaskError> + 'static,
        T: 'static;
    /// Reap a panicked worker and recycle its slot.
    fn reap(&self, id: TaskId);
    /// Advisory pool size; grows on demand.
    fn pool_size_hint(&self) -> usize;
    /// Drain and shut down every worker.
    fn shutdown(&self) -> Result<(), ChannelError>;
}

/// Main-thread scheduler driving the deterministic frame timeline.
///
/// `begin_frame` / `commit` pairs are vsync-bounded. Workers run off-frame;
/// IR merges occur only at `commit` points, where the scheduler drains
/// pending worker IR via `try_recv` against the frame-budget deadline.
/// Stale or late IR is dropped — cadence is preserved over worker liveness.
pub trait Scheduler {
    /// Open a new frame at `now`; returns its [`FrameId`].
    fn begin_frame(&self, now: Instant) -> FrameId;
    /// Drain pending worker IR against the frame-budget deadline; never
    /// blocks on a channel. Late or partial IR is discarded.
    fn commit(&self, frame: FrameId) -> FrameResult;
    /// Delegate a spawn to the underlying [`WorkerPool`].
    fn spawn<T, F>(&self, kind: TaskKind, task: F) -> Box<dyn TaskHandle<T>>
    where
        F: FnOnce(SharedState) -> Result<T, TaskError> + 'static,
        T: 'static;
}

/// Typed, serialised, backpressure-aware channel backed by a SAB ring buffer
/// with `Atomics`-based signalling (ADR 021).
///
/// The sole cross-thread primitive. Sockets are never `GPUDevice`-aware:
/// only serialisable IR, asset blobs, and command/result enums traverse
/// them. `send` yields on backpressure; `try_send` / `try_recv` take an
/// explicit deadline and never block past it.
pub trait IPCSocket<T: Serial> {
    /// Send a message, yielding on backpressure.
    fn send(&mut self, msg: T) -> Result<(), ChannelError>;
    /// Send a message, failing if not delivered by `deadline`.
    fn try_send(&mut self, msg: T, deadline: Instant) -> Result<(), ChannelError>;
    /// Block until a message arrives.
    fn recv(&mut self) -> Result<T, ChannelError>;
    /// Non-blocking receive with `deadline`; returns `Ok(None)` on underrun.
    fn try_recv(&mut self, deadline: Instant) -> Result<Option<T>, ChannelError>;
    /// Number of ring slots.
    fn capacity(&self) -> usize;
    /// Close the channel; subsequent sends/recvs return
    /// [`ChannelError::Closed`].
    fn close(&mut self);
}

// ============================================================================
// Wave-3 in-process implementation
// ============================================================================

/// In-process [`IPCSocket`] backed by a `VecDeque<T>`.
///
/// This is the Wave-3 stand-in for the SAB-backed ring buffer (ADR 021).
/// No `SharedArrayBuffer` is involved — the channel is purely in-process,
/// suitable for unit tests, single-threaded hosts, and any caller that
/// needs a deterministic, panic-free channel before the WASM-thread IPC
/// shim lands.
///
/// Semantics (per IMPL-W10b):
/// - `send` / `try_send` push to the back of the deque and return `Ok(())`.
/// - `recv` pops from the front; an empty deque yields
///   [`Err(ChannelError::Underrun)`].
/// - `try_recv` pops from the front; an empty deque yields `Ok(None)`.
/// - `capacity` returns a fixed `1024` (advisory; not enforced — the deque
///   grows unbounded in practice). TODO(Wave N): enforce backpressure
///   (`ChannelError::Backpressure`) once the SAB ring lands.
/// - `close` flips a `closed` flag; every subsequent `send` / `try_send` /
///   `recv` / `try_recv` short-circuits to [`Err(ChannelError::Closed)`],
///   even if messages remain buffered.
///
/// The `deadline` parameters of `try_send` / `try_recv` are accepted for
/// signature compatibility but ignored — the in-process channel is
/// non-blocking, so deadlines are trivially satisfied. Real deadline
/// semantics arrive with the SAB-backed ring.
#[derive(Debug)]
pub struct LocalIPCSocket<T> {
    /// Backing FIFO queue.
    queue: VecDeque<T>,
    /// Set by `close`; short-circuits all subsequent operations.
    closed: bool,
}

impl<T> LocalIPCSocket<T> {
    /// Create a new open `LocalIPCSocket` with an empty queue.
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
            closed: false,
        }
    }

    /// Returns `true` once [`close`](IPCSocket::close) has been called.
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Current number of buffered messages.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Returns `true` if no messages are buffered.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

impl<T> Default for LocalIPCSocket<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Serial> IPCSocket<T> for LocalIPCSocket<T> {
    fn send(&mut self, msg: T) -> Result<(), ChannelError> {
        if self.closed {
            return Err(ChannelError::Closed);
        }
        if self.queue.len() >= self.capacity() {
            return Err(ChannelError::Backpressure);
        }
        self.queue.push_back(msg);
        Ok(())
    }

    fn try_send(&mut self, msg: T, _deadline: Instant) -> Result<(), ChannelError> {
        if self.closed {
            return Err(ChannelError::Closed);
        }
        if self.queue.len() >= self.capacity() {
            return Err(ChannelError::Backpressure);
        }
        self.queue.push_back(msg);
        Ok(())
    }

    fn recv(&mut self) -> Result<T, ChannelError> {
        if self.closed {
            return Err(ChannelError::Closed);
        }
        match self.queue.pop_front() {
            Some(msg) => Ok(msg),
            None => Err(ChannelError::Underrun),
        }
    }

    fn try_recv(&mut self, _deadline: Instant) -> Result<Option<T>, ChannelError> {
        if self.closed {
            return Err(ChannelError::Closed);
        }
        Ok(self.queue.pop_front())
    }

    fn capacity(&self) -> usize {
        1024
    }

    fn close(&mut self) {
        self.closed = true;
    }
}

// ============================================================================
// Wave-F in-process worker pool + scheduler
// ============================================================================
//
// Real WASM worker threads (ADR 021) cannot run inside a host unit test or a
// single-threaded dev host. The types below provide a synchronous, in-process
// implementation of [`WorkerPool`] / [`Scheduler`] / [`TaskHandle`] so that
// the rest of the stack can exercise the spawn → poll → commit surface before
// the SAB-backed IPC shim lands.
//
// Semantics (Gap H1):
// - `LocalWorkerPool::spawn` runs the task **synchronously** on the calling
//   thread, hands the worker a default [`SharedState`], and returns a
//   [`LocalTaskHandle`] preloaded with the result.
// - `LocalTaskHandle::poll` returns `Ready(result)` on the first call and
//   `Pending` thereafter (the result is consumed via `Option::take`).
// - `LocalTaskHandle::cancel` flips a flag and returns `Ok(())`; since the
//   task already ran synchronously, cancellation cannot preempt it — the
//   flag is recorded for observability but the stored result is still
//   returned on the next poll. Real preemption arrives with the SAB-backed
//   workers.
// - `LocalWorkerPool::reap` / `shutdown` are no-ops (no worker threads to
//   recycle or drain) and `pool_size_hint` returns `1`.
// - `LocalScheduler::begin_frame` returns monotonically increasing
//   [`FrameId`]s from an internal counter; `commit` returns the default
//   [`FrameResult`]; `spawn` delegates to an internal [`LocalWorkerPool`].

/// In-process [`TaskHandle`] backed by a precomputed result.
///
/// Created by [`LocalWorkerPool::spawn`] after running the task synchronously.
/// The first [`TaskHandle::poll`] consumes the stored result via `Option::take`
/// and returns [`Poll::Ready`]; subsequent polls return [`Poll::Pending`].
/// `cancel` flips an internal flag (visible to debug formatters) but cannot
/// preempt a task that already ran synchronously.
///
/// Interior mutability (`RefCell` / `Cell`) lets the trait's `&self` methods
/// mutate state. The handle is therefore `!Sync` — it is meant to be polled
/// from a single (main) thread, matching the ADR 021 main-thread ownership
/// model.
pub struct LocalTaskHandle<T> {
    /// Identifier minted by the pool at spawn time.
    id: TaskId,
    /// Precomputed task result; consumed by the first `poll`.
    result: RefCell<Option<Result<T, TaskError>>>,
    /// Set by `cancel`; observability-only (synchronous tasks cannot be
    /// preempted).
    cancelled: Cell<bool>,
}

impl<T> LocalTaskHandle<T> {
    /// Construct a handle preloaded with `result` and the given `id`.
    ///
    /// Normally only called by [`LocalWorkerPool::spawn`]; exposed `pub` so
    /// host-side test harnesses can mint handles directly.
    pub fn new(id: TaskId, result: Result<T, TaskError>) -> Self {
        Self {
            id,
            result: RefCell::new(Some(result)),
            cancelled: Cell::new(false),
        }
    }

    /// `true` once [`TaskHandle::cancel`] has been called.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.get()
    }
}

impl<T> fmt::Debug for LocalTaskHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Do not require `T: Debug`; surface only whether a result is pending.
        f.debug_struct("LocalTaskHandle")
            .field("id", &self.id)
            .field("has_result", &self.result.borrow().is_some())
            .field("cancelled", &self.cancelled.get())
            .finish()
    }
}

impl<T: 'static> TaskHandle<T> for LocalTaskHandle<T> {
    fn id(&self) -> TaskId {
        self.id
    }

    fn poll(&self, _deadline: Instant) -> Poll<Result<T, TaskError>> {
        match self.result.borrow_mut().take() {
            Some(r) => Poll::Ready(r),
            None => Poll::Pending,
        }
    }

    fn cancel(&self) -> Result<(), ChannelError> {
        self.cancelled.set(true);
        Ok(())
    }
}

/// Synchronous, in-process [`WorkerPool`] (Gap H1).
///
/// `spawn` runs the task **immediately** on the calling thread, hands the
/// worker a default [`SharedState`], and returns a [`LocalTaskHandle`]
/// preloaded with the result. `reap` and `shutdown` are no-ops (there are no
/// worker threads to recycle or drain); `pool_size_hint` returns `1`.
///
/// Suitable for unit tests, single-threaded dev hosts, and any caller that
/// needs the spawn → poll surface before the ADR 021 WASM-thread IPC shim
/// lands. The `next_id` counter is a `Cell<u64>`, so the pool is `!Sync`.
#[derive(Debug)]
pub struct LocalWorkerPool {
    /// Next task identifier to mint; monotonically increasing.
    next_id: Cell<u64>,
}

impl LocalWorkerPool {
    /// Create a new pool with the task-id counter at zero.
    pub fn new() -> Self {
        Self {
            next_id: Cell::new(0),
        }
    }
}

impl Default for LocalWorkerPool {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkerPool for LocalWorkerPool {
    fn spawn<T, F>(&self, _kind: TaskKind, task: F) -> Box<dyn TaskHandle<T>>
    where
        F: FnOnce(SharedState) -> Result<T, TaskError> + 'static,
        T: 'static,
    {
        let id = TaskId(self.next_id.get());
        self.next_id.set(id.0.wrapping_add(1));
        // Synchronous execution: run the task inline on the calling thread
        // with a default SharedState. The result is preloaded into the
        // handle, so the first poll resolves immediately.
        let result = task(SharedState::default());
        Box::new(LocalTaskHandle::new(id, result))
    }

    fn reap(&self, _id: TaskId) {
        // No worker threads to reap; synchronous execution recycles the slot
        // implicitly when the handle drops.
    }

    fn pool_size_hint(&self) -> usize {
        1
    }

    fn shutdown(&self) -> Result<(), ChannelError> {
        // Nothing to drain: tasks run inline and the pool owns no threads.
        Ok(())
    }
}

/// Synchronous, in-process [`Scheduler`] (Gap H1).
///
/// `begin_frame` returns monotonically increasing [`FrameId`]s from an
/// internal counter; `commit` returns the default [`FrameResult`]; `spawn`
/// delegates to an internal [`LocalWorkerPool`]. Like the pool, the frame
/// counter is a `Cell<u64>`, so the scheduler is `!Sync`.
#[derive(Debug, Default)]
pub struct LocalScheduler {
    /// Next frame identifier to mint; monotonically increasing.
    next_frame: Cell<u64>,
    /// Backing worker pool for `spawn` delegation.
    pool: LocalWorkerPool,
}

impl LocalScheduler {
    /// Create a new scheduler with both the frame counter and the backing
    /// pool at their defaults.
    pub fn new() -> Self {
        Self::default()
    }
}

impl Scheduler for LocalScheduler {
    fn begin_frame(&self, _now: Instant) -> FrameId {
        let id = FrameId(self.next_frame.get());
        self.next_frame.set(id.0.wrapping_add(1));
        id
    }

    fn commit(&self, _frame: FrameId) -> FrameResult {
        // Synchronous execution drains no channels; return the default
        // FrameResult. Real deadline-bounded IR draining arrives with the
        // SAB-backed scheduler.
        FrameResult::default()
    }

    fn spawn<T, F>(&self, kind: TaskKind, task: F) -> Box<dyn TaskHandle<T>>
    where
        F: FnOnce(SharedState) -> Result<T, TaskError> + 'static,
        T: 'static,
    {
        self.pool.spawn(kind, task)
    }
}

// ============================================================================
// Wave 3 tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal `Serial` message used by the channel tests.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct MockMsg(u32);
    impl Serial for MockMsg {}

    #[test]
    fn send_recv_roundtrip_preserves_fifo_order() {
        let mut sock: LocalIPCSocket<MockMsg> = LocalIPCSocket::new();
        sock.send(MockMsg(1)).unwrap();
        sock.send(MockMsg(2)).unwrap();
        sock.send(MockMsg(3)).unwrap();
        assert_eq!(sock.len(), 3);
        assert_eq!(sock.recv().unwrap(), MockMsg(1));
        assert_eq!(sock.recv().unwrap(), MockMsg(2));
        assert_eq!(sock.recv().unwrap(), MockMsg(3));
        assert!(sock.is_empty());
    }

    #[test]
    fn try_send_and_try_recv_roundtrip() {
        let mut sock: LocalIPCSocket<MockMsg> = LocalIPCSocket::new();
        sock.try_send(MockMsg(42), Instant(())).unwrap();
        assert_eq!(sock.try_recv(Instant(())).unwrap(), Some(MockMsg(42)));
    }

    #[test]
    fn recv_on_empty_returns_underrun() {
        let mut sock: LocalIPCSocket<MockMsg> = LocalIPCSocket::new();
        assert!(matches!(sock.recv(), Err(ChannelError::Underrun)));
    }

    #[test]
    fn try_recv_on_empty_returns_ok_none() {
        let mut sock: LocalIPCSocket<MockMsg> = LocalIPCSocket::new();
        assert_eq!(sock.try_recv(Instant(())).unwrap(), None);
    }

    #[test]
    fn capacity_is_fixed_1024() {
        let sock: LocalIPCSocket<MockMsg> = LocalIPCSocket::new();
        assert_eq!(sock.capacity(), 1024);
    }

    #[test]
    fn send_returns_backpressure_when_capacity_exceeded() {
        let mut sock: LocalIPCSocket<MockMsg> = LocalIPCSocket::new();
        // Fill to capacity.
        for i in 0..1024 {
            sock.send(MockMsg(i)).unwrap();
        }
        // Next `send` should fail with `Backpressure`.
        let result = sock.send(MockMsg(9999));
        assert!(matches!(result, Err(ChannelError::Backpressure)));
        // `try_send` enforces the same bound.
        let result = sock.try_send(MockMsg(8888), Instant(()));
        assert!(matches!(result, Err(ChannelError::Backpressure)));
        // Verify the queue length hasn't exceeded capacity.
        assert_eq!(sock.len(), 1024);
        // Draining one slot re-opens the queue.
        assert_eq!(sock.recv().unwrap(), MockMsg(0));
        sock.send(MockMsg(7777)).expect("send succeeds after drain");
        assert_eq!(sock.len(), 1024);
    }

    #[test]
    fn close_blocks_all_further_operations() {
        let mut sock: LocalIPCSocket<MockMsg> = LocalIPCSocket::new();
        // Buffer a message, then close. Even with a buffered message,
        // every subsequent operation short-circuits to `Closed`.
        sock.send(MockMsg(1)).unwrap();
        sock.close();
        assert!(sock.is_closed());
        assert!(matches!(sock.send(MockMsg(2)), Err(ChannelError::Closed)));
        assert!(matches!(
            sock.try_send(MockMsg(3), Instant(())),
            Err(ChannelError::Closed)
        ));
        assert!(matches!(sock.recv(), Err(ChannelError::Closed)));
        assert!(matches!(
            sock.try_recv(Instant(())),
            Err(ChannelError::Closed)
        ));
    }

    #[test]
    fn close_on_empty_socket_blocks_subsequent_recv() {
        let mut sock: LocalIPCSocket<MockMsg> = LocalIPCSocket::new();
        sock.close();
        // `recv` on a closed empty socket returns `Closed`, not `Underrun`.
        assert!(matches!(sock.recv(), Err(ChannelError::Closed)));
        assert!(matches!(
            sock.try_recv(Instant(())),
            Err(ChannelError::Closed)
        ));
    }

    #[test]
    fn default_creates_open_empty_socket() {
        let sock: LocalIPCSocket<MockMsg> = LocalIPCSocket::default();
        assert!(sock.is_empty());
        assert!(!sock.is_closed());
        assert_eq!(sock.capacity(), 1024);
    }

    // ---- Wave-F: LocalWorkerPool / LocalTaskHandle -----------------------

    #[test]
    fn local_worker_pool_spawn_returns_ok_result() {
        let pool = LocalWorkerPool::new();
        let handle = pool.spawn(TaskKind::Compute, |_state: SharedState| Ok(42));
        // The id is minted from the pool's counter, starting at 0.
        assert_eq!(handle.id(), TaskId(0));
        match handle.poll(Instant(())) {
            Poll::Ready(Ok(v)) => assert_eq!(v, 42),
            other => panic!("expected Poll::Ready(Ok(42)), got {:?}", other),
        }
    }

    #[test]
    fn local_worker_pool_spawn_returns_err_panic() {
        let pool = LocalWorkerPool::new();
        // Annotate the task type so the closure's `Err(...)` arm fixes `T`.
        let handle: Box<dyn TaskHandle<()>> = pool
            .spawn(TaskKind::AssetDecode, |_state: SharedState| {
                Err(TaskError::Panic(Blob::default()))
            });
        match handle.poll(Instant(())) {
            Poll::Ready(Err(TaskError::Panic(_))) => {}
            other => panic!("expected Poll::Ready(Err(Panic(_))), got {:?}", other),
        }
    }

    #[test]
    fn local_task_handle_poll_twice_returns_pending() {
        let pool = LocalWorkerPool::new();
        let handle = pool.spawn(TaskKind::IO, |_state: SharedState| Ok(7));
        // First poll consumes the result.
        assert!(matches!(handle.poll(Instant(())), Poll::Ready(Ok(7))));
        // Second poll finds no result left → Pending.
        assert!(matches!(handle.poll(Instant(())), Poll::Pending));
    }

    #[test]
    fn local_task_handle_cancel_sets_flag_and_returns_ok() {
        // Construct the handle directly so we can inspect `is_cancelled`
        // (the trait object returned by `spawn` does not expose it).
        let handle = LocalTaskHandle::new(TaskId(0), Ok::<u32, TaskError>(1));
        assert!(!handle.is_cancelled());
        handle.cancel().expect("cancel should return Ok(())");
        assert!(handle.is_cancelled());
        // The synchronous result is still delivered: cancel cannot preempt a
        // task that already ran inline.
        assert!(matches!(handle.poll(Instant(())), Poll::Ready(Ok(1))));
    }

    #[test]
    fn local_worker_pool_mints_monotonic_task_ids() {
        let pool = LocalWorkerPool::new();
        let h0 = pool.spawn(TaskKind::Compute, |_s: SharedState| Ok(0));
        let h1 = pool.spawn(TaskKind::Compute, |_s: SharedState| Ok(1));
        let h2 = pool.spawn(TaskKind::Compute, |_s: SharedState| Ok(2));
        assert_eq!(h0.id(), TaskId(0));
        assert_eq!(h1.id(), TaskId(1));
        assert_eq!(h2.id(), TaskId(2));
    }

    #[test]
    fn local_worker_pool_pool_size_hint_is_one() {
        let pool = LocalWorkerPool::new();
        assert_eq!(pool.pool_size_hint(), 1);
    }

    #[test]
    fn local_worker_pool_reap_and_shutdown_are_noops() {
        let pool = LocalWorkerPool::new();
        let h = pool.spawn(TaskKind::Compute, |_s: SharedState| Ok(5));
        // reap must not panic on a known id.
        pool.reap(h.id());
        // shutdown returns Ok(()).
        pool.shutdown().expect("shutdown should return Ok(())");
    }

    #[test]
    fn shared_state_default_is_constructable() {
        // The whole point of the Wave-F constructors: an in-process worker
        // can receive a SharedState without the SAB shim.
        let state = SharedState::default();
        let _ = state.sab;
        let _ = state.device_caps;
        let _ = state.clock;
        // new() is equivalent to default().
        let _ = SharedState::new();
    }

    // ---- Wave-F: LocalScheduler ------------------------------------------

    #[test]
    fn local_scheduler_begin_frame_mints_monotonic_ids() {
        let sched = LocalScheduler::new();
        let f0 = sched.begin_frame(Instant(()));
        let f1 = sched.begin_frame(Instant(()));
        let f2 = sched.begin_frame(Instant(()));
        assert_eq!(f0, FrameId(0));
        assert_eq!(f1, FrameId(1));
        assert_eq!(f2, FrameId(2));
        assert_ne!(f0, f1);
    }

    #[test]
    fn local_scheduler_commit_returns_default_frame_result() {
        let sched = LocalScheduler::new();
        let frame = sched.begin_frame(Instant(()));
        let _result = sched.commit(frame);
    }

    #[test]
    fn local_scheduler_spawn_delegates_to_pool() {
        let sched = LocalScheduler::new();
        let handle = sched.spawn(TaskKind::Compute, |_s: SharedState| Ok(99));
        match handle.poll(Instant(())) {
            Poll::Ready(Ok(v)) => assert_eq!(v, 99),
            other => panic!("expected Poll::Ready(Ok(99)), got {:?}", other),
        }
    }

    #[test]
    fn local_scheduler_default_is_constructable() {
        let sched = LocalScheduler::default();
        let _ = sched.begin_frame(Instant(()));
    }
}
