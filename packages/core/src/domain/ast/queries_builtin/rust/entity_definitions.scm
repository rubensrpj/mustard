; Rust named type declarations.
; Each clause captures the type identifier as @name and a literal @kind.
; Verified against tree-sitter-rust 0.24.2 node-types.json:
;   struct_item / enum_item / trait_item / type_item / union_item
;   all carry a `name:` field of type `type_identifier`.
(struct_item name: (type_identifier) @name) @kind
(enum_item name: (type_identifier) @name) @kind
(trait_item name: (type_identifier) @name) @kind
(type_item name: (type_identifier) @name) @kind
(union_item name: (type_identifier) @name) @kind
