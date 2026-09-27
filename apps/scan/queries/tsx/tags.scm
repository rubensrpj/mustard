; TSX — o que só a gramática do TSX tem. Estes padrões entram só na consulta
; do `.tsx` (`extra_queries` no languages.toml), depois dos de typescript/;
; na consulta do `.ts`, cuja gramática não tem estes nós, seriam pulados.

; O texto escrito solto numa tela, entre as marcas de um elemento (`<p>Seu
; carrinho está vazio</p>`; o fragmento `<>…</>` também é elemento): sem
; aspas, ele se guarda como está escrito. O nó do texto sozinho existe também
; na gramática do `.ts`; o do elemento, só na do TSX.
(jsx_element (jsx_text) @text.plain)
