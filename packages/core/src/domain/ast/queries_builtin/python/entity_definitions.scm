; Python class declarations.
; Verified against tree-sitter-python 0.25.0 node-types.json:
;   class_definition name: identifier
(class_definition name: (identifier) @name) @kind
