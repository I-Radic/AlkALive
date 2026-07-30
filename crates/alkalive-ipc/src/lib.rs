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
#[derive(Debug, Clone)]
pub struct Blob(());

/// Monotonically increasing worker identifier issued by [`WorkerPool::spawn`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskId(());

/// Snapshot of host GPU capabilities handed to workers.
///
/// Immutable snapshot only — never the `GPUDevice` itself (ADR 003 / ADR 021).
#[derive(Debug, Clone)]
pub struct DeviceCaps(());

/// Monotonic clock shared between main thread and workers for ADR 016
/// trace correlation on the unified author-owned timeline.
#[derive(Debug, Clone, Copy)]
pub struct MonotonicClock(());

/// A point in time read from a [`MonotonicClock`]; used as a deadline
/// argument to non-blocking polls and channel receives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Instant(());

/// Per-frame identifier issued by [`Scheduler::begin_frame`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameId(());

/// Outcome of [`Scheduler::commit`] — the main thread drains pending worker
/// IR at the frame-budget deadline and never blocks on a channel.
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
pub struct SharedArrayBuffer(());

/// Cross-thread state handed to a spawned worker.
///
/// Workers receive `SharedState` and never the `GPUDevice`. The SAB is the
/// IPC + IR staging area; `device_caps` is an immutable capability snapshot;
/// `clock` is the ADR 016 trace-correlation clock.
#[derive(Debug, Clone)]
pub struct SharedState {
    /// IPC + IR staging ring buffer.
    pub sab: SharedArrayBuffer,
    /// Immutable GPU capability snapshot (never the device).
    pub device_caps: DeviceCaps,
    /// ADR 016 trace-correlation clock.
    pub clock: MonotonicClock,
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
        self.queue.push_back(msg);
        Ok(())
    }

    fn try_send(&mut self, msg: T, _deadline: Instant) -> Result<(), ChannelError> {
        if self.closed {
            return Err(ChannelError::Closed);
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
}
