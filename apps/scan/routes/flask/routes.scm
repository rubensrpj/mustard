; A rota pelo nome do método: `@bp.get("/x")` sobre a função. O caminho é o
; primeiro argumento; o objeto antes do nome, a aplicação ou o blueprint em
; que ela se registra; a função decorada, quem atende.
(decorated_definition
  (decorator
    (call
      function: (attribute object: (_) @route.receiver attribute: (identifier) @route.method)
      arguments: (argument_list . (string) @route.path)))
  definition: (function_definition name: (identifier) @route.handler))

; O `@app.route("/x")`: sem a lista `methods`, a rota do método padrão.
((decorated_definition
  (decorator
    (call
      function: (attribute object: (_) @route.receiver attribute: (identifier) @_route)
      arguments: (argument_list . (string) @route.path)))
  definition: (function_definition name: (identifier) @route.handler))
 (#eq? @_route "route"))

; Com a lista `methods=["GET", "POST"]`, uma rota por texto escrito nela.
((decorated_definition
  (decorator
    (call
      function: (attribute object: (_) @route.receiver attribute: (identifier) @_route)
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
 (#eq? @_route "route")
 (#eq? @_methods "methods"))

; O grupo: `Blueprint('pedidos', __name__, url_prefix='/pedidos')`, com o
; prefixo do argumento `url_prefix`.
((call
   function: [(identifier) @_blueprint (attribute attribute: (identifier) @_blueprint)]
   arguments: (argument_list
     (keyword_argument name: (identifier) @_prefix value: (string) @route.prefix))) @route.nest
 (#eq? @_blueprint "Blueprint")
 (#eq? @_prefix "url_prefix"))

; O nome que guarda um grupo: `bp = Blueprint(…, url_prefix='/pedidos')`.
((assignment left: (identifier) @route.variable right: (call) @route.value)
 (#match? @route.value "Blueprint"))

; A montagem: `app.register_blueprint(bp, url_prefix='/api')` soma o
; prefixo às rotas registradas em `bp`, aqui ou no arquivo de onde ele vem;
; em `pedidos.bp`, o nome é o que vem depois do ponto.
((call
   function: (attribute attribute: (identifier) @_register)
   arguments: (argument_list
     . [(identifier) @route.target (attribute attribute: (identifier) @route.target)]
     (keyword_argument name: (identifier) @_prefix value: (string) @route.prefix)))
 (#eq? @_register "register_blueprint")
 (#eq? @_prefix "url_prefix"))
