; Rust — use imports and item definitions.
(use_declaration argument: (_) @import)

; The names `use m::limite` brings into the file: what it brought, not a use
; of it. The whole path is already the import's.
(use_declaration argument: (scoped_identifier name: (identifier) @imported))
(use_declaration argument: (identifier) @imported)
(use_list (identifier) @imported)
(use_list (scoped_identifier name: (identifier) @imported))
(use_as_clause alias: (identifier) @imported)

(struct_item name: (type_identifier) @name) @definition.struct
(enum_item name: (type_identifier) @name) @definition.enum
(trait_item name: (type_identifier) @name) @definition.trait
(type_item name: (type_identifier) @name) @definition.type

; Constants — `const` and `static`, the two Rust forms of a named value fixed
; at compile time. Kept as one kind: neither is a callable or a type, and
; `mine.rs::is_significant` leaves the kind out of both allowlists, so a
; constant never becomes an architectural unit.
(const_item name: (identifier) @name) @definition.constant
; The value of a `const` is not its header: `pub const LIMITE: u32 = 10;`
; reads `pub const LIMITE: u32`.
(const_item name: (identifier) @name value: (_) @value) @definition.constant
(static_item name: (identifier) @name) @definition.constant

; Functions — a free function is a UNIT, a method is a MEMBER. Rust spells both
; with the same `function_item` node, so the line is drawn by CONTEXT: each
; pattern is anchored on its PARENT, which makes them mutually exclusive (no two
; ever match the same node) and therefore independent of match order — the very
; hazard that once justified recording every fn as @definition.function. A trait
; method without a body is a `function_signature_item`, which that single
; pattern never captured at all.
(source_file (function_item name: (identifier) @name) @definition.function)
(mod_item body: (declaration_list (function_item name: (identifier) @name) @definition.function))
(impl_item body: (declaration_list (function_item name: (identifier) @name) @definition.method))
(trait_item body: (declaration_list (function_item name: (identifier) @name) @definition.method))
(trait_item body: (declaration_list (function_signature_item name: (identifier) @name) @definition.method))

; Members — struct fields and enum variants. Member kinds feed the digest's
; domain-term index only: the miner's significance gate (mine.rs) never treats
; them as units.
(field_declaration name: (field_identifier) @name) @definition.field
(enum_variant name: (identifier) @name) @definition.enum_member

; Decorations — an attribute is not code of the declaration it adorns: the
; engine passes over it to find the doc comment above, starts the header after
; it, and reads no call out of it (`#[derive(Debug)]` calls nothing).
(attribute_item) @decoration
(inner_attribute_item) @decoration
