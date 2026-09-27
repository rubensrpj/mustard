; A chamada: `api.get('/pedidos')`, `axios.post(`/pedidos/${id}`, corpo)`. O
; caminho é o primeiro argumento; o que não é texto escrito ali
; (`api.get(caminho)`) não faz chamada.
(call_expression
  function: (member_expression object: (identifier) @client.receiver property: (property_identifier) @client.method)
  arguments: (arguments . [(string) (template_string)] @client.path))

; O cliente feito pela biblioteca e guardado num nome, com a base ou sem ela:
; `const api = axios.create({ baseURL: '/api' })`.
((variable_declarator
   name: (identifier) @client.instance
   value: (call_expression
     function: (member_expression object: (identifier) @client.factory property: (property_identifier) @_create)
     arguments: (arguments . (object (pair key: (property_identifier) @_base value: (string) @client.base)))) @client.made)
 (#eq? @_create "create")
 (#eq? @_base "baseURL"))

((variable_declarator
   name: (identifier) @client.instance
   value: (call_expression
     function: (member_expression object: (identifier) @client.factory property: (property_identifier) @_create)) @client.made)
 (#eq? @_create "create"))

; O cliente que o arquivo exporta como padrão:
; `export default axios.create({ baseURL: '/api' })`.
((export_statement
   value: (call_expression
     function: (member_expression object: (identifier) @client.factory property: (property_identifier) @_create)
     arguments: (arguments . (object (pair key: (property_identifier) @_base value: (string) @client.base)))) @client.made)
 (#eq? @_create "create")
 (#eq? @_base "baseURL"))

((export_statement
   value: (call_expression
     function: (member_expression object: (identifier) @client.factory property: (property_identifier) @_create)) @client.made)
 (#eq? @_create "create"))
