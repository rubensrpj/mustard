; A rota: `http.HandleFunc("/pedidos/", ler)` e `mux.Handle("GET /aves/{id}", h)`.
; O caminho é o primeiro argumento, e quem atende, o último. O objeto antes do
; nome é o pacote ou o roteador em que ela se registra.
((call_expression
   function: (selector_expression operand: (_) @route.receiver field: (field_identifier) @_handle)
   arguments: (argument_list . (interpreted_string_literal) @route.path (_) @route.handler .))
 (#match? @_handle "^(HandleFunc|Handle)$"))
