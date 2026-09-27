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

; O grupo: `var pedidos = app.MapGroup("/pedidos");` soma o prefixo às rotas
; registradas em `pedidos`.
((variable_declarator
   name: (identifier) @route.target
   (invocation_expression
     function: (member_access_expression name: (identifier) @_group)
     arguments: (argument_list . (argument . (string_literal) @route.prefix) .)))
 (#eq? @_group "MapGroup"))
