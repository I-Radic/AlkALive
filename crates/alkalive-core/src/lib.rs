//! AlkALive alkalive-core crate.
//!
//! Core language model: modules, interfaces, types, encapsulation boundaries,
//! slots, signals, and the error surface defined in `docs/SPECIFICATION.md`
//! §2.7–2.9. This is a Wave 3 trait-definition skeleton: every domain method
//! body is `todo!()`; no behaviour is implemented yet.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use core::marker::PhantomData;

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
    pub fn transition(&mut self, target: ModuleState) -> Result<(), ModuleError> {
        let _ = target;
        todo!()
    }
    /// Mount `child` into the named declared slot (§2.5).
    pub fn mount(&self, slot: Symbol, child: ModuleId) -> Result<MountHandle, SlotError> {
        let _ = (slot, child);
        todo!()
    }
    /// Tear down this module deterministically.
    pub fn destroy(&mut self) -> Result<(), ModuleError> {
        todo!()
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
    pub fn mount(&self, child: ModuleId) -> Result<MountHandle, SlotError> {
        let _ = child;
        todo!()
    }
}

/// A typed output signal emitter (ADR 014).
#[derive(Debug, Clone)]
pub struct Signal<T> {
    _marker: PhantomData<T>,
}

impl<T> Signal<T> {
    /// Emit `value` to all subscribers (module-internal writer).
    pub fn emit(&self, value: T) {
        let _ = value;
        todo!()
    }
    /// Subscribe `listener`; a capability-gated reader.
    pub fn subscribe(&self, listener: Listener<T>) -> Subscription {
        let _ = listener;
        todo!()
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
