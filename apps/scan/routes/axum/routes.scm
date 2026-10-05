; O caminho: `.route("/pedidos/:id", get(ler).post(criar))`. Os métodos da
; corrente escrita no segundo argumento tomam este caminho.
((call_expression
   function: (field_expression field: (field_identifier) @_route)
   arguments: (arguments . (string_literal) @route.path . (_) @route.group .))
 (#eq? @_route "route"))

; Cada método da corrente: o primeiro, chamado pelo nome (`get(ler)`,
; `routing::get(ler)`), e os seguintes, chamados nele (`.post(criar)`). Fora
; do segundo argumento de um `.route`, não é rota.
(call_expression
  function: (identifier) @route.method.grouped
  arguments: (arguments . (_) @route.handler .))

(call_expression
  function: (scoped_identifier name: (identifier) @route.method.grouped)
  arguments: (arguments . (_) @route.handler .))

(call_expression
  function: (field_expression field: (field_identifier) @route.method.grouped)
  arguments: (arguments . (_) @route.handler .))

; O prefixo: `.nest("/api", …)`. Vale para o roteador escrito ali mesmo e,
; quando ali se chama uma função (`rotas()`, `pedidos::rotas()`), para as
; rotas escritas nela, neste arquivo ou no arquivo a que a chamada liga.
((call_expression
   function: (field_expression field: (field_identifier) @_nest)
   arguments: (arguments . (string_literal) @route.prefix . (_) @route.scope .))
 (#eq? @_nest "nest"))

((call_expression
   function: (field_expression field: (field_identifier) @_nest)
   arguments: (arguments . (string_literal) @route.prefix . (call_expression function: (identifier) @route.target) .))
 (#eq? @_nest "nest"))

((call_expression
   function: (field_expression field: (field_identifier) @_nest)
   arguments: (arguments
     . (string_literal) @route.prefix
     . (call_expression function: (scoped_identifier name: (identifier) @route.target)) .))
 (#eq? @_nest "nest"))
