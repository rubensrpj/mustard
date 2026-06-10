; C# — syntactic tags. Generic capture vocabulary understood by the engine:
;   @import            an import/using statement (text is cleaned to a path)
;   @namespace         a declared namespace/package name
;   @name              the identifier of the enclosing @definition.*
;   @definition.<kind> a declaration; <kind> becomes Decl.kind verbatim
; The engine knows ONLY these capture names — never a node name or a language.

(using_directive) @import

(namespace_declaration name: (_) @namespace)
(file_scoped_namespace_declaration name: (_) @namespace)

(class_declaration name: (identifier) @name) @definition.class
(interface_declaration name: (identifier) @name) @definition.interface
(record_declaration name: (identifier) @name) @definition.record
(struct_declaration name: (identifier) @name) @definition.struct
(enum_declaration name: (identifier) @name) @definition.enum

; Members — methods, properties, fields, enum members. Member kinds feed the
; digest's domain-term index only: the miner's significance gate (mine.rs)
; is kind-based and never sees them. Derived from the upstream
; tree-sitter-c-sharp tags.scm (MIT) — see queries/README.md.
(method_declaration name: (identifier) @name) @definition.method
(property_declaration name: (identifier) @name) @definition.property
(field_declaration (variable_declaration (variable_declarator name: (identifier) @name))) @definition.field
(enum_member_declaration name: (identifier) @name) @definition.enum_member
