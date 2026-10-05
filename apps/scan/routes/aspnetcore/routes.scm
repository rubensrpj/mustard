; O prefixo da classe: o caminho do `[Route("…")]` escrito sobre ela, que vale
; para as ações dentro dela; o nome da classe troca o marcador.
((class_declaration
   (attribute_list
     (attribute
       name: (identifier) @_attribute
       (attribute_argument_list . (attribute_argument . (string_literal) @route.prefix))))
   name: (identifier) @route.class) @route.scope
 (#eq? @_attribute "Route"))

; O atributo da ação, com o caminho. Quem atende a rota é o método em que ele
; está escrito.
(method_declaration
  (attribute_list
    (attribute
      name: (identifier) @route.method
      (attribute_argument_list . (attribute_argument . (string_literal) @route.path)))))

; O atributo da ação sem caminho (`[HttpGet]`): a rota fica no prefixo da
; classe.
((method_declaration
   (attribute_list
     (attribute name: (identifier) @route.method) @_attribute))
 (#not-match? @_attribute "[(]"))

; A API mínima: `app.MapGet("/x", Ler)`, com o caminho escrito ali.
(invocation_expression
  function: (member_access_expression expression: (_) @route.receiver name: (identifier) @route.method)
  arguments: (argument_list . (argument . (string_literal) @route.path) (argument) @route.handler .))

; O grupo: `X.MapGroup("/pedidos")` faz do objeto `X` um grupo com mais este
; prefixo. O valor que começa por ele é o grupo, em qualquer ponto da cadeia.
((invocation_expression
   function: (member_access_expression expression: (_) @route.receiver name: (identifier) @_group)
   arguments: (argument_list . (argument . (string_literal) @route.prefix) .)) @route.nest
 (#eq? @_group "MapGroup"))

; A variável que guarda um grupo: `var pedidos = app.MapGroup("/pedidos");`.
((variable_declarator name: (identifier) @route.variable (_) @route.value)
 (#match? @route.value "MapGroup"))

; O parâmetro de um método que leva um grupo, pelo tipo. A posição dele entre
; os parâmetros é o lugar a que a chamada entrega o grupo.
((method_declaration
   parameters: (parameter_list
     (parameter type: (_) @_type name: (identifier) @route.parameter.name) @route.parameter))
 (#match? @_type "(^|[.])(IEndpointRouteBuilder|RouteGroupBuilder)[?]?$"))

; O receptor do método de extensão, marcado com `this`: recebe o objeto
; escrito antes do nome na chamada, e fica fora da contagem dos outros.
((method_declaration
   parameters: (parameter_list (parameter (modifier) @_this) @route.parameter.receiver))
 (#eq? @_this "this"))

; A chamada que pode entregar um grupo: pelo objeto antes do nome
; (`api.MapOrders()`) e por cada argumento (`module.MapEndpoints(api, v)`).
((invocation_expression
   function: (member_access_expression expression: (_) @route.receiver name: (identifier) @route.call))
 (#not-eq? @route.call "MapGroup"))

((invocation_expression
   function: (member_access_expression name: (identifier) @route.call)
   arguments: (argument_list (argument) @route.argument))
 (#not-eq? @route.call "MapGroup"))

(invocation_expression
  function: (identifier) @route.call
  arguments: (argument_list (argument) @route.argument))
