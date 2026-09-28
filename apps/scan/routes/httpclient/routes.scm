; Os nomes que só o cliente tem (`Http.GetFromJsonAsync<Pedido>("api/x")`,
; `Http.PostAsJsonAsync("api/x", p)`): a chamada conta com qualquer objeto,
; como o `Http` que a página recebe do framework sem o declarar. O caminho é o
; primeiro argumento; o que não é texto escrito ali (`Http.GetStringAsync(url)`)
; não faz chamada.
((invocation_expression
   function: (member_access_expression
     expression: (_) @client.receiver.any
     name: [(identifier) @client.method (generic_name . (identifier) @client.method)])
   arguments: (argument_list
     .
     (argument
       [(string_literal) (verbatim_string_literal) (raw_string_literal) (interpolated_string_expression)] @client.path)))
 (#match? @client.method "^(GetFromJsonAsync|GetStringAsync|GetByteArrayAsync|GetStreamAsync|PostAsJsonAsync|PutAsJsonAsync|PatchAsJsonAsync|DeleteFromJsonAsync)$"))

; Os nomes que outras bibliotecas também usam (`_http.GetAsync("api/x")`): a
; chamada conta só num cliente declarado ou feito no arquivo; `this._http`
; vale como `_http`.
((invocation_expression
   function: (member_access_expression
     expression: [(identifier) @client.receiver
                  (member_access_expression expression: "this" name: (identifier) @client.receiver)]
     name: (identifier) @client.method)
   arguments: (argument_list
     .
     (argument
       [(string_literal) (verbatim_string_literal) (raw_string_literal) (interpolated_string_expression)] @client.path)))
 (#match? @client.method "^(GetAsync|PostAsync|PutAsync|PatchAsync|DeleteAsync)$"))

; A mensagem montada na chamada:
; `Http.SendAsync(new HttpRequestMessage(HttpMethod.Get, "api/x"))`. O método
; é o do `HttpMethod`, e o caminho, o segundo argumento da mensagem.
((invocation_expression
   function: (member_access_expression expression: (_) @client.receiver.any name: (identifier) @client.method)
   arguments: (argument_list
     .
     (argument
       (object_creation_expression
         type: (identifier) @_message
         arguments: (argument_list
           .
           (argument (member_access_expression expression: (identifier) @_http name: (identifier) @client.option))
           .
           (argument
             [(string_literal) (verbatim_string_literal) (raw_string_literal) (interpolated_string_expression)] @client.path))))))
 (#eq? @client.method "SendAsync")
 (#eq? @_message "HttpRequestMessage")
 (#eq? @_http "HttpMethod"))

; O cliente declarado com o tipo, sem base: a propriedade
; (`[Inject] public HttpClient Http { get; set; }`), o campo, a variável e o
; parâmetro (`PedidosService(HttpClient http)`).
((property_declaration type: (_) @_type name: (identifier) @client.instance) @client.made
 (#match? @_type "(^|[.])HttpClient[?]?$"))

((variable_declaration type: (_) @_type (variable_declarator name: (identifier) @client.instance) @client.made)
 (#match? @_type "(^|[.])HttpClient[?]?$"))

((parameter type: (_) @_type name: (identifier) @client.instance) @client.made
 (#match? @_type "(^|[.])HttpClient[?]?$"))

; O cliente feito ali, guardado num nome: `var c = new HttpClient();`,
; `_http = new HttpClient { … };`, `var c = fabrica.CreateClient("api");`.
((variable_declarator name: (identifier) @client.instance (object_creation_expression type: (_) @_type) @client.made)
 (#match? @_type "(^|[.])HttpClient$"))

((assignment_expression left: (identifier) @client.instance right: (object_creation_expression type: (_) @_type) @client.made)
 (#match? @_type "(^|[.])HttpClient$"))

((variable_declarator
   name: (identifier) @client.instance
   (invocation_expression function: (member_access_expression name: (identifier) @_create)) @client.made)
 (#eq? @_create "CreateClient"))

; A base escrita ao fazer o cliente:
; `new HttpClient { BaseAddress = new Uri("https://loja.com/api/") }`.
((variable_declarator
   name: (identifier) @client.instance
   (object_creation_expression
     type: (_) @_type
     initializer: (initializer_expression
       (assignment_expression
         left: (identifier) @_base
         right: (object_creation_expression
           type: (identifier) @_uri
           arguments: (argument_list . (argument (string_literal) @client.base)))))) @client.made)
 (#match? @_type "(^|[.])HttpClient$")
 (#eq? @_base "BaseAddress")
 (#eq? @_uri "Uri"))

((assignment_expression
   left: (identifier) @client.instance
   right: (object_creation_expression
     type: (_) @_type
     initializer: (initializer_expression
       (assignment_expression
         left: (identifier) @_base
         right: (object_creation_expression
           type: (identifier) @_uri
           arguments: (argument_list . (argument (string_literal) @client.base)))))) @client.made)
 (#match? @_type "(^|[.])HttpClient$")
 (#eq? @_base "BaseAddress")
 (#eq? @_uri "Uri"))

; A base posta depois no cliente: `cliente.BaseAddress = new Uri("…");`.
((assignment_expression
   left: (member_access_expression expression: (identifier) @client.instance name: (identifier) @_base)
   right: (object_creation_expression
     type: (identifier) @_uri
     arguments: (argument_list . (argument (string_literal) @client.base)))) @client.made
 (#eq? @_base "BaseAddress")
 (#eq? @_uri "Uri"))
