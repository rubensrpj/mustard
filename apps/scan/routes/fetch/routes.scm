; A chamada solta, com o método das opções ou sem ele:
; `fetch('/api/pedidos')`, `fetch('/api/pedidos', { method: 'POST' })`. O
; caminho é o primeiro argumento; o que não é texto escrito ali
; (`fetch(endereco)`) não faz chamada. A soma que começa por texto
; (`'/api/pedidos/' + id + '/itens'`) é o caminho com um parâmetro no lugar
; de cada valor.
((call_expression
   function: (identifier) @client.method
   arguments: (arguments
     . [(string) @client.path
        (template_string) @client.path
        (binary_expression operator: "+") @client.path.sum]))
 (#eq? @client.method "fetch"))

((call_expression
   function: (identifier) @client.method
   arguments: (arguments
     . [(string) @client.path
        (template_string) @client.path
        (binary_expression operator: "+") @client.path.sum]
     (object (pair key: (property_identifier) @_method value: (string) @client.option))))
 (#eq? @client.method "fetch")
 (#eq? @_method "method"))

; A soma, pedaço por pedaço: o primeiro é texto ou outra soma; o que fica
; entre dois sinais de somar é um pedaço, e o texto escrito ali leva
; `client.sum.text`.
(binary_expression
  left: [(string) @client.sum.text (template_string) @client.sum.text (binary_expression)]
  operator: "+" @client.sum.plus) @client.sum

(binary_expression
  left: [(string) (template_string) (binary_expression)]
  operator: "+"
  right: [(string) (template_string)] @client.sum.text)
