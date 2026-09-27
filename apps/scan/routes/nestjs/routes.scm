; O prefixo da classe: o caminho do `@Controller('…')`, que vale para os
; métodos dentro dela. Na classe exportada, o decorador fica no `export`.
((export_statement
   decorator: (decorator
     (call_expression function: (identifier) @_decorator arguments: (arguments . (string) @route.prefix)))
   declaration: (class_declaration)) @route.scope
 (#eq? @_decorator "Controller"))

((class_declaration
   decorator: (decorator
     (call_expression function: (identifier) @_decorator arguments: (arguments . (string) @route.prefix)))) @route.scope
 (#eq? @_decorator "Controller"))

; O decorador do método, com o caminho. Quem atende a rota é o método que ele
; enfeita.
(class_body
  decorator: (decorator
    (call_expression function: (identifier) @route.method arguments: (arguments . (string) @route.path))))

; O decorador do método com uma lista de caminhos: cada um é uma rota.
(class_body
  decorator: (decorator
    (call_expression function: (identifier) @route.method arguments: (arguments . (array (string) @route.path)))))

; O decorador do método sem caminho (`@Get()`): a rota fica no prefixo da
; classe.
((class_body
   decorator: (decorator
     (call_expression function: (identifier) @route.method arguments: (arguments) @_arguments)))
 (#eq? @_arguments "()"))
