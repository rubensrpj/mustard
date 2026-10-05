; A rota: `Route::get('/pedidos/{id}', [PedidoController::class, 'show'])`. O
; caminho é o primeiro argumento; quem atende, o segundo: o texto do fim da
; lista, o texto `'Controlador@acao'`, a classe ou a função escrita ali.
((scoped_call_expression
   scope: [(name) (qualified_name)] @_route
   name: (name) @route.method
   arguments: (arguments
     .
     (argument [(string) (encapsed_string)] @route.path)
     .
     (argument
       [(array_creation_expression
          (array_element_initializer)
          .
          (array_element_initializer [(string) (encapsed_string)] @route.handler.text)
          .)
        (string) @route.handler.text
        (encapsed_string) @route.handler.text
        (class_constant_access_expression . (name) @route.handler)
        (anonymous_function) @route.handler
        (arrow_function) @route.handler])
     .))
 (#match? @_route "(^|\\\\)Route$"))

; A mesma rota chamada na cadeia de outra (`Route::middleware('auth')->get(…)`).
((member_call_expression
   object: (scoped_call_expression scope: [(name) (qualified_name)] @_route)
   name: (name) @route.method
   arguments: (arguments
     .
     (argument [(string) (encapsed_string)] @route.path)
     .
     (argument
       [(array_creation_expression
          (array_element_initializer)
          .
          (array_element_initializer [(string) (encapsed_string)] @route.handler.text)
          .)
        (string) @route.handler.text
        (encapsed_string) @route.handler.text
        (class_constant_access_expression . (name) @route.handler)
        (anonymous_function) @route.handler
        (arrow_function) @route.handler])
     .))
 (#match? @_route "(^|\\\\)Route$"))

; `Route::match(['get', 'post'], '/x', …)`: uma rota por método da lista.
((scoped_call_expression
   scope: [(name) (qualified_name)] @_route
   name: (name) @_match
   arguments: (arguments
     .
     (argument (array_creation_expression (array_element_initializer [(string) (encapsed_string)] @route.method.text)))
     .
     (argument [(string) (encapsed_string)] @route.path)
     .
     (argument
       [(array_creation_expression
          (array_element_initializer)
          .
          (array_element_initializer [(string) (encapsed_string)] @route.handler.text)
          .)
        (string) @route.handler.text
        (encapsed_string) @route.handler.text
        (class_constant_access_expression . (name) @route.handler)
        (anonymous_function) @route.handler
        (arrow_function) @route.handler])
     .))
 (#match? @_route "(^|\\\\)Route$")
 (#eq? @_match "match"))

; O recurso: `Route::resource('pedidos', PedidoController::class)` e
; `Route::apiResource(…)`. As rotas dele vêm da tabela `resources`.
((scoped_call_expression
   scope: [(name) (qualified_name)] @_route
   name: (name) @route.resource
   arguments: (arguments . (argument [(string) (encapsed_string)] @route.path)))
 (#match? @_route "(^|\\\\)Route$")
 (#match? @route.resource "^(resource|apiResource)$"))

; O prefixo das rotas escritas na função do grupo:
; `Route::prefix('api')->group(function () { … })`.
((member_call_expression
   object: (scoped_call_expression
     scope: [(name) (qualified_name)] @_route
     name: (name) @_prefix
     arguments: (arguments . (argument [(string) (encapsed_string)] @route.prefix)))
   name: (name) @_group
   arguments: (arguments . (argument [(anonymous_function) (arrow_function)] @route.scope)))
 (#match? @_route "(^|\\\\)Route$")
 (#eq? @_prefix "prefix")
 (#eq? @_group "group"))

; O mesmo no meio de uma cadeia: `Route::middleware('auth')->prefix('v1')->group(…)`.
((member_call_expression
   object: (member_call_expression
     name: (name) @_prefix
     arguments: (arguments . (argument [(string) (encapsed_string)] @route.prefix)))
   name: (name) @_group
   arguments: (arguments . (argument [(anonymous_function) (arrow_function)] @route.scope)))
 (#eq? @_prefix "prefix")
 (#eq? @_group "group"))

; E com outra chamada entre o prefixo e o grupo:
; `Route::prefix('v1')->middleware('auth')->group(…)`.
((member_call_expression
   object: (member_call_expression
     object: [(scoped_call_expression
                name: (name) @_prefix
                arguments: (arguments . (argument [(string) (encapsed_string)] @route.prefix)))
              (member_call_expression
                name: (name) @_prefix
                arguments: (arguments . (argument [(string) (encapsed_string)] @route.prefix)))])
   name: (name) @_group
   arguments: (arguments . (argument [(anonymous_function) (arrow_function)] @route.scope)))
 (#eq? @_prefix "prefix")
 (#eq? @_group "group"))

; O grupo com o prefixo na lista de opções:
; `Route::group(['prefix' => 'api'], function () { … })`.
((scoped_call_expression
   scope: [(name) (qualified_name)] @_route
   name: (name) @_group
   arguments: (arguments
     .
     (argument
       (array_creation_expression
         (array_element_initializer
           [(string) (encapsed_string)] @_key
           .
           [(string) (encapsed_string)] @route.prefix)))
     .
     (argument [(anonymous_function) (arrow_function)] @route.scope)))
 (#match? @_route "(^|\\\\)Route$")
 (#eq? @_group "group")
 (#match? @_key "^.prefix.$"))
