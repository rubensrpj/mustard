; A chamada, com o endereço escrito ali (`http.get('https://loja.com/api/x')`,
; como nas versões antigas da biblioteca) ou feito pelo `Uri`:
; `http.get(Uri.parse('https://loja.com/api/pedidos/$id'))`,
; `http.post(Uri.https('loja.com', '/api/pedidos'), body: corpo)`. O objeto é
; o nome que o import traz (`import 'package:http/http.dart' as http;`) ou um
; cliente do arquivo; `this.cliente` vale como `cliente`. O endereço que não é
; texto escrito ali (`http.get(Uri.parse(url))`) não faz chamada.
(method_invocation
  function: (unconditional_assignable_selector
    .
    [(identifier) @client.receiver
     (unconditional_assignable_selector . (this) (identifier) @client.receiver .)]
    (identifier) @client.method
    .)
  arguments: (argument_part (arguments . (argument (string_literal) @client.path))))

((method_invocation
   function: (unconditional_assignable_selector
     .
     [(identifier) @client.receiver
      (unconditional_assignable_selector . (this) (identifier) @client.receiver .)]
     (identifier) @client.method
     .)
   arguments: (argument_part
     (arguments
       .
       (argument
         (method_invocation
           function: (unconditional_assignable_selector . (identifier) @_uri (identifier) @_parse .)
           arguments: (argument_part (arguments . (argument (string_literal) @client.path))))))))
 (#eq? @_uri "Uri")
 (#eq? @_parse "parse"))

; No `Uri.https('máquina', '/caminho')`, o caminho é o segundo argumento.
((method_invocation
   function: (unconditional_assignable_selector
     .
     [(identifier) @client.receiver
      (unconditional_assignable_selector . (this) (identifier) @client.receiver .)]
     (identifier) @client.method
     .)
   arguments: (argument_part
     (arguments
       .
       (argument
         (method_invocation
           function: (unconditional_assignable_selector . (identifier) @_uri (identifier) @_scheme .)
           arguments: (argument_part (arguments . (argument) . (argument (string_literal) @client.path))))))))
 (#eq? @_uri "Uri")
 (#match? @_scheme "^https?$"))

; O cliente feito pela biblioteca e guardado num nome, sem base:
; `final cliente = http.Client();`.
((_
   (identifier) @client.instance
   .
   (method_invocation
     function: (unconditional_assignable_selector . (identifier) @client.factory (identifier) @_client .)) @client.made)
 (#eq? @_client "Client"))

; O cliente declarado com o tipo, sem base: o campo ou a variável
; (`final http.Client cliente;`) e o parâmetro (`Repo(http.Client cliente)`).
((type_identifier) @_type
 .
 (initialized_identifier_list (initialized_identifier . (identifier) @client.instance) @client.made)
 (#eq? @_type "Client"))

((formal_parameter (type_identifier) @_type . name: (identifier) @client.instance) @client.made
 (#eq? @_type "Client"))
