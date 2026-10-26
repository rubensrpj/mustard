; Go — package (namespace), imports, top-level types and funcs.
(package_clause (package_identifier) @namespace)
(import_spec path: (interpreted_string_literal) @import)
; O apelido de `import s "strings"` é o nome que o import traz: o próprio
; pacote, escrito antes de outro nome (`s.Join()`).
(import_spec name: (package_identifier) @imported)

(type_spec name: (type_identifier) @name type: (struct_type)) @definition.struct
(type_spec name: (type_identifier) @name type: (interface_type)) @definition.interface
(type_alias name: (type_identifier) @name) @definition.type
(function_declaration name: (identifier) @name) @definition.function

; Constants — each name of a `const` spec, grouped or not; the value is not
; its header. A spec with no value repeats the one above it (`iota`).
; The names are the spec's own identifier children: the type is a type node
; and the value is inside its own list, and a `name:` field would give only
; the first name of `a, b = 1, 2`.
(const_spec (identifier) @name) @definition.const
(const_spec (identifier) @name value: (_) @value) @definition.const

; Membros — métodos com receptor e campos de struct. Os kinds de membro chegam
; ao mapa com as outras declarações do arquivo, e o grafo lista cada um sob o
; tipo dono dele. A tag de método segue o tags.scm do tree-sitter-go (MIT) —
; veja queries/README.md.
(method_declaration name: (field_identifier) @name) @definition.method
; O método com receptor é escrito fora do corpo do tipo: o dono dele é o tipo
; do receptor, com ou sem ponteiro e sem os argumentos de tipo.
(method_declaration
  receiver: (parameter_list (parameter_declaration type: [
    (type_identifier) @owner
    (pointer_type (type_identifier) @owner)
    (generic_type type: (type_identifier) @owner)
    (pointer_type (generic_type type: (type_identifier) @owner))]))
  name: (field_identifier) @name) @definition.method
(field_declaration name: (field_identifier) @name) @definition.field

; An interface method is a member like a receiver method; left uncaptured, its
; header was read as a call.
(method_elem name: (field_identifier) @name) @definition.method

; Os nomes que o corpo de uma função liga: no alcance que a língua dá a cada
; um, marcado mais abaixo, o mesmo nome escrito sozinho é deles.
(short_var_declaration left: (expression_list (identifier) @local))
(var_spec name: (identifier) @local)
(parameter_declaration name: (identifier) @local)
(range_clause left: (expression_list (identifier) @local))
; O nome do `switch v := x.(type)`, o do `case v := <-ch` do `select` e o
; parâmetro variádico (`xs ...int`). O resultado com nome já é
; `parameter_declaration`.
(type_switch_statement alias: (expression_list (identifier) @local))
(receive_statement left: (expression_list (identifier) @local))
(variadic_parameter_declaration name: (identifier) @local)

; Até onde vale cada nome ligado: o bloco, o `if`, o `for`, o `switch`, cada
; caso dele e do `select`, e as funções fecham o alcance. O valor que a
; declaração curta, o `var`, o `range`, o `switch` de tipo e o recebimento
; leem antes de ligar o nome fica fora do alcance dos nomes que eles ligam.
(block) @local.scope
(if_statement) @local.scope
(for_statement) @local.scope
(expression_switch_statement) @local.scope
(type_switch_statement) @local.scope
(expression_case) @local.scope
(type_case) @local.scope
(default_case) @local.scope
(communication_case) @local.scope
(function_declaration) @local.scope
(method_declaration) @local.scope
(func_literal) @local.scope
(short_var_declaration right: (_) @local.outside)
(var_spec value: (_) @local.outside)
(range_clause right: (_) @local.outside)
(type_switch_statement value: (_) @local.outside)
(receive_statement right: (_) @local.outside)

; A função entregue como valor, sem ser chamada ali: o nome escrito como
; argumento (`sort.Slice(xs, menor)`, `http.HandleFunc("/", rotas.Inicio)`),
; à direita de uma declaração ou de uma atribuição (`f := dobro`) ou como
; valor de um campo (`Handler{Run: rodar}`). O motor liga o nome só a uma
; função ou a um método à vista.
(argument_list (identifier) @call.value)
(argument_list (selector_expression field: (field_identifier) @call.value))
(short_var_declaration right: (expression_list (identifier) @call.value))
(short_var_declaration right: (expression_list (selector_expression field: (field_identifier) @call.value)))
(assignment_statement right: (expression_list (identifier) @call.value))
(assignment_statement right: (expression_list (selector_expression field: (field_identifier) @call.value)))
(var_spec value: (expression_list (identifier) @call.value))
(keyed_element value: (literal_element (identifier) @call.value))

; O campo escrito depois do objeto, sem chamada ali: o lido ou escrito
; (`pedido.Total`, `p.Total`). O motor liga o nome só a um campo, como liga a
; chamada de método escrita depois do mesmo objeto.
(selector_expression field: (field_identifier) @member)

; Os textos fixos: o literal de texto escrito no código. O motor guarda o que
; tem duas palavras ou forma de caminho ou chave, com a marca (log, erro ou
; texto) e a declaração que o contém.
(interpreted_string_literal) @text
(raw_string_literal) @text

; A declaração que o Go exporta do pacote: a de nome que começa por letra
; maiúscula. Quem está fora do projeto pode usá-la, e a falta de uso dentro
; dele não diz que ela sobra. A função, o método e o campo valem pelo próprio
; nome; o tipo e a constante só no topo do arquivo, porque os escritos dentro
; de uma função são locais dela.
([(function_declaration name: (identifier) @_name)
  (method_declaration name: (field_identifier) @_name)
  (method_elem name: (field_identifier) @_name)
  (field_declaration name: (field_identifier) @_name)] @visible
  (#match? @_name "^\\p{Lu}"))
((source_file (type_declaration
  [(type_spec name: (type_identifier) @_name)
   (type_alias name: (type_identifier) @_name)] @visible))
  (#match? @_name "^\\p{Lu}"))
((source_file (const_declaration (const_spec name: (identifier) @_name) @visible))
  (#match? @_name "^\\p{Lu}"))
