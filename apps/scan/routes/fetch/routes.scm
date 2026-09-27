; A chamada solta, com o método das opções ou sem ele:
; `fetch('/api/pedidos')`, `fetch('/api/pedidos', { method: 'POST' })`. O
; caminho é o primeiro argumento; o que não é texto escrito ali
; (`fetch(endereco)`) não faz chamada.
((call_expression
   function: (identifier) @client.method
   arguments: (arguments . [(string) (template_string)] @client.path))
 (#eq? @client.method "fetch"))

((call_expression
   function: (identifier) @client.method
   arguments: (arguments
     . [(string) (template_string)] @client.path
     (object (pair key: (property_identifier) @_method value: (string) @client.option))))
 (#eq? @client.method "fetch")
 (#eq? @_method "method"))
