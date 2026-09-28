; A rota: o decorador `@router.get("/{id}")` sobre a função. O caminho é o
; primeiro argumento; o objeto antes do nome, o roteador em que ela se
; registra; a função decorada, quem atende.
(decorated_definition
  (decorator
    (call
      function: (attribute object: (_) @route.receiver attribute: (identifier) @route.method)
      arguments: (argument_list . (string) @route.path)))
  definition: (function_definition name: (identifier) @route.handler))

; O `@app.api_route("/x")`: sem a lista `methods`, a rota do método padrão.
((decorated_definition
  (decorator
    (call
      function: (attribute object: (_) @route.receiver attribute: (identifier) @_api_route)
      arguments: (argument_list . (string) @route.path)))
  definition: (function_definition name: (identifier) @route.handler))
 (#eq? @_api_route "api_route"))

; Com a lista `methods=["GET", "POST"]`, uma rota por texto escrito nela.
((decorated_definition
  (decorator
    (call
      function: (attribute object: (_) @route.receiver attribute: (identifier) @_api_route)
      arguments: (argument_list
        . (string) @route.path
        (keyword_argument
          name: (identifier) @_methods
          value: [
            (list (string) @route.method.text)
            (tuple (string) @route.method.text)
            (set (string) @route.method.text)
          ]))))
  definition: (function_definition name: (identifier) @route.handler))
 (#eq? @_api_route "api_route")
 (#eq? @_methods "methods"))

; O grupo: `APIRouter(prefix="/pedidos")`, com o prefixo do argumento
; `prefix`.
((call
   function: [(identifier) @_router (attribute attribute: (identifier) @_router)]
   arguments: (argument_list
     (keyword_argument name: (identifier) @_prefix value: (string) @route.prefix))) @route.nest
 (#eq? @_router "APIRouter")
 (#eq? @_prefix "prefix"))

; O nome que guarda um grupo: `router = APIRouter(prefix="/pedidos")`.
((assignment left: (identifier) @route.variable right: (call) @route.value)
 (#match? @route.value "APIRouter"))

; A montagem: `app.include_router(router, prefix="/api")` soma o prefixo às
; rotas registradas em `router`, aqui ou no arquivo de onde ele vem; em
; `pedidos.router`, o nome é o que vem depois do ponto.
((call
   function: (attribute attribute: (identifier) @_include)
   arguments: (argument_list
     . [(identifier) @route.target (attribute attribute: (identifier) @route.target)]
     (keyword_argument name: (identifier) @_prefix value: (string) @route.prefix)))
 (#eq? @_include "include_router")
 (#eq? @_prefix "prefix"))
