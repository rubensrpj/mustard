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

; Decorated functions — a decorator wraps the def in a `decorated_definition`,
; so the two patterns above never see it. The same line is drawn by the
; wrapper's parent: the module for a function, a class body for a method.
(module
  (decorated_definition
    definition: (function_definition name: (identifier) @name) @definition.function))
(class_definition
  body: (block
    (decorated_definition
      definition: (function_definition name: (identifier) @name) @definition.method)))

; Docstrings — the first string of a body documents the def or the class. One
; pattern per form above, each with the same kind, so the docstring joins the
; declaration the pattern above already gives. `string_content` is the text
; without its quotes.
(class_definition
  name: (identifier) @name
  body: (block . (expression_statement (string (string_content) @doc)))) @definition.class
(module
  (function_definition
    name: (identifier) @name
    body: (block . (expression_statement (string (string_content) @doc)))) @definition.function)
(class_definition
  body: (block
    (function_definition
      name: (identifier) @name
      body: (block . (expression_statement (string (string_content) @doc)))) @definition.method))
(module
  (decorated_definition
    definition: (function_definition
      name: (identifier) @name
      body: (block . (expression_statement (string (string_content) @doc)))) @definition.function))
(class_definition
  body: (block
    (decorated_definition
      definition: (function_definition
        name: (identifier) @name
        body: (block . (expression_statement (string (string_content) @doc)))) @definition.method)))

; Decorations — a decorator (`@app.get("/")`, `@dataclass`) is not code of the
; declaration it adorns: the engine passes over it to find the comment above
; and reads no call out of it.
(decorator) @decoration
