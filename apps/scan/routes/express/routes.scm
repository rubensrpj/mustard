; A rota: `router.post('/pedidos', criar)`. O caminho é o primeiro argumento,
; e quem atende, o último; o que vem entre eles (`auth`) não conta. O caminho
; que não é texto escrito ali (`router.get(caminho, criar)`) não faz rota.
(call_expression
  function: (member_expression object: (_) @route.receiver property: (property_identifier) @route.method)
  arguments: (arguments . (string) @route.path (_) @route.handler .))

; A montagem: `app.use('/api', router)` soma o prefixo às rotas registradas
; em `router`, ou às do arquivo de onde vem o `router` trazido por import. O
; objeto que recebe a montagem (`app`) é o lugar dela: o prefixo posto nele
; vem na frente (`api.use('/pedidos', pedidos)` com `app.use('/api', api)`).
((call_expression
   function: (member_expression object: (_) @route.receiver property: (property_identifier) @_use)
   arguments: (arguments . (string) @route.prefix (identifier) @route.target .))
 (#eq? @_use "use"))

; O nome que recebe o módulo inteiro: o import padrão
; (`import aves from './aves'`) e o que recebe o `require`
; (`const aves = require('./aves')`). Montado, ele leva o prefixo a todas as
; rotas do arquivo de onde vem.
(import_clause (identifier) @route.module)

((variable_declarator
   name: (identifier) @route.module
   value: (call_expression function: (identifier) @_require))
 (#eq? @_require "require"))
