; A chamada solta, com o método das opções ou sem ele:
; `fetch('/api/pedidos')`, `fetch('/api/pedidos', { method: 'POST' })`. O
; caminho é o primeiro argumento; o que não é texto escrito ali
; (`fetch(endereco)`) não faz chamada. O texto somado a um valor
; (`'/api/pedidos/' + id`) é o caminho com um parâmetro no lugar do valor.
((call_expression
   function: (identifier) @client.method
   arguments: (arguments
     . [(string) @client.path
        (template_string) @client.path
        (binary_expression
          left: [(string) (template_string)] @client.path
          operator: "+"
          right: [(identifier) (member_expression) (call_expression) (subscript_expression) (parenthesized_expression)] @client.path.tail)]))
 (#eq? @client.method "fetch"))

((call_expression
   function: (identifier) @client.method
   arguments: (arguments
     . [(string) @client.path
        (template_string) @client.path
        (binary_expression
          left: [(string) (template_string)] @client.path
          operator: "+"
          right: [(identifier) (member_expression) (call_expression) (subscript_expression) (parenthesized_expression)] @client.path.tail)]
     (object (pair key: (property_identifier) @_method value: (string) @client.option))))
 (#eq? @client.method "fetch")
 (#eq? @_method "method"))
