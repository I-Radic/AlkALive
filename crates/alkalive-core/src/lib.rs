//! AlkALive alkalive-core crate.
//!
//! Core language model: modules, interfaces, types, encapsulation boundaries,
//! slots, signals, and the error surface defined in `docs/SPECIFICATION.md`
//! §2.7–2.9. Wave 4 implements the runtime-free semantics: module lifecycle
//! state machine, encapsulation access checks, slot mounting, type soundness
//! queries, and interface slot lookup. Wave 5 replaces the `Signal::emit` /
//! `Signal::subscribe` `todo!()` stubs with a last-known-good value buffer
//! and a unique subscription-id minter; dispatch to subscribers remains
//! deferred to the Wave 6 runtime integration (observer registry, ADR 014).

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
#[derive(Debug, Clone)]
pub struct Listener<T> {
    /// Opaque listener identifier.
    pub id: u64,
    _marker: PhantomData<T>,
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
/// Wave 5 replaces the prior `todo!()` stubs with a real last-known-good
/// buffer and a unique subscription-id minter. [`Signal::emit`] stores the
/// emitted value in an internal [`RefCell<Option<T>>`] (interior mutability
/// lets `emit` take `&self`, matching the module-internal writer contract).
/// [`Signal::subscribe`] mints a unique [`Subscription`] from an internal
/// [`AtomicU64`] counter. Dispatch to registered subscribers — the runtime's
/// observer registry (ADR 014 / §2.6) and capability-grant verification
/// (ADR 018) — remains a TODO pending the Wave 6 runtime integration.
///
/// `Clone` is implemented manually (not derived) because [`AtomicU64`] does
/// not implement `Clone`; the cloned signal loads the current counter value
/// and the last-known-good value (the latter requires `T: Clone`).
#[derive(Debug)]
pub struct Signal<T> {
    /// Last value emitted via [`Signal::emit`]; `None` until the first emit.
    last_value: RefCell<Option<T>>,
    /// Monotonic counter backing [`Signal::subscribe`]; starts at `1` so
    /// `Subscription(0)` remains a sentinel "no subscription" value (mirrors
    /// the [`NEXT_MOUNT_HANDLE`] mount-handle counter).
    next_subscription_id: AtomicU64,
}

impl<T: Clone> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self {
            last_value: RefCell::new(self.last_value.borrow().clone()),
            next_subscription_id: AtomicU64::new(self.next_subscription_id.load(Ordering::Relaxed)),
        }
    }
}

impl<T> Signal<T> {
    /// Create a new signal with no last value and the subscription counter
    /// starting at `1`.
    pub fn new() -> Self {
        Self {
            last_value: RefCell::new(None),
            next_subscription_id: AtomicU64::new(1),
        }
    }

    /// Emit `value` to all subscribers (module-internal writer).
    ///
    /// Wave 5 stores `value` as the last-known-good emission in the
    /// internal buffer. Dispatch to registered subscribers is deferred —
    /// the runtime's observer registry (ADR 014 / §2.6) is not yet wired.
    pub fn emit(&self, value: T) {
        *self.last_value.borrow_mut() = Some(value);
        // TODO(observer registry, Wave 6 runtime integration): dispatch
        // `value` to every registered subscriber via the runtime's observer
        // registry (ADR 014 / §2.6). Until then we only retain the
        // last-known-good value.
    }

    /// Subscribe `listener`; a capability-gated reader.
    ///
    /// Wave 5 mints a unique [`Subscription`] id from an internal
    /// [`AtomicU64`] counter. Capability-grant verification (ADR 018) and
    /// registration in the runtime's subscriber table are deferred — the
    /// runtime's observer registry (ADR 014 / §2.6) is not yet wired.
    pub fn subscribe(&self, listener: Listener<T>) -> Subscription {
        let _ = listener;
        // TODO(observer registry, Wave 6 runtime integration): register
        // `listener` in the runtime's observer registry and verify the
        // caller's capability grant (ADR 018 / ADR 014). Until then we only
        // mint a unique subscription id.
        let id = self.next_subscription_id.fetch_add(1, Ordering::Relaxed);
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
        let s1 = signal.subscribe(Listener { id: 1, _marker: PhantomData });
        let s2 = signal.subscribe(Listener { id: 2, _marker: PhantomData });
        let s3 = signal.subscribe(Listener { id: 3, _marker: PhantomData });
        // Counter starts at 1; Subscription(0) is the sentinel.
        assert!(s1.0 > 0);
        assert!(s2.0 > s1.0);
        assert!(s3.0 > s2.0);
        assert_ne!(s1, s2);
        assert_ne!(s2, s3);
    }
}
