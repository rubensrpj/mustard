; A rota: `router.get('/pedidos/<id>', ler)`. O caminho é o primeiro
; argumento, e quem atende, o último. O objeto antes do nome é o roteador em
; que ela se registra.
(method_invocation
  function: (unconditional_assignable_selector . (_) @route.receiver (identifier) @route.method .)
  arguments: (argument_part (arguments . (argument (string_literal) @route.path) (argument (_) @route.handler) .)))

; A montagem: `router.mount('/api/', outro.call)`, `router.mount('/api/', outro)`
; e `router.mount('/api/', rotas().call)` somam o prefixo às rotas de
; `outro` ou da função `rotas`, aqui ou no arquivo de onde o nome vem.
((method_invocation
   function: (unconditional_assignable_selector (identifier) @_mount .)
   arguments: (argument_part
     (arguments
       .
       (argument (string_literal) @route.prefix)
       .
       (argument
         [(identifier) @route.target
          (unconditional_assignable_selector . (identifier) @route.target)
          (unconditional_assignable_selector . (method_invocation function: (identifier) @route.target))
          (method_invocation function: (identifier) @route.target)])
       .)))
 (#eq? @_mount "mount"))

; A anotação no método: `@Route.get('/x')`. Quem atende é o método; a rota se
; registra no roteador da classe, pelo nome dela.
((class_definition
   name: (identifier) @route.receiver
   body: (class_body
     (annotation
       name: (scoped_identifier scope: (identifier) @_route name: (identifier) @route.method)
       (arguments . (argument (string_literal) @route.path)))))
 (#eq? @_route "Route"))

; A montagem pela anotação: `@Route.mount('/api/')` no getter que devolve o
; roteador de outra classe (`Router get _api => Outra().router;`).
((class_body
   (annotation
     name: (scoped_identifier scope: (identifier) @_route name: (identifier) @_mount)
     (arguments . (argument (string_literal) @route.prefix)))
   .
   (method_signature (getter_signature))
   .
   (function_body
     [(unconditional_assignable_selector . (method_invocation function: (identifier) @route.target))
      (method_invocation function: (identifier) @route.target)]))
 (#eq? @_route "Route")
 (#eq? @_mount "mount"))
