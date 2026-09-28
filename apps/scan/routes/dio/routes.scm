; A chamada: `dio.get('/pedidos/$id')`, `this.dio.post('/pedidos', data: p)`.
; O caminho é o primeiro argumento; o que não é texto escrito ali
; (`dio.get(caminho)`) não faz chamada. A soma que começa por texto
; (`'/pedidos/' + id + '/itens'`) é o caminho com um parâmetro no lugar de
; cada valor. O objeto precisa ser um cliente do arquivo; `this.dio` vale
; como `dio`.
(method_invocation
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
        [(string_literal) @client.path
         (additive_expression) @client.path.sum]))))

; A soma, pedaço por pedaço: o primeiro é texto ou outra soma; o que fica
; entre dois sinais de somar é um pedaço — `pedido.id` inteiro —, e o texto
; escrito ali leva `client.sum.text`.
((additive_expression
   . [(string_literal) @client.sum.text (additive_expression)]
   . (additive_operator) @client.sum.plus) @client.sum
 (#eq? @client.sum.plus "+"))

((additive_expression
   (additive_operator) @_plus
   . (string_literal) @client.sum.text .)
 (#eq? @_plus "+"))

; O cliente feito e guardado num nome, com a base ou sem ela:
; `final dio = Dio(BaseOptions(baseUrl: 'https://loja.com/api'));`,
; `Dio(const BaseOptions(…))`, `final dio = Dio();`, e o `: _dio = Dio(…)`
; do construtor.
((_
   (identifier) @client.instance
   .
   (method_invocation
     function: (identifier) @_dio
     arguments: (argument_part
       (arguments
         .
         (argument
           [(method_invocation
              function: (identifier) @_options
              arguments: (argument_part
                (arguments (named_argument (label (identifier) @_base) (string_literal) @client.base))))
            (const_object_expression
              (type_identifier) @_options
              (arguments (named_argument (label (identifier) @_base) (string_literal) @client.base)))])))) @client.made)
 (#eq? @_dio "Dio")
 (#eq? @_options "BaseOptions")
 (#eq? @_base "baseUrl"))

((_ (identifier) @client.instance . (method_invocation function: (identifier) @_dio) @client.made)
 (#eq? @_dio "Dio"))

; O cliente declarado com o tipo, sem base: o campo ou a variável
; (`final Dio _dio;`) e o parâmetro (`Repo(Dio dio)`).
((type_identifier) @_type
 .
 (initialized_identifier_list (initialized_identifier . (identifier) @client.instance) @client.made)
 (#eq? @_type "Dio"))

((formal_parameter (type_identifier) @_type . name: (identifier) @client.instance) @client.made
 (#eq? @_type "Dio"))
