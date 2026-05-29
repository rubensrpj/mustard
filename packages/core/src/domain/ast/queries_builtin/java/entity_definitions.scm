; Java named type declarations.
; Verified against tree-sitter-java 0.23.5 node-types.json:
;   class_declaration / interface_declaration / enum_declaration /
;   record_declaration all carry a `name:` field of type `identifier`.
(class_declaration name: (identifier) @name) @kind
(interface_declaration name: (identifier) @name) @kind
(enum_declaration name: (identifier) @name) @kind
(record_declaration name: (identifier) @name) @kind
