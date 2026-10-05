; TSX — o que só a gramática do TSX tem. Estes padrões entram só na consulta
; do `.tsx` (`extra_queries` no languages.toml), depois dos de typescript/;
; na consulta do `.ts`, cuja gramática não tem estes nós, seriam pulados.

; O texto escrito solto numa tela, entre as marcas de um elemento (`<p>Seu
; carrinho está vazio</p>`; o fragmento `<>…</>` também é elemento): sem
; aspas, ele se guarda como está escrito. O nó do texto sozinho existe também
; na gramática do `.ts`; o do elemento, só na do TSX.
(jsx_element (jsx_text) @text.plain)

; A função entregue como valor a um atributo de elemento (`onClick={salvar}`,
; `onClick={this.salvar}`), sem ser chamada ali. O texto entre chaves no meio
; da tela é o que se mostra, e não entra.
(jsx_attribute (jsx_expression (identifier) @call.value))
(jsx_attribute (jsx_expression (member_expression property: (property_identifier) @call.value)))
