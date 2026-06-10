; Python — imports and definitions. A module is a file, so no @namespace.
(import_statement name: (dotted_name) @import)
(import_statement name: (aliased_import (dotted_name) @import))
(import_from_statement module_name: (dotted_name) @import)
(import_from_statement module_name: (relative_import) @import)

(class_definition name: (identifier) @name) @definition.class
(function_definition name: (identifier) @name) @definition.function

; Members — class-level attributes (`name = ""` / `name: str = ""` in a class
; body), the closest Python syntax has to a field declaration. Methods stay
; @definition.function: a method is the same function_definition node (the
; upstream tree-sitter-python tags.scm (MIT) draws no method/function line
; either), and a second pattern on the same node would make the recorded kind
; depend on match order. Member kinds feed the digest's domain-term index only:
; the miner's significance gate (mine.rs) is kind-based and never sees them.
(class_definition
  body: (block
    (expression_statement
      (assignment left: (identifier) @name) @definition.field)))
