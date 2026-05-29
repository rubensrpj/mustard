; Java import declarations.
; Verified against tree-sitter-java 0.23.5 node-types.json:
;   import_declaration children: scoped_identifier | identifier | asterisk
(import_declaration (scoped_identifier) @import)
(import_declaration (identifier) @import)
