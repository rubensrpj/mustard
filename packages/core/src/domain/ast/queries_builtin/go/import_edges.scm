; Go import specs.
; Verified against tree-sitter-go 0.25.0 node-types.json:
;   import_spec path: (interpreted_string_literal | raw_string_literal)
(import_spec path: (interpreted_string_literal) @import)
(import_spec path: (raw_string_literal) @import)
