; O atributo na função: `#[get("/aves/{id}")]`, também pelo caminho
; (`#[actix_web::get("/x")]`). Quem atende é a função que ele enfeita.
(attribute_item
  (attribute
    [(identifier) @route.method (scoped_identifier name: (identifier) @route.method)]
    arguments: (token_tree . (string_literal) @route.path)))

; O `#[route("/x", method = "GET", method = "POST")]`: uma rota por método
; escrito.
((attribute_item
   (attribute
     [(identifier) @_route (scoped_identifier name: (identifier) @_route)]
     arguments: (token_tree . (string_literal) @route.path (identifier) @_method . (string_literal) @route.method.text)))
 (#eq? @_route "route")
 (#eq? @_method "method"))

; A rota pela chamada: `.route(web::get().to(h))` no grupo de um
; `web::resource("/x")`, que dá o caminho, e `.route("/x", web::post().to(h))`,
; com o caminho escrito ali. O método é o nome da função, e quem atende, o
; argumento do `.to`.
((call_expression
   function: (field_expression value: (_) @route.receiver field: (field_identifier) @_route)
   arguments: (arguments
     .
     (call_expression
       function: (field_expression
         value: (call_expression function: [(identifier) @route.method (scoped_identifier name: (identifier) @route.method)])
         field: (field_identifier) @_to)
       arguments: (arguments . (_) @route.handler .))
     .))
 (#eq? @_route "route")
 (#eq? @_to "to"))

((call_expression
   function: (field_expression value: (_) @route.receiver field: (field_identifier) @_route)
   arguments: (arguments
     .
     (string_literal) @route.path
     .
     (call_expression
       function: (field_expression
         value: (call_expression function: [(identifier) @route.method (scoped_identifier name: (identifier) @route.method)])
         field: (field_identifier) @_to)
       arguments: (arguments . (_) @route.handler .))
     .))
 (#eq? @_route "route")
 (#eq? @_to "to"))

; O grupo: `web::scope("/api")` e `web::resource("/x")`. O valor que começa
; por ele é o grupo, em qualquer ponto da cadeia.
((call_expression
   function: [(identifier) @_group (scoped_identifier name: (identifier) @_group)]
   arguments: (arguments . (string_literal) @route.prefix .)) @route.nest
 (#match? @_group "^(scope|resource)$"))

; A montagem no grupo: `.service(ler)` e `.configure(config)` chamados na
; cadeia de um grupo levam o prefixo dele à função que nomeiam, aqui ou no
; arquivo de onde o nome vem.
((call_expression
   function: (field_expression value: (_) @route.receiver field: (field_identifier) @_mount)
   arguments: (arguments . [(identifier) @route.target (scoped_identifier name: (identifier) @route.target)] .))
 (#match? @_mount "^(service|configure)$"))
