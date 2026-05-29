; TypeScript named type declarations.
; Verified against tree-sitter-typescript 0.23.2 (typescript) node-types.json:
;   class_declaration      name: type_identifier
;   interface_declaration  name: type_identifier
;   enum_declaration       name: identifier
;   type_alias_declaration name: type_identifier
(class_declaration name: (type_identifier) @name) @kind
(interface_declaration name: (type_identifier) @name) @kind
(enum_declaration name: (identifier) @name) @kind
(type_alias_declaration name: (type_identifier) @name) @kind
