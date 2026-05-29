; Rust `use` declarations.
; `use_declaration` carries an `argument:` field (the imported path tree).
; Capturing the whole node keeps the extractor agnostic about the many
; argument shapes (scoped_identifier, use_list, use_wildcard, ...).
; Verified against tree-sitter-rust 0.24.2 node-types.json.
(use_declaration) @import
