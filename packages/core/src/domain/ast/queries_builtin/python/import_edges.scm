; Python import declarations.
; Verified against tree-sitter-python 0.25.0 node-types.json:
;   import_statement       name: (dotted_name | aliased_import)
;   import_from_statement  module_name: (dotted_name | relative_import)
(import_statement (dotted_name) @import)
(import_from_statement module_name: (dotted_name) @import)
