; Go named type declarations.
; Verified against tree-sitter-go 0.25.0 node-types.json:
;   type_spec  name: type_identifier ; type: (struct_type | interface_type | ...)
;   type_alias name: type_identifier
; A bare `type_spec` capture handles struct/interface/alias uniformly; the
; @kind anchors the whole spec node.
(type_spec name: (type_identifier) @name) @kind
(type_alias name: (type_identifier) @name) @kind
