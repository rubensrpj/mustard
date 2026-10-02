; O prefixo da classe: o caminho do `#[Route('/api')]` escrito sobre ela, que
; vale para as rotas dos métodos dentro dela.
((class_declaration
   attributes: (attribute_list
     (attribute_group
       (attribute
         [(name) (qualified_name)] @_route
         parameters: (arguments . (argument [(string) (encapsed_string)] @route.prefix)))))) @route.scope
 (#match? @_route "(^|\\\\)Route$"))

; A rota: `#[Route('/pedidos/{id}')]` no método, que a atende. Sem `methods`,
; o método padrão.
((method_declaration
   attributes: (attribute_list
     (attribute_group
       (attribute
         [(name) (qualified_name)] @_route
         parameters: (arguments . (argument [(string) (encapsed_string)] @route.path))))))
 (#match? @_route "(^|\\\\)Route$"))

; Com `methods: ['GET', 'POST']` ou `methods: 'GET'`, uma rota por método
; escrito.
((method_declaration
   attributes: (attribute_list
     (attribute_group
       (attribute
         [(name) (qualified_name)] @_route
         parameters: (arguments
           .
           (argument [(string) (encapsed_string)] @route.path)
           (argument
             name: (name) @_methods
             [(array_creation_expression (array_element_initializer [(string) (encapsed_string)] @route.method.text))
              (string) @route.method.text
              (encapsed_string) @route.method.text]))))))
 (#match? @_route "(^|\\\\)Route$")
 (#eq? @_methods "methods"))
