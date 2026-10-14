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

; A declaração exportada, visível fora do arquivo: a que o `export` escreve
; (`export function`, `export class`, `export const`, `export default`) e o
; membro de uma classe exportada, que é público quando não diz o contrário.
; O membro `private` ou `protected` fica dentro dela, ainda que a classe saia.
(export_statement) @visible
(export_statement declaration: (_) @visible)
(export_statement
  declaration: (class_declaration
    body: (class_body [(method_definition) (public_field_definition)] @visible)))
(export_statement
  declaration: (abstract_class_declaration
    body: (class_body [(method_definition) (public_field_definition) (abstract_method_signature)] @visible)))
((method_definition (accessibility_modifier) @_access) @hidden
  (#not-eq? @_access "public"))
((public_field_definition (accessibility_modifier) @_access) @hidden
  (#not-eq? @_access "public"))
