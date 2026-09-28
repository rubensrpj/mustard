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

; O prefixo escrito no `path` do objeto de opções
; (`@Controller({ path: 'v1/pedidos', version: '2' })`). O objeto sem `path`
; não dá prefixo: a rota fica no caminho do método, como no `@Controller()`.
((export_statement
   decorator: (decorator
     (call_expression
       function: (identifier) @_decorator
       arguments: (arguments . (object (pair key: (property_identifier) @_key value: (string) @route.prefix)))))
   declaration: (class_declaration)) @route.scope
 (#eq? @_decorator "Controller")
 (#eq? @_key "path"))

((class_declaration
   decorator: (decorator
     (call_expression
       function: (identifier) @_decorator
       arguments: (arguments . (object (pair key: (property_identifier) @_key value: (string) @route.prefix)))))) @route.scope
 (#eq? @_decorator "Controller")
 (#eq? @_key "path"))

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

; O prefixo global: `app.setGlobalPrefix('api')`. Vale para as rotas do
; projeto do arquivo que o escreve.
((call_expression
   function: (member_expression property: (property_identifier) @_global)
   arguments: (arguments . (string) @route.prefix.global))
 (#eq? @_global "setGlobalPrefix"))

; Cada item de texto do `exclude`: `setGlobalPrefix('api', { exclude: ['saude'] })`.
((call_expression
   function: (member_expression property: (property_identifier) @_global)
   arguments: (arguments
     . (string) @route.prefix.global
     . (object (pair key: (property_identifier) @_exclude value: (array (string) @route.exclude)))))
 (#eq? @_global "setGlobalPrefix")
 (#eq? @_exclude "exclude"))

; O item de objeto do `exclude`: o `path` e o `method` dele, em matches
; separados, que o objeto junta
; (`{ path: 'saude', method: RequestMethod.GET }`).
((call_expression
   function: (member_expression property: (property_identifier) @_global)
   arguments: (arguments
     . (string) @route.prefix.global
     . (object
         (pair
           key: (property_identifier) @_exclude
           value: (array (object (pair key: (property_identifier) @_path value: (string) @route.exclude)) @route.exclude.item)))))
 (#eq? @_global "setGlobalPrefix")
 (#eq? @_exclude "exclude")
 (#eq? @_path "path"))

((call_expression
   function: (member_expression property: (property_identifier) @_global)
   arguments: (arguments
     . (string) @route.prefix.global
     . (object
         (pair
           key: (property_identifier) @_exclude
           value: (array
             (object
               (pair
                 key: (property_identifier) @_method
                 value: (member_expression property: (property_identifier) @route.exclude.method))) @route.exclude.item)))))
 (#eq? @_global "setGlobalPrefix")
 (#eq? @_exclude "exclude")
 (#eq? @_method "method"))

; A árvore de rotas dos módulos: cada item de
; `RouterModule.register([{ path: 'api', module: ApiModule, children: [...] }])`
; põe o `path` nos controladores do `module` dele e soma o dele aos
; `children`, de fora para dentro, em qualquer ordem das chaves.
((object
   (pair key: (property_identifier) @_path value: (string) @route.prefix)
   (pair key: (property_identifier) @_module value: (identifier) @route.target))
 (#eq? @_path "path")
 (#eq? @_module "module"))

((object
   (pair key: (property_identifier) @_module value: (identifier) @route.target)
   (pair key: (property_identifier) @_path value: (string) @route.prefix))
 (#eq? @_path "path")
 (#eq? @_module "module"))

((object
   (pair key: (property_identifier) @_path value: (string) @route.prefix)
   (pair key: (property_identifier) @_children value: (array) @route.scope))
 (#eq? @_path "path")
 (#eq? @_children "children"))

((object
   (pair key: (property_identifier) @_children value: (array) @route.scope)
   (pair key: (property_identifier) @_path value: (string) @route.prefix))
 (#eq? @_path "path")
 (#eq? @_children "children"))

; O módulo que lista os controladores: `@Module({ controllers: [PedidosController] })`
; na classe `PedidosModule`. O prefixo posto no módulo vale para as rotas de
; cada controlador da lista. Na classe exportada, o decorador fica no
; `export`.
((export_statement
   decorator: (decorator
     (call_expression
       function: (identifier) @_decorator
       arguments: (arguments
         . (object (pair key: (property_identifier) @_controllers value: (array (identifier) @route.target))))))
   declaration: (class_declaration name: (type_identifier) @route.receiver))
 (#eq? @_decorator "Module")
 (#eq? @_controllers "controllers"))

((class_declaration
   decorator: (decorator
     (call_expression
       function: (identifier) @_decorator
       arguments: (arguments
         . (object (pair key: (property_identifier) @_controllers value: (array (identifier) @route.target))))))
   name: (type_identifier) @route.receiver)
 (#eq? @_decorator "Module")
 (#eq? @_controllers "controllers"))
