; A rota: todo `path('pedidos/<int:id>/', views.ler_pedido)`, `re_path()` ou
; `url()` escrito numa lista — a `urlpatterns`, uma lista guardada noutro nome
; ou a escrita dentro de um `include`. O caminho é o primeiro argumento; quem
; atende, o nome do segundo.
((list
   (call
     function: (identifier) @_path
     arguments: (argument_list . (string) @route.path . [(identifier) (attribute)] @route.handler)))
 (#match? @_path "^(path|re_path|url)$"))

; A visão de classe, `PedidoView.as_view()`: quem atende é a classe.
((list
   (call
     function: (identifier) @_path
     arguments: (argument_list
       . (string) @route.path
       . (call function: (attribute object: (_) @route.handler attribute: (identifier) @_as_view)))))
 (#match? @_path "^(path|re_path|url)$")
 (#eq? @_as_view "as_view"))

; A montagem de um arquivo: `path('api/', include('loja.urls'))` soma o
; prefixo a todas as rotas do arquivo que o texto nomeia, também na tupla com
; o nome do app (`include(('loja.urls', 'loja'))`).
((list
   (call
     function: (identifier) @_path
     arguments: (argument_list
       . (string) @route.prefix
       . (call
           function: (identifier) @_include
           arguments: (argument_list . [(string) @route.target.module (tuple . (string) @route.target.module)])))))
 (#match? @_path "^(path|re_path|url)$")
 (#eq? @_include "include"))

; A montagem de uma lista escrita ali mesmo, sozinha ou numa tupla com o nome
; do app: `path('api/', include([path('pedidos/', views.ler)]))` soma o
; prefixo às rotas escritas dentro dela.
((list
   (call
     function: (identifier) @_path
     arguments: (argument_list
       . (string) @route.prefix
       . (call
           function: (identifier) @_include
           arguments: (argument_list . [(list) @route.scope (tuple . (list) @route.scope)])))))
 (#match? @_path "^(path|re_path|url)$")
 (#eq? @_include "include"))

; A montagem de uma lista guardada num nome, sozinho ou numa tupla com o nome
; do app: `path('api/', include(extra))` soma o prefixo às rotas escritas na
; lista `extra`, a deste arquivo ou, quando um import traz o nome, a do
; arquivo de onde ele vem. O nome que o import traz como módulo
; (`from loja import urls as loja_urls`) monta o arquivo inteiro.
((list
   (call
     function: (identifier) @_path
     arguments: (argument_list
       . (string) @route.prefix
       . (call
           function: (identifier) @_include
           arguments: (argument_list . [(identifier) @route.target (tuple . (identifier) @route.target)])))))
 (#match? @_path "^(path|re_path|url)$")
 (#eq? @_include "include"))
