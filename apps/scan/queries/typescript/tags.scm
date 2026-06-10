; TypeScript / TSX — imports and declarations. Same grammar family, one query set.
(import_statement source: (string (string_fragment) @import))

(class_declaration name: (_) @name) @definition.class
(abstract_class_declaration name: (_) @name) @definition.class
(interface_declaration name: (_) @name) @definition.interface
(enum_declaration name: (_) @name) @definition.enum
(type_alias_declaration name: (_) @name) @definition.type
(function_declaration name: (_) @name) @definition.function

; Exported top-level consts (e.g. `export const userTable = pgTable(...)`).
; This is the syntax hook a convention like Drizzle/GraphQL plugs into — the
; engine never knows the framework; it just sees a recurring `export const`.
(export_statement
  declaration: (lexical_declaration
    (variable_declarator name: (identifier) @name) @definition.const))

; Members — methods (class + interface), class fields, interface properties,
; enum members. Member kinds feed the digest's domain-term index only: the
; miner's significance gate (mine.rs) is kind-based and never sees them.
; Derived from the upstream tree-sitter-typescript tags.scm (MIT) — see
; queries/README.md. A plain enum member is the enum_body's own `name` field;
; an initialized one is an enum_assignment.
(method_definition name: (_) @name) @definition.method
(method_signature name: (_) @name) @definition.method
(abstract_method_signature name: (_) @name) @definition.method
(public_field_definition name: (_) @name) @definition.field
(property_signature name: (_) @name) @definition.property
(enum_body name: (property_identifier) @name @definition.enum_member)
(enum_assignment name: (_) @name) @definition.enum_member
