; C# — syntactic tags, in the engine's generic capture vocabulary.
; The whole vocabulary is listed in queries/README.md. The engine knows ONLY
; those capture names — never a node name or a language.

(using_directive) @import

; A `global using` is in sight of every file of the project, not only of the
; file that writes it; it stays an @import of its own file too.
((using_directive) @import.global
  (#match? @import.global "^global"))

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

; A constructor is a member like a method; left uncaptured, its header was read
; as a call and the class appeared to use itself.
(constructor_declaration name: (identifier) @name) @definition.method

; Decorations — an attribute list (`[HttpGet("{id}")]`, `[Fact]`) is not code
; of the declaration it adorns: the engine starts the header after it and reads
; no call out of it.
(attribute_list) @decoration
