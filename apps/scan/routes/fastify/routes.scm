; A rota: `f.get('/rapido/:id', ler)`. O caminho é o primeiro argumento, e
; quem atende, o último; as opções entre eles (`{ schema }`) não contam.
(call_expression
  function: (member_expression object: (_) @route.receiver property: (property_identifier) @route.method)
  arguments: (arguments . (string) @route.path (_) @route.handler .))

; A rota do objeto: `f.route({ method: 'POST', url: '/x', handler: h })`, com
; o método em texto ou em lista, antes do `url` ou depois dele.
((call_expression
   function: (member_expression object: (_) @route.receiver property: (property_identifier) @_route)
   arguments: (arguments
     .
     (object
       (pair key: (property_identifier) @_method value: [(string) @route.method.text (array (string) @route.method.text)])
       (pair key: (property_identifier) @_url value: (string) @route.path)
       (pair key: (property_identifier) @_handler value: (_) @route.handler))))
 (#eq? @_route "route")
 (#eq? @_method "method")
 (#eq? @_url "url")
 (#eq? @_handler "handler"))

((call_expression
   function: (member_expression object: (_) @route.receiver property: (property_identifier) @_route)
   arguments: (arguments
     .
     (object
       (pair key: (property_identifier) @_url value: (string) @route.path)
       (pair key: (property_identifier) @_method value: [(string) @route.method.text (array (string) @route.method.text)])
       (pair key: (property_identifier) @_handler value: (_) @route.handler))))
 (#eq? @_route "route")
 (#eq? @_method "method")
 (#eq? @_url "url")
 (#eq? @_handler "handler"))

; A montagem: `f.register(plugin, { prefix: '/api' })` soma o prefixo às
; rotas escritas na função `plugin`, aqui ou no arquivo de onde ela vem.
((call_expression
   function: (member_expression property: (property_identifier) @_register)
   arguments: (arguments
     .
     (identifier) @route.target
     .
     (object (pair key: (property_identifier) @_prefix value: (string) @route.prefix))))
 (#eq? @_register "register")
 (#eq? @_prefix "prefix"))

; O nome que recebe o módulo inteiro: o import padrão
; (`import rotas from './rotas'`) e o que recebe o `require`
; (`const rotas = require('./rotas')`). Montado, ele leva o prefixo a todas
; as rotas do arquivo de onde vem.
(import_clause (identifier) @route.module)

((variable_declarator
   name: (identifier) @route.module
   value: (call_expression function: (identifier) @_require))
 (#eq? @_require "require"))
