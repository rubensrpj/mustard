; A rota: `path('pedidos/<int:id>/', views.ler_pedido)` na lista
; `urlpatterns`. O caminho é o primeiro argumento; quem atende, o nome do
; segundo.
((assignment
   left: (identifier) @_list
   right: (list
     (call
       function: (identifier) @_path
       arguments: (argument_list . (string) @route.path . [(identifier) (attribute)] @route.handler))))
 (#eq? @_list "urlpatterns")
 (#match? @_path "^(path|re_path|url)$"))

((augmented_assignment
   left: (identifier) @_list
   right: (list
     (call
       function: (identifier) @_path
       arguments: (argument_list . (string) @route.path . [(identifier) (attribute)] @route.handler))))
 (#eq? @_list "urlpatterns")
 (#match? @_path "^(path|re_path|url)$"))

; A visão de classe, `PedidoView.as_view()`: quem atende é a classe.
((assignment
   left: (identifier) @_list
   right: (list
     (call
       function: (identifier) @_path
       arguments: (argument_list
         . (string) @route.path
         . (call function: (attribute object: (_) @route.handler attribute: (identifier) @_as_view))))))
 (#eq? @_list "urlpatterns")
 (#match? @_path "^(path|re_path|url)$")
 (#eq? @_as_view "as_view"))

((augmented_assignment
   left: (identifier) @_list
   right: (list
     (call
       function: (identifier) @_path
       arguments: (argument_list
         . (string) @route.path
         . (call function: (attribute object: (_) @route.handler attribute: (identifier) @_as_view))))))
 (#eq? @_list "urlpatterns")
 (#match? @_path "^(path|re_path|url)$")
 (#eq? @_as_view "as_view"))

; A montagem: `path('api/', include('loja.urls'))` soma o prefixo a todas as
; rotas do arquivo que o texto nomeia.
((assignment
   left: (identifier) @_list
   right: (list
     (call
       function: (identifier) @_path
       arguments: (argument_list
         . (string) @route.prefix
         . (call function: (identifier) @_include arguments: (argument_list . (string) @route.target.module))))))
 (#eq? @_list "urlpatterns")
 (#match? @_path "^(path|re_path|url)$")
 (#eq? @_include "include"))

((augmented_assignment
   left: (identifier) @_list
   right: (list
     (call
       function: (identifier) @_path
       arguments: (argument_list
         . (string) @route.prefix
         . (call function: (identifier) @_include arguments: (argument_list . (string) @route.target.module))))))
 (#eq? @_list "urlpatterns")
 (#match? @_path "^(path|re_path|url)$")
 (#eq? @_include "include"))
