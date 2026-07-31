//! AlkALive alkalive-core crate.
//!
//! Core language model: modules, interfaces, types, encapsulation boundaries,
//! slots, signals, and the error surface defined in `docs/SPECIFICATION.md`
//! §2.7–2.9. Wave 4 implements the runtime-free semantics: module lifecycle
//! state machine, encapsulation access checks, slot mounting, type soundness
//! queries, and interface slot lookup. Wave 5 replaces the `Signal::emit` /
//! `Signal::subscribe` `todo!()` stubs with a last-known-good value buffer
//! and a unique subscription-id minter; the observer-registry gap closes the
//! dispatch loop — `emit` now fans the value out to every registered
//! `Listener` callback in registration order. Capability-grant verification
//! (ADR 018) and integration with the runtime's observer registry
//! (ADR 014 / §2.6) remain deferred to the runtime-integration wave.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use core::cell::RefCell;
use core::marker::PhantomData;
use core::sync::atomic::{AtomicU64, Ordering};

/// Six-state module lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModuleState {
    /// Not yet loaded.
    Unloaded,
    /// Streaming WASM decode in progress (ADR 017).
    Loading,
    /// Decoded and validated; not yet activated.
    Ready,
    /// Live on the frame timeline.
    Active,
    /// Temporarily paused.
    Suspended,
    /// Terminal.
    Destroyed,
}

/// Visibility scope of a field or declaration (ADR 008).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Visibility {
    /// Accessible only to the owning instance.
    Owner,
    /// Accessible within the declaring module.
    Module,
    /// Accessible via a granted capability.
    Capability,
    /// Universally accessible.
    Public,
}

/// Top-level classification of a [`Type`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeKind {
    /// Primitive scalar.
    Primitive,
    /// User-defined struct.
    Struct,
    /// User-defined enum.
    Enum,
    /// Interface contract reference.
    Interface,
    /// Module reference.
    Module,
}

/// Two-level soundness verdict for a [`Type`] (ADR 009).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Soundness {
    /// Proven sound at the source level.
    Proven,
    /// Explicit `unsafe` attestation; not proven.
    UnsafeAttested,
}

/// Permitted child cardinality for a [`Slot`] (ADR 007 / ADR 014).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cardinality {
    /// Zero or one child.
    Optional,
    /// Exactly one child.
    Single,
    /// Any number of children.
    Many,
}

/// Stable identifier for a module instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModuleId(pub u64);

/// Stable identifier for an interface contract (ADR 014).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InterfaceId(pub u64);

/// Identifier for a capability grant (ADR 018).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CapabilityId(pub u64);

/// Interned symbol name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Symbol(pub u64);

/// Slot name; a type alias for [`Symbol`].
pub type SlotName = Symbol;

/// Opaque trace identifier (ADR 016).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TraceId(pub u64);

/// Opaque runtime value carried by an interface input default.
#[derive(Debug, Clone)]
pub struct Value {
    /// Type-erased encoded payload.
    pub payload: Box<[u8]>,
}

/// Handle returned by [`Slot::mount`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MountHandle(pub u64);

/// Opaque listener registered against a [`Signal`].
///
/// Carries an optional callback (`Fn(&T)`) that [`Signal::emit`] invokes
/// with the emitted value. Listeners built via [`Listener::new`] carry no
/// callback and are silently skipped on emit — useful for tests and for
/// subscribers that only need the minted [`Subscription`] id.
/// [`Listener::with_callback`] installs a callback that fires on every
/// emission in registration order.
///
/// Not `Clone`: the boxed callback (`Box<dyn Fn(&T) + 'static>`) is not
/// cloneable. [`Signal<T>::clone`] therefore starts the clone with an empty
/// listener table; subscribers remain bound to the original instance.
pub struct Listener<T> {
    /// Opaque listener identifier.
    pub id: u64,
    /// Optional callback invoked with the emitted value on each
    /// [`Signal::emit`]. `None` for listeners built via [`Listener::new`].
    #[allow(clippy::type_complexity)] // boxed callback is the canonical shape
    callback: Option<Box<dyn Fn(&T) + 'static>>,
    _marker: PhantomData<T>,
}

impl<T> Listener<T> {
    /// Create a listener with no callback.
    ///
    /// `emit` silently skips listeners with no callback. Use this for tests
    /// or for subscribers that only need the minted [`Subscription`] id (the
    /// runtime's observer registry / capability-grant verification,
    /// ADR 014 / ADR 018, will layer on top in a later wave).
    pub fn new(id: u64) -> Self {
        Self {
            id,
            callback: None,
            _marker: PhantomData,
        }
    }

    /// Create a listener whose `f` is invoked with the emitted value on
    /// every [`Signal::emit`].
    pub fn with_callback(id: u64, f: impl Fn(&T) + 'static) -> Self {
        Self {
            id,
            callback: Some(Box::new(f)),
            _marker: PhantomData,
        }
    }
}

impl<T> core::fmt::Debug for Listener<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Don't try to print the callback (it is not `Debug`); surface
        // whether one is installed so the listener table stays
        // introspectable from `Signal`'s derived `Debug`.
        f.debug_struct("Listener")
            .field("id", &self.id)
            .field("has_callback", &self.callback.is_some())
            .finish()
    }
}

/// Opaque subscription returned by [`Signal::subscribe`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Subscription(pub u64);

/// Opaque reference to a module's exclusively-owned scene-graph subtree (ADR 007).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OwnedSubtreeRef(pub u64);

/// WASM-level structural type signature (ADR 009 level 2).
#[derive(Debug, Clone)]
pub struct WasmTypeSig {
    /// Raw bytes of the structural signature.
    pub bytes: Box<[u8]>,
}

/// Validation failure surfaced by the WASM validator (ADR 009 level 2).
#[derive(Debug, Clone)]
pub struct WasmValidationError {
    /// Human-readable diagnostic.
    pub message: Box<str>,
    /// Byte offset into the WASM binary where validation failed.
    pub offset: Option<u64>,
}

/// A capability grant recorded against a module's imports (ADR 018).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CapabilityGrant {
    /// The granted capability.
    pub capability: CapabilityId,
    /// The module providing the capability.
    pub provider: ModuleId,
}

/// A loaded module instance (ADR 007 / ADR 015).
#[derive(Debug, Clone)]
pub struct Module {
    /// Stable instance identifier.
    pub id: ModuleId,
    /// Interface contract implemented by this module.
    pub iface: InterfaceId,
    /// Current lifecycle state.
    pub state: ModuleState,
    /// Encapsulation boundary.
    pub boundary: EncapsulationBoundary,
    /// Exclusively-owned scene-graph subtree (ADR 007).
    pub scene_graph: OwnedSubtreeRef,
    /// Declared capability imports (ADR 018).
    pub imports: Box<[CapabilityGrant]>,
}

impl Module {
    /// Transition this module to `target`, validating the lifecycle edge.
    ///
    /// Legal edges (SPECIFICATION §2.4):
    /// `Unloaded → Loading → Ready → Active ⇄ Suspended → Destroyed`.
    /// Both `Active → Destroyed` and `Suspended → Destroyed` are permitted
    /// (owner drop or HMR replacement). [`ModuleState::Destroyed`] is
    /// terminal; any transition out of it (or any other unlisted edge)
    /// returns [`ModuleError::IllegalTransition`].
    pub fn transition(&mut self, target: ModuleState) -> Result<(), ModuleError> {
        let allowed = matches!(
            (self.state, target),
            (ModuleState::Unloaded, ModuleState::Loading)
                | (ModuleState::Loading, ModuleState::Ready)
                | (ModuleState::Ready, ModuleState::Active)
                | (ModuleState::Active, ModuleState::Suspended)
                | (ModuleState::Suspended, ModuleState::Active)
                | (ModuleState::Active, ModuleState::Destroyed)
                | (ModuleState::Suspended, ModuleState::Destroyed)
        );
        if !allowed {
            return Err(ModuleError::IllegalTransition(self.state, target));
        }
        self.state = target;
        Ok(())
    }

    /// Mount `child` into the named declared slot (§2.5).
    ///
    /// Wave 4 delegates to [`Slot::mount`] with a descriptor synthesised from
    /// `slot` and the module's interface id. Full interface-level slot
    /// lookup, type-checking against the mounted child, and cardinality
    /// enforcement are compile-time / runtime concerns owned by Wave 6
    /// (runtime integration). This method mints a unique [`MountHandle`]
    /// without performing those checks.
    pub fn mount(&self, slot: Symbol, child: ModuleId) -> Result<MountHandle, SlotError> {
        let descriptor = Slot {
            name: slot,
            child_iface: self.iface,
            cardinality: Cardinality::Single,
        };
        descriptor.mount(child)
    }

    /// Tear down this module deterministically.
    ///
    /// Transitions to [`ModuleState::Destroyed`]. If the current state cannot
    /// legally reach `Destroyed` (e.g. already destroyed, or still `Unloaded`
    /// / `Loading` / `Ready`), returns the underlying
    /// [`ModuleError::IllegalTransition`].
    pub fn destroy(&mut self) -> Result<(), ModuleError> {
        self.transition(ModuleState::Destroyed)
    }
}

/// A typed interface contract (ADR 014).
#[derive(Debug, Clone)]
pub struct Interface {
    /// Interned interface name.
    pub name: Symbol,
    /// Declared inputs with optional defaults.
    pub inputs: Box<[(Symbol, Type, Option<Value>)]>,
    /// Declared output signals (`Signal<T>`).
    pub outputs: Box<[(Symbol, Type)]>,
    /// Declared child slots.
    pub slots: Box<[(Symbol, Type, Cardinality)]>,
}

impl Interface {
    /// Find the first declared slot whose name equals `name`.
    ///
    /// Returns a shared reference to the matching `(Symbol, Type, Cardinality)`
    /// triple, or `None` if no such slot is declared on this interface.
    pub fn find_slot(&self, name: Symbol) -> Option<&(Symbol, Type, Cardinality)> {
        self.slots.iter().find(|(s, _, _)| *s == name)
    }
}

/// A typed language entity (ADR 009).
#[derive(Debug, Clone)]
pub struct Type {
    /// Interned type name.
    pub name: Symbol,
    /// Top-level kind.
    pub kind: TypeKind,
    /// Source-level soundness verdict.
    pub soundness: Soundness,
    /// WASM-level structural signature.
    pub wasm_shape: WasmTypeSig,
}

impl Type {
    /// Returns `true` iff this type's source-level soundness has been proven
    /// (ADR 009 level 1). `unsafe`-attested types are not proven.
    pub fn is_proven(&self) -> bool {
        self.soundness == Soundness::Proven
    }
}

/// Encapsulation boundary for a field or declaration (ADR 008).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EncapsulationBoundary {
    /// Owning module instance.
    pub owner: ModuleId,
    /// Visibility scope.
    pub visibility: Visibility,
    /// Required capability; `Some` iff `visibility == Capability`.
    pub capability: Option<CapabilityId>,
}

impl EncapsulationBoundary {
    /// Decide whether `accessor` may read a declaration guarded by this
    /// boundary, given an optional `grant` carried in the accessor's
    /// capability set.
    ///
    /// Wave 4 rules (SPECIFICATION §2.3):
    /// - The owner always passes (regardless of `visibility`).
    /// - [`Visibility::Public`] passes for any accessor.
    /// - [`Visibility::Module`] is satisfied iff `accessor == owner`
    ///   (Wave 4 simplification; the full "same declaring module" rule
    ///   requires module-graph resolution in Wave 6).
    /// - [`Visibility::Capability`] is satisfied iff `grant` equals
    ///   [`Self::capability`].
    /// - [`Visibility::Owner`] passes only for the owner (covered above).
    pub fn check_access(&self, accessor: ModuleId, grant: Option<CapabilityId>) -> bool {
        if accessor == self.owner {
            return true;
        }
        match self.visibility {
            Visibility::Public => true,
            // Wave 4 simplification: same-module access reduces to owner.
            Visibility::Module => accessor == self.owner,
            Visibility::Capability => grant == self.capability,
            Visibility::Owner => false,
        }
    }
}

/// A named, typed child mount point (ADR 007 / ADR 014).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Slot {
    /// Slot name.
    pub name: Symbol,
    /// Interface the mounted child must implement.
    pub child_iface: InterfaceId,
    /// Permitted child cardinality.
    pub cardinality: Cardinality,
}

impl Slot {
    /// Mount `child` into this slot.
    ///
    /// Wave 4 mints a unique [`MountHandle`] drawn from a process-global
    /// monotonically increasing counter (ADR 007). `child` is accepted but
    /// not yet stored — full child-tracking (cardinality enforcement,
    /// scene-graph attachment, panic trapping at the owning boundary per
    /// §2.5) is implemented in Wave 6 alongside the runtime.
    pub fn mount(&self, child: ModuleId) -> Result<MountHandle, SlotError> {
        // `child` will be wired into the runtime's slot occupancy table in
        // Wave 6; until then we accept it to preserve the API.
        let _ = child;
        let id = NEXT_MOUNT_HANDLE.fetch_add(1, Ordering::Relaxed);
        Ok(MountHandle(id))
    }
}

/// Process-global monotonically increasing counter backing [`Slot::mount`].
///
/// Declared as an immutable `static` of type [`AtomicU64`]; the interior
/// mutability is provided by `&self` atomic operations, which are safe under
/// `#![forbid(unsafe_code)]`. Starts at `1` so `MountHandle(0)` remains a
/// sentinel "no mount" value usable by callers.
static NEXT_MOUNT_HANDLE: AtomicU64 = AtomicU64::new(1);

/// A typed output signal emitter (ADR 014).
///
/// Stores the last-known-good emitted value in an internal
/// [`RefCell<Option<T>>`] (interior mutability lets `emit` take `&self`,
/// matching the module-internal writer contract) and dispatches each
/// emission to every registered [`Listener`] callback in registration
/// order. [`Signal::subscribe`] mints a unique [`Subscription`] from an
/// internal [`AtomicU64`] counter and records the `(Subscription, Listener)`
/// pair in an internal listener table.
///
/// Capability-grant verification (ADR 018) and integration with the
/// runtime's observer registry (ADR 014 / §2.6) remain deferred — this
/// crate owns only the in-process dispatch surface.
///
/// `Clone` is implemented manually (not derived) because [`AtomicU64`] does
/// not implement `Clone` and [`Listener<T>`] is not `Clone` (the boxed
/// callback is not cloneable). The cloned signal loads the current counter
/// value and the last-known-good value (the latter requires `T: Clone`)
/// and starts with an empty listener table; subscribers remain bound to
/// the original instance.
#[derive(Debug)]
pub struct Signal<T> {
    /// Last value emitted via [`Signal::emit`]; `None` until the first emit.
    last_value: RefCell<Option<T>>,
    /// Monotonic counter backing [`Signal::subscribe`]; starts at `1` so
    /// `Subscription(0)` remains a sentinel "no subscription" value (mirrors
    /// the [`NEXT_MOUNT_HANDLE`] mount-handle counter).
    next_subscription_id: AtomicU64,
    /// Registered `(Subscription, Listener)` pairs, dispatched in
    /// registration order on every [`Signal::emit`].
    listeners: RefCell<Vec<(Subscription, Listener<T>)>>,
}

impl<T: Clone> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self {
            last_value: RefCell::new(self.last_value.borrow().clone()),
            next_subscription_id: AtomicU64::new(self.next_subscription_id.load(Ordering::Relaxed)),
            // Subscribers are bound to the original instance; the clone
            // starts with an empty listener table (`Listener<T>` is not
            // `Clone` because its boxed callback is not cloneable).
            listeners: RefCell::new(Vec::new()),
        }
    }
}

/// Maximum number of listeners a single [`Signal`] will register.
///
/// `subscribe` enforces this cap to deny a malicious module the ability to
/// register unbounded listeners (which would make `emit` dispatch
/// arbitrarily slow). Once a signal holds `MAX_SUBSCRIBERS` listeners,
/// further `subscribe` calls return the sentinel `Subscription(0)` ("no
/// subscription") and do not register the listener.
pub const MAX_SUBSCRIBERS: usize = 1024;

impl<T> Signal<T> {
    /// Create a new signal with no last value, no listeners, and the
    /// subscription counter starting at `1`.
    pub fn new() -> Self {
        Self {
            last_value: RefCell::new(None),
            next_subscription_id: AtomicU64::new(1),
            listeners: RefCell::new(Vec::new()),
        }
    }

    /// Emit `value` to all subscribers (module-internal writer).
    ///
    /// Stores `value` as the last-known-good emission in the internal
    /// buffer and dispatches a borrow (`&value`) to every registered
    /// [`Listener`] callback that carries one, in registration order.
    /// Listeners built via [`Listener::new`] (no callback) are silently
    /// skipped.
    ///
    /// Capability-grant verification (ADR 018) and integration with the
    /// runtime's observer registry (ADR 014 / §2.6) remain deferred.
    ///
    /// *Reentrancy note*: the listener table is borrowed immutably for the
    /// duration of the dispatch loop, so a callback that re-enters `emit`
    /// on the same signal is permitted (its listeners fire synchronously).
    /// A callback that re-enters `subscribe` on the same signal will panic
    /// (the table cannot be mutated while shared-borrowed).
    pub fn emit(&self, value: T) {
        // Dispatch to every registered listener with a callback first,
        // borrowing `value` immutably so we can still move it into
        // `last_value` afterwards.
        {
            let listeners = self.listeners.borrow();
            for (_sub, listener) in listeners.iter() {
                if let Some(ref f) = listener.callback {
                    f(&value);
                }
            }
        }
        // Retain the last-known-good emission.
        *self.last_value.borrow_mut() = Some(value);
    }

    /// Subscribe `listener`; a capability-gated reader.
    ///
    /// Mints a unique [`Subscription`] id from an internal [`AtomicU64`]
    /// counter and records the `(Subscription, listener)` pair in the
    /// signal's listener table so subsequent [`Signal::emit`] calls can
    /// dispatch to it.
    ///
    /// Capability-grant verification (ADR 018) and registration in the
    /// runtime's observer registry (ADR 014 / §2.6) remain deferred.
    ///
    /// *Capacity*: to prevent a malicious module from registering unbounded
    /// listeners (which would make [`Signal::emit`] dispatch arbitrarily
    /// slow), `subscribe` enforces [`MAX_SUBSCRIBERS`]. Once the signal
    /// holds that many listeners, further calls return the sentinel
    /// `Subscription(0)` ("no subscription") and do not register the
    /// listener.
    pub fn subscribe(&self, listener: Listener<T>) -> Subscription {
        let listeners = self.listeners.borrow();
        if listeners.len() >= MAX_SUBSCRIBERS {
            return Subscription(0); // sentinel: subscription rejected
        }
        drop(listeners); // release borrow before mutable borrow
        let id = self.next_subscription_id.fetch_add(1, Ordering::Relaxed);
        self.listeners
            .borrow_mut()
            .push((Subscription(id), listener));
        Subscription(id)
    }
}

impl<T> Default for Signal<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors raised by module lifecycle operations.
#[derive(Debug, Clone)]
pub enum ModuleError {
    /// WASM validation failed at level 2 (ADR 009).
    CompileFailure(WasmValidationError),
    /// Attempted an illegal state transition.
    IllegalTransition(ModuleState, ModuleState),
    /// A required capability was not granted.
    CapabilityDenied(CapabilityId),
    /// HMR rehydration failed for the named slot.
    HmrRehydrateFailure(SlotName),
}

/// Errors raised by slot mounting (§2.9).
#[derive(Debug, Clone)]
pub enum SlotError {
    /// The referenced slot was not declared on the interface.
    UndeclaredSlot(Symbol),
    /// The mounted child's type did not match the slot's declared type.
    TypeMismatch,
    /// The slot's cardinality was exceeded.
    CardinalityExceeded,
}

/// Errors raised by signal emit/subscribe (§2.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignalError {
    /// `emit` called after the owning module was destroyed.
    EmitAfterDestroyed,
    /// `subscribe` called without the required capability.
    ListenerCapabilityDenied,
}

/// Structural failure reported at an owning boundary (§2.5 / §2.9).
#[derive(Debug, Clone)]
pub struct Failure {
    /// Slot in which the failure was trapped.
    pub slot: SlotName,
    /// Underlying cause.
    pub cause: ModuleError,
    /// Trace identifier for correlation (ADR 016).
    pub trace: TraceId,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    /// Build a [`Module`] in the given state with a minimal boundary owned
    /// by `ModuleId(1)`.
    fn module_in(state: ModuleState) -> Module {
        Module {
            id: ModuleId(1),
            iface: InterfaceId(1),
            state,
            boundary: EncapsulationBoundary {
                owner: ModuleId(1),
                visibility: Visibility::Owner,
                capability: None,
            },
            scene_graph: OwnedSubtreeRef(0),
            imports: Box::new([]),
        }
    }

    /// Build a minimal proven [`Type`] for use in interface slot tuples.
    fn dummy_type() -> Type {
        Type {
            name: Symbol(0),
            kind: TypeKind::Primitive,
            soundness: Soundness::Proven,
            wasm_shape: WasmTypeSig {
                bytes: Box::new([]),
            },
        }
    }

    // ---- Module lifecycle -------------------------------------------------

    #[test]
    fn lifecycle_unloaded_to_active() {
        let mut m = module_in(ModuleState::Unloaded);
        m.transition(ModuleState::Loading).unwrap();
        assert_eq!(m.state, ModuleState::Loading);
        m.transition(ModuleState::Ready).unwrap();
        assert_eq!(m.state, ModuleState::Ready);
        m.transition(ModuleState::Active).unwrap();
        assert_eq!(m.state, ModuleState::Active);
    }

    #[test]
    fn lifecycle_active_suspend_resume() {
        let mut m = module_in(ModuleState::Active);
        m.transition(ModuleState::Suspended).unwrap();
        assert_eq!(m.state, ModuleState::Suspended);
        m.transition(ModuleState::Active).unwrap();
        assert_eq!(m.state, ModuleState::Active);
    }

    #[test]
    fn lifecycle_active_to_destroyed() {
        let mut m = module_in(ModuleState::Active);
        m.transition(ModuleState::Destroyed).unwrap();
        assert_eq!(m.state, ModuleState::Destroyed);
    }

    #[test]
    fn lifecycle_suspended_to_destroyed() {
        let mut m = module_in(ModuleState::Suspended);
        m.transition(ModuleState::Destroyed).unwrap();
        assert_eq!(m.state, ModuleState::Destroyed);
    }

    #[test]
    fn lifecycle_destroyed_is_terminal() {
        let mut m = module_in(ModuleState::Destroyed);
        let err = m.transition(ModuleState::Active).unwrap_err();
        assert!(
            matches!(
                err,
                ModuleError::IllegalTransition(ModuleState::Destroyed, ModuleState::Active)
            ),
            "expected IllegalTransition(Destroyed, Active), got {err:?}"
        );
        // State must be unchanged.
        assert_eq!(m.state, ModuleState::Destroyed);
    }

    #[test]
    fn lifecycle_illegal_unloaded_to_active() {
        let mut m = module_in(ModuleState::Unloaded);
        let err = m.transition(ModuleState::Active).unwrap_err();
        assert!(
            matches!(
                err,
                ModuleError::IllegalTransition(ModuleState::Unloaded, ModuleState::Active)
            ),
            "expected IllegalTransition(Unloaded, Active), got {err:?}"
        );
        assert_eq!(m.state, ModuleState::Unloaded);
    }

    #[test]
    fn destroy_from_active() {
        let mut m = module_in(ModuleState::Active);
        m.destroy().unwrap();
        assert_eq!(m.state, ModuleState::Destroyed);
    }

    #[test]
    fn destroy_from_destroyed_is_illegal() {
        let mut m = module_in(ModuleState::Destroyed);
        assert!(m.destroy().is_err());
        assert_eq!(m.state, ModuleState::Destroyed);
    }

    // ---- EncapsulationBoundary::check_access -----------------------------

    #[test]
    fn visibility_owner_allows_only_owner() {
        let b = EncapsulationBoundary {
            owner: ModuleId(7),
            visibility: Visibility::Owner,
            capability: None,
        };
        assert!(b.check_access(ModuleId(7), None));
        assert!(!b.check_access(ModuleId(8), None));
    }

    #[test]
    fn visibility_module_simplifies_to_owner() {
        let b = EncapsulationBoundary {
            owner: ModuleId(7),
            visibility: Visibility::Module,
            capability: None,
        };
        // Wave 4 simplification: same-module access reduces to accessor==owner.
        assert!(b.check_access(ModuleId(7), None));
        assert!(!b.check_access(ModuleId(8), None));
    }

    #[test]
    fn visibility_public_allows_everyone() {
        let b = EncapsulationBoundary {
            owner: ModuleId(7),
            visibility: Visibility::Public,
            capability: None,
        };
        assert!(b.check_access(ModuleId(7), None));
        assert!(b.check_access(ModuleId(8), None));
        assert!(b.check_access(ModuleId(999), None));
    }

    #[test]
    fn visibility_capability_requires_matching_grant() {
        let cap = CapabilityId(42);
        let b = EncapsulationBoundary {
            owner: ModuleId(7),
            visibility: Visibility::Capability,
            capability: Some(cap),
        };
        // Non-owner, no grant -> denied.
        assert!(!b.check_access(ModuleId(8), None));
        // Non-owner, wrong grant -> denied.
        assert!(!b.check_access(ModuleId(8), Some(CapabilityId(1))));
        // Non-owner, matching grant -> allowed.
        assert!(b.check_access(ModuleId(8), Some(cap)));
        // Owner always passes regardless of grant.
        assert!(b.check_access(ModuleId(7), None));
    }

    // ---- Interface::find_slot --------------------------------------------

    #[test]
    fn find_slot_found_returns_first_match() {
        let t = dummy_type();
        let iface = Interface {
            name: Symbol(1),
            inputs: Box::new([]),
            outputs: Box::new([]),
            slots: Box::new([
                (Symbol(10), t.clone(), Cardinality::Optional),
                (Symbol(11), t.clone(), Cardinality::Single),
            ]),
        };
        let found = iface.find_slot(Symbol(11)).expect("slot 11 should exist");
        assert_eq!(found.0, Symbol(11));
        assert_eq!(found.2, Cardinality::Single);
    }

    #[test]
    fn find_slot_not_found() {
        let t = dummy_type();
        let iface = Interface {
            name: Symbol(1),
            inputs: Box::new([]),
            outputs: Box::new([]),
            slots: Box::new([(Symbol(10), t, Cardinality::Optional)]),
        };
        assert!(iface.find_slot(Symbol(999)).is_none());
    }

    // ---- Type::is_proven -------------------------------------------------

    #[test]
    fn type_is_proven_for_proven_soundness() {
        let mut t = dummy_type();
        assert!(t.is_proven());
        t.soundness = Soundness::UnsafeAttested;
        assert!(!t.is_proven());
    }

    // ---- Slot::mount / Module::mount -------------------------------------

    #[test]
    fn slot_mount_returns_unique_handles() {
        let slot = Slot {
            name: Symbol(1),
            child_iface: InterfaceId(1),
            cardinality: Cardinality::Single,
        };
        let h1 = slot.mount(ModuleId(2)).unwrap();
        let h2 = slot.mount(ModuleId(3)).unwrap();
        // Counter is monotonic and never zero (starts at 1).
        assert_ne!(h1, h2);
        assert!(h1.0 > 0);
        assert!(h2.0 > h1.0);
    }

    #[test]
    fn module_mount_delegates_to_slot_mount() {
        let m = module_in(ModuleState::Active);
        let h = m.mount(Symbol(5), ModuleId(9)).unwrap();
        assert!(h.0 > 0);
    }

    // ---- Signal::emit / Signal::subscribe --------------------------------

    #[test]
    fn signal_emit_stores_last_value() {
        let signal: Signal<i32> = Signal::new();
        // No value until the first emit.
        assert!(signal.last_value.borrow().is_none());
        signal.emit(42);
        assert_eq!(*signal.last_value.borrow(), Some(42));
        // A second emit overwrites the last-known-good value.
        signal.emit(7);
        assert_eq!(*signal.last_value.borrow(), Some(7));
    }

    #[test]
    fn signal_subscribe_returns_unique_nonzero_ids() {
        let signal: Signal<i32> = Signal::new();
        let s1 = signal.subscribe(Listener::new(1));
        let s2 = signal.subscribe(Listener::new(2));
        let s3 = signal.subscribe(Listener::new(3));
        // Counter starts at 1; Subscription(0) is the sentinel.
        assert!(s1.0 > 0);
        assert!(s2.0 > s1.0);
        assert!(s3.0 > s2.0);
        assert_ne!(s1, s2);
        assert_ne!(s2, s3);
    }

    #[test]
    fn signal_subscribe_rejects_excess_subscribers() {
        let signal: Signal<i32> = Signal::new();
        // Register MAX_SUBSCRIBERS listeners
        for i in 0..MAX_SUBSCRIBERS {
            signal.subscribe(Listener::new(i as u64));
        }
        // Next subscription should return sentinel Subscription(0)
        let sub = signal.subscribe(Listener::new(MAX_SUBSCRIBERS as u64));
        assert_eq!(sub, Subscription(0));
        // Verify listener count didn't exceed the limit
        assert_eq!(signal.listeners.borrow().len(), MAX_SUBSCRIBERS);
    }

    // ---- Signal::emit dispatch (Gap #1) ---------------------------------

    #[test]
    fn signal_emit_dispatches_to_subscribed_callback() {
        // Create a Signal<i32>, subscribe with a callback that increments
        // a counter, emit 42, verify counter == 1 and last_value == Some(42).
        let signal: Signal<i32> = Signal::new();
        let counter = Rc::new(Cell::new(0u32));
        let counter_for_cb = counter.clone();
        signal.subscribe(Listener::with_callback(1, move |_v| {
            counter_for_cb.set(counter_for_cb.get() + 1);
        }));
        assert!(signal.last_value.borrow().is_none(), "no emit yet");
        signal.emit(42);
        assert_eq!(counter.get(), 1, "callback should have fired once");
        assert_eq!(*signal.last_value.borrow(), Some(42));
    }

    #[test]
    fn signal_emit_dispatches_to_all_subscribers() {
        // Subscribe 3 listeners, emit once, verify all 3 received the value.
        let signal: Signal<i32> = Signal::new();
        let c1 = Rc::new(Cell::new(0u32));
        let c2 = Rc::new(Cell::new(0u32));
        let c3 = Rc::new(Cell::new(0u32));
        let c1_cb = c1.clone();
        let c2_cb = c2.clone();
        let c3_cb = c3.clone();
        signal.subscribe(Listener::with_callback(1, move |_v| {
            c1_cb.set(c1_cb.get() + 1);
        }));
        signal.subscribe(Listener::with_callback(2, move |_v| {
            c2_cb.set(c2_cb.get() + 1);
        }));
        signal.subscribe(Listener::with_callback(3, move |_v| {
            c3_cb.set(c3_cb.get() + 1);
        }));
        signal.emit(7);
        assert_eq!(c1.get(), 1, "listener 1 should have fired once");
        assert_eq!(c2.get(), 1, "listener 2 should have fired once");
        assert_eq!(c3.get(), 1, "listener 3 should have fired once");
        assert_eq!(*signal.last_value.borrow(), Some(7));
    }

    #[test]
    fn signal_emit_with_no_callback_listener_does_not_panic() {
        // Subscribe with no callback (Listener::new), emit, verify no panic.
        let signal: Signal<i32> = Signal::new();
        signal.subscribe(Listener::new(1));
        // A no-callback listener must be silently skipped; emit must not
        // panic and must still retain the last-known-good value.
        signal.emit(99);
        assert_eq!(*signal.last_value.borrow(), Some(99));
    }
}
