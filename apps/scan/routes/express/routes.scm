; A rota: `router.post('/pedidos', criar)`. O caminho é o primeiro argumento,
; e quem atende, o último; o que vem entre eles (`auth`) não conta. O caminho
; que não é texto escrito ali (`router.get(caminho, criar)`) não faz rota.
(call_expression
  function: (member_expression object: (_) @route.receiver property: (property_identifier) @route.method)
  arguments: (arguments . (string) @route.path (_) @route.handler .))

; A montagem: `app.use('/api', router)` soma o prefixo às rotas registradas
; em `router`.
((call_expression
   function: (member_expression property: (property_identifier) @_use)
   arguments: (arguments . (string) @route.prefix (identifier) @route.target .))
 (#eq? @_use "use"))
