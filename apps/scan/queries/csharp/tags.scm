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
; O parâmetro do construtor primário (`class Servico(IRepo repo)`) vale na
; classe inteira, também nas outras partes de uma classe `partial`: é um campo
; dela.
(class_declaration (parameter_list (parameter name: (identifier) @name) @definition.field))
(struct_declaration (parameter_list (parameter name: (identifier) @name) @definition.field))

; A constructor is a member like a method; left uncaptured, its header was read
; as a call and the class appeared to use itself.
(constructor_declaration name: (identifier) @name) @definition.method

; O caminho escrito antes do nome numa chamada qualificada, de duas partes ou
; mais (`System.IO.File` em `System.IO.File.ReadAllText()`): a raiz dele diz
; se a chamada é de biblioteca.
(invocation_expression
  function: (member_access_expression
    expression: (member_access_expression) @call.path))

; O método entregue como valor, sem ser chamado ali (o grupo de método): o
; nome escrito como argumento (`xs.Select(Metade)`, `xs.Select(Calc.Dobro)`),
; como valor de uma variável (`Func<int, int> f = Metade;`) ou à direita de
; uma atribuição (`Salvo += Avisar;`). O motor liga o nome só a uma função ou
; a um método à vista. No argumento com nome (`F(x: Metade)`), o valor é o
; último nome dele.
(argument (identifier) @call.value .)
(argument (member_access_expression name: (identifier) @call.value) .)
(variable_declarator "=" (identifier) @call.value)
(variable_declarator "=" (member_access_expression name: (identifier) @call.value))
(assignment_expression right: (identifier) @call.value)
(assignment_expression right: (member_access_expression name: (identifier) @call.value))

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
(tuple_pattern name: (identifier) @local)
(parenthesized_variable_designation name: (identifier) @local)
(declaration_expression name: (identifier) @local)
(declaration_pattern name: (identifier) @local)
(recursive_pattern name: (identifier) @local)
(var_pattern name: (identifier) @local)
(catch_declaration name: (identifier) @local)
(from_clause name: (identifier) @local)
(let_clause . (identifier) @local)

; Os textos fixos: o literal de texto escrito no código. O motor guarda o que
; tem duas palavras ou forma de caminho ou chave, com a marca (log, erro ou
; texto) e a declaração que o contém.
(string_literal) @text
(verbatim_string_literal) @text
(raw_string_literal) @text
(interpolated_string_expression) @text
