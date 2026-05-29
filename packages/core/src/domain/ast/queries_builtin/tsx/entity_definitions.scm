; TSX shares the TypeScript node grammar for type declarations.
; Verified against tree-sitter-typescript 0.23.2 (tsx) node-types.json:
;   class_declaration / interface_declaration / type_alias_declaration
;   name: type_identifier ; enum_declaration name: identifier
(class_declaration name: (type_identifier) @name) @kind
(interface_declaration name: (type_identifier) @name) @kind
(enum_declaration name: (identifier) @name) @kind
(type_alias_declaration name: (type_identifier) @name) @kind
