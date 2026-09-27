; Rust — `impl Trait for Type` attaches Trait to Type. The trait is the contract;
; the type keeps it among its supertypes, the contract the member linking reads
; (`graph::link_members`), exactly like a C# base class. Inherent impls
; (`impl Type`) have no `trait:` field and so do not match. The @name (the implemented type) is matched to the type's
; declaration by name, since the impl block is a separate node from the item.
(impl_item
  trait: (_) @supertype
  type: (_) @name)
