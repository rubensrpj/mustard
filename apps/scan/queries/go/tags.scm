; Go — package (namespace), imports, top-level types and funcs.
(package_clause (package_identifier) @namespace)
(import_spec path: (interpreted_string_literal) @import)

(type_spec name: (type_identifier) @name type: (struct_type)) @definition.struct
(type_spec name: (type_identifier) @name type: (interface_type)) @definition.interface
(type_alias name: (type_identifier) @name) @definition.type
(function_declaration name: (identifier) @name) @definition.function

; Constants — each name of a `const` spec, grouped or not; the value is not
; its header. A spec with no value repeats the one above it (`iota`).
; The names are the spec's own identifier children: the type is a type node
; and the value is inside its own list, and a `name:` field would give only
; the first name of `a, b = 1, 2`.
(const_spec (identifier) @name) @definition.const
(const_spec (identifier) @name value: (_) @value) @definition.const

; Members — receiver methods and struct fields. Member kinds feed the digest's
; domain-term index only: the miner's significance gate (mine.rs) is kind-based
; and never sees them. The method tag follows the upstream tree-sitter-go
; tags.scm (MIT) — see queries/README.md.
(method_declaration name: (field_identifier) @name) @definition.method
(field_declaration name: (field_identifier) @name) @definition.field

; An interface method is a member like a receiver method; left uncaptured, its
; header was read as a call.
(method_elem name: (field_identifier) @name) @definition.method
