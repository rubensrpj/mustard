; Dart — imports and definitions. Generic capture vocabulary only:
;   @import            an import directive's URI (the `package:`/relative path)
;   @name              the identifier of the enclosing @definition.*
;   @definition.<kind> a declaration; <kind> becomes Decl.kind verbatim
;   @decoration        an attribute or annotation, never code of the declaration
;   @body              the body of a declaration kept beside it, not inside it
; The engine knows ONLY these capture names — never a node name or a language.
;
; Verified against tree-sitter-dart-orchard 0.3 node-types.json:
;   library_import -> import_specification -> (uri (string_literal)); capturing
;     the (uri) keeps the path out of any `as`/`show`/`hide` combinator, and the
;     engine's clean_import strips the surrounding quotes.
;   class_definition / mixin_declaration / enum_declaration /
;     extension_declaration each expose a `name:` field of type (identifier).
;   a class body member is a (function_signature name: (identifier)) — the
;     closest the grammar has to a method declaration node.
(import_specification (configurable_uri (uri) @import))
(import_specification (uri) @import)

(class_definition name: (identifier) @name) @definition.class
; `mixin_declaration` has NO `name:` field (node-types.json: fields = {}); the
; mixin name is a positional (identifier) child — a `name:` pattern would fail
; to compile and be dropped silently, so match it positionally.
(mixin_declaration (identifier) @name) @definition.mixin
(enum_declaration name: (identifier) @name) @definition.enum
(extension_declaration name: (identifier) @name) @definition.extension

; Functions — a library-level function is a UNIT, a member is a MEMBER. Dart
; spells both with `function_signature`, so the line is drawn by CONTEXT: a
; library function is a DIRECT child of `program`, while a member is always
; wrapped — in `method_signature` (class and extension bodies) or in
; `declaration` (a mixin's abstract member). A node has one parent, so the three
; patterns are mutually exclusive and the recorded kind never depends on match
; order. Before this, every library function was recorded as a member and the
; miner never saw it.
(program (function_signature name: (identifier) @name) @definition.function)
(method_signature (function_signature name: (identifier) @name) @definition.method)
(declaration (function_signature name: (identifier) @name) @definition.method)

; Constructors — a member like a method; left uncaptured, the header was read
; as a call and the class appeared to use itself. The name is the identifier
; right before the parameter list, so a named constructor (`Caixa.vazia()`)
; records its own name.
(constructor_signature (identifier) @name . (formal_parameter_list)) @definition.method
(constant_constructor_signature (identifier) @name . (formal_parameter_list)) @definition.method
(factory_constructor_signature (identifier) @name . (formal_parameter_list)) @definition.method

; Bodies — Dart keeps a function's body as the NEXT SIBLING of its signature,
; not inside it, so the signature alone ends on its own line. @body marks the
; body that belongs to the declaration, and the declaration ends where it ends:
; a call inside the body is then used by the function that encloses it.
(program (function_signature name: (identifier) @name) @definition.function . (function_body) @body)
;
; A member keeps its body beside the `method_signature` that wraps it, and this
; holds in a class, a mixin, an extension and an enum alike, so the pattern is
; anchored on the wrapper and not on the body that holds it. Each form of member
; has its own line: the method, the constructor (plain and named), the factory,
; the getter and the setter. The setter is captured only here: its name is a
; declaration, and its header is no longer read as a call.
((method_signature (function_signature name: (identifier) @name) @definition.method) . (function_body) @body)
((method_signature (constructor_signature (identifier) @name . (formal_parameter_list)) @definition.method) . (function_body) @body)
((method_signature (factory_constructor_signature (identifier) @name . (formal_parameter_list)) @definition.method) . (function_body) @body)
((method_signature (getter_signature name: (identifier) @name) @definition.method) . (function_body) @body)
((method_signature (setter_signature name: (identifier) @name) @definition.method) . (function_body) @body)

; Library — a `part` file shares one library with its owner and imports
; nothing, so each side is an import of the other: `part 'x.dart';` in the
; owner, `part of 'owner.dart';` (by file) or `part of loja.caixa;` (by the
; library name, answered by the owner's `library loja.caixa;` namespace).
(part_directive (uri) @import)
(part_of_directive (uri) @import)
(part_of_directive (dotted_identifier_list) @import)
(library_name (dotted_identifier_list) @namespace)

; Decorations — an annotation (`@override`, `@immutable`) is not code of the
; declaration it adorns: the engine passes over it to find the doc comment
; above, starts the header after it, and reads no call out of it.
(annotation) @decoration
