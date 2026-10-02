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
; classe inteira, também nas outras partes de uma classe `partial`, mas é
; parâmetro, e não campo: mora no cabeçalho do tipo, cuja assinatura já o
; traz, e ninguém o lê como membro (`outro.repo`).
(class_declaration (parameter_list (parameter name: (identifier) @name) @definition.parameter))
(struct_declaration (parameter_list (parameter name: (identifier) @name) @definition.parameter))

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

; O membro escrito depois do objeto, sem chamada ali: a propriedade ou o
; campo lido ou escrito (`pedido.Total`, `this.Total`), também pelo acesso
; opcional (`pedido?.Total`). O motor liga o nome só a uma propriedade ou a um
; campo, como liga a chamada de método escrita depois do mesmo objeto.
(member_access_expression name: (identifier) @member)
(member_binding_expression name: (identifier) @member)

; A propriedade ou o campo escrito num padrão de propriedade (`x is { Total: > 0 }`,
; `x is { Name: var n }`, `x switch { { Desconto: 0 } => ... }`): o nome antes
; dos dois-pontos é lido do objeto que o padrão confere, escrito sozinho e sem
; chamada ali. Quando o padrão escreve o tipo antes das chaves
; (`x is Pedido { Total: var t }`), o tipo é o dono das propriedades do mesmo
; padrão, como o tipo escrito antes das chaves em Rust. A desconstrução
; posicional (`var (a, b) = x;`, `x is (var a, var b)`) não escreve o nome de
; membro nenhum: `a` e `b` são variáveis novas, e não há o que ligar.
(property_pattern_clause (subpattern (identifier) @member ":"))
(recursive_pattern
  type: [(identifier) (qualified_name)] @member.of
  (property_pattern_clause (subpattern (identifier) @member ":")))

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

; O tipo que a assinatura ou a variável escreve para um nome local
; (`Pedido pedido`, `IRepo repo = ...;`): a chamada feita sobre esse nome, ou
; sobre os campos dele, é do tipo. O tipo embrulhado (`List<Pedido>`, o vetor)
; é o de fora.
(parameter type: [(identifier) (generic_name) (qualified_name)] @local.type name: (identifier) @local)
(variable_declaration type: [(identifier) (generic_name) (qualified_name)] @local.type (variable_declarator name: (identifier) @local))

; Os textos fixos: o literal de texto escrito no código. O motor guarda o que
; tem duas palavras ou forma de caminho ou chave, com a marca (log, erro ou
; texto) e a declaração que o contém.
(string_literal) @text
(verbatim_string_literal) @text
(raw_string_literal) @text
(interpolated_string_expression) @text
