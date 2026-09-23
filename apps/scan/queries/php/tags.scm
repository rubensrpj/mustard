; PHP — imports, namespace, and definitions. Generic capture vocabulary only:
;   @import            a `use` import (the imported qualified name is the path)
;   @namespace         the declared `namespace App\Http;` name
;   @name              the identifier of the enclosing @definition.*
;   @definition.<kind> a declaration; <kind> becomes Decl.kind verbatim
;   @decoration        an attribute or annotation, never code of the declaration
; The engine knows ONLY these capture names — never a node name or a language.

(namespace_use_clause [(qualified_name) (name)] @import)

(namespace_definition name: (namespace_name) @namespace)

(class_declaration name: (name) @name) @definition.class
(interface_declaration name: (name) @name) @definition.interface
(trait_declaration name: (name) @name) @definition.trait
(enum_declaration name: (name) @name) @definition.enum

(function_definition name: (name) @name) @definition.function

; Constants — each element of a `const` statement, in a class or at the top of
; the file; the value is not its header.
(const_declaration (const_element (name) @name (_) @value)) @definition.const

; The names `use function` and `use const` bring into the file: what it
; brought, not a use of it.
(namespace_use_clause type: _ (qualified_name (name) @imported))
(namespace_use_clause type: _ (name) @imported)
(namespace_use_declaration type: _
  body: (namespace_use_group (namespace_use_clause [(qualified_name (name) @imported) (name) @imported])))

; Members — methods, typed properties, enum cases. Member kinds feed the
; digest's domain-term index only: the miner's significance gate (mine.rs) is
; kind-based and never sees them. The method tag follows the upstream
; tree-sitter-php tags.scm (MIT) — see queries/README.md.
(method_declaration name: (name) @name) @definition.method
(property_declaration (property_element name: (variable_name (name) @name))) @definition.property
(enum_case name: (name) @name) @definition.enum_member

; Decorations — an attribute list (`#[Route('/lista')]`) is not code of the
; declaration it adorns: the engine starts the header after it and reads no
; call out of it.
(attribute_list) @decoration
