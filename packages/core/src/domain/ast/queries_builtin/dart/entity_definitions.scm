; Dart named type declarations.
; Verified against tree-sitter-dart-orchard 0.6 node-types.json:
;   class_definition / enum_declaration / extension_declaration carry a `name:`
;   field of type `identifier`; `mixin_declaration` has NO field (fields = {}) —
;   its name is a positional (identifier) child, so match it positionally or the
;   pattern fails to compile and is dropped silently.
(class_definition name: (identifier) @name) @kind
(enum_declaration name: (identifier) @name) @kind
(mixin_declaration (identifier) @name) @kind
(extension_declaration name: (identifier) @name) @kind
