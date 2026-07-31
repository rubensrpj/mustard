; Python — imports and definitions. A module is a file, so no @namespace.
(import_statement name: (dotted_name) @import)
(import_statement name: (aliased_import (dotted_name) @import))
(import_from_statement module_name: (dotted_name) @import)
(import_from_statement module_name: (relative_import) @import)

(class_definition name: (identifier) @name) @definition.class

; Functions — a module-level function is a UNIT, a method is a MEMBER. Python
; spells both with `function_definition`, so the line is drawn by CONTEXT: a
; module-level function is a DIRECT child of `module`, a method is a direct
; child of a class body's `block`. A node has one parent, so the two patterns
; are mutually exclusive and the recorded kind never depends on match order —
; the hazard that once justified recording every def as @definition.function.
; A function nested inside another function is neither: a local closure is not
; an architectural unit.
(module (function_definition name: (identifier) @name) @definition.function)
(class_definition
  body: (block (function_definition name: (identifier) @name) @definition.method))

; Members — class-level attributes (`name = ""` / `name: str = ""` in a class
; body), the closest Python syntax has to a field declaration. Member kinds feed
; the digest's domain-term index only: the miner's significance gate (mine.rs)
; never treats them as units.
(class_definition
  body: (block
    (expression_statement
      (assignment left: (identifier) @name) @definition.field)))
