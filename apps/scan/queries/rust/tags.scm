; Rust — use imports and item definitions.
(use_declaration argument: (_) @import)

(struct_item name: (type_identifier) @name) @definition.struct
(enum_item name: (type_identifier) @name) @definition.enum
(trait_item name: (type_identifier) @name) @definition.trait
(function_item name: (identifier) @name) @definition.function
(type_item name: (type_identifier) @name) @definition.type

; Members — struct fields and enum variants. Methods stay @definition.function:
; a method is the same function_item node inside an impl block (the upstream
; tree-sitter-rust tags.scm (MIT) draws no method/function line either), and a
; second pattern on the same node would make the recorded kind depend on match
; order. Member kinds feed the digest's domain-term index only: the miner's
; significance gate (mine.rs) is kind-based and never sees them.
(field_declaration name: (field_identifier) @name) @definition.field
(enum_variant name: (identifier) @name) @definition.enum_member
