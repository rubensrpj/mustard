; C# — syntactic tags, in the engine's generic capture vocabulary.
; The whole vocabulary is listed in queries/README.md. The engine knows ONLY
; those capture names — never a node name or a language.

(using_directive) @import

; A `global using` is in sight of every file of the project, not only of the
; file that writes it; it stays an @import of its own file too.
((using_directive) @import.global
  (#match? @import.global "^global"))

(namespace_declaration name: (_) @namespace)
(file_scoped_namespace_declaration name: (_) @namespace)

(class_declaration name: (identifier) @name) @definition.class
(interface_declaration name: (identifier) @name) @definition.interface
(record_declaration name: (identifier) @name) @definition.record
(struct_declaration name: (identifier) @name) @definition.struct
(enum_declaration name: (identifier) @name) @definition.enum

; Membros — métodos, propriedades, campos, membros de enum. Os kinds de membro
; chegam ao mapa com as outras declarações do arquivo, e o grafo lista cada um
; sob o tipo dono dele. Derivado do tags.scm do tree-sitter-c-sharp (MIT) —
; veja queries/README.md.
(method_declaration name: (identifier) @name) @definition.method
(property_declaration name: (identifier) @name) @definition.property
; A `const` is a constant, not a field. Its pattern comes before the field's
; because the first pattern that matches a node gives the kind. A `static
; readonly` stays a field: its names are the names of properties and of the
; standard library, and a constant of the project is written `const`.
((field_declaration
  (modifier) @_modifier
  (variable_declaration (variable_declarator name: (identifier) @name (_) @value))) @definition.const
  (#eq? @_modifier "const"))
(field_declaration (variable_declaration (variable_declarator name: (identifier) @name))) @definition.field
(enum_member_declaration name: (identifier) @name) @definition.enum_member

; A constructor is a member like a method; left uncaptured, its header was read
; as a call and the class appeared to use itself.
(constructor_declaration name: (identifier) @name) @definition.method

; Decorations — an attribute list (`[HttpGet("{id}")]`, `[Fact]`) is not code
; of the declaration it adorns: the engine starts the header after it and reads
; no call out of it.
(attribute_list) @decoration

; Os nomes que o corpo de uma função liga: da linha seguinte até o fim da
; declaração, o mesmo nome escrito sozinho é deles.
(variable_declarator name: (identifier) @local)
(parameter name: (identifier) @local)
(lambda_expression parameters: (implicit_parameter) @local)
(foreach_statement left: (identifier) @local)

; Os textos fixos: o literal de texto escrito no código. O motor guarda o que
; tem duas palavras ou forma de caminho ou chave, com a marca (log, erro ou
; texto) e a declaração que o contém.
(string_literal) @text
(verbatim_string_literal) @text
(raw_string_literal) @text
(interpolated_string_expression) @text
