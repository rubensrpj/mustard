; A rota: `g.GET("/aves/:id", lerAve)`. O caminho é o primeiro argumento, e
; quem atende, o último; o que vem entre eles (`auth`) não conta. O objeto
; antes do nome é o motor ou o grupo em que ela se registra.
(call_expression
  function: (selector_expression operand: (_) @route.receiver field: (field_identifier) @route.method)
  arguments: (argument_list . (interpreted_string_literal) @route.path (_) @route.handler .))

; O grupo: `r.Group("/api")` faz do objeto `r` um grupo com mais este
; prefixo. O valor que começa por ele é o grupo, em qualquer ponto da cadeia.
((call_expression
   function: (selector_expression operand: (_) @route.receiver field: (field_identifier) @_group)
   arguments: (argument_list . (interpreted_string_literal) @route.prefix)) @route.nest
 (#eq? @_group "Group"))

; A variável que guarda um grupo: `g := r.Group("/api")` e
; `var g = r.Group("/api")`.
((short_var_declaration
   left: (expression_list . (identifier) @route.variable .)
   right: (expression_list . (_) @route.value .))
 (#match? @route.value "Group"))

((var_spec
   name: (identifier) @route.variable
   value: (expression_list . (_) @route.value .))
 (#match? @route.value "Group"))
