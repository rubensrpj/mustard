; PHP — imports, namespace, and definitions. Generic capture vocabulary only:
;   @import            a `use` import (the imported qualified name is the path)
;   @namespace         the declared `namespace App\Http;` name
;   @name              the identifier of the enclosing @definition.*
;   @definition.<kind> a declaration; <kind> becomes Decl.kind verbatim
; The engine knows ONLY these capture names — never a node name or a language.

(namespace_use_clause [(qualified_name) (name)] @import)

(namespace_definition name: (namespace_name) @namespace)

(class_declaration name: (name) @name) @definition.class
(interface_declaration name: (name) @name) @definition.interface
(trait_declaration name: (name) @name) @definition.trait
(enum_declaration name: (name) @name) @definition.enum

(function_definition name: (name) @name) @definition.function
(method_declaration name: (name) @name) @definition.function
