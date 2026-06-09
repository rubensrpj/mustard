; PHP — contracts a type builds on: `class X extends Base implements IFoo`.
; @supertype names are attached (by declaration name) to the matching @definition.
; `extends` (base_clause) and `implements` (class_interface_clause) both feed the
; base list; the engine mines whichever base name recurs across many types as a
; shared contract. Only the generic capture vocabulary is used.

(class_declaration
  name: (name) @name
  (base_clause [(name) (qualified_name)] @supertype))
(class_declaration
  name: (name) @name
  (class_interface_clause [(name) (qualified_name)] @supertype))
(interface_declaration
  name: (name) @name
  (base_clause [(name) (qualified_name)] @supertype))
