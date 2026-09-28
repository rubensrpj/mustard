; TypeScript / TSX — imports e declarações. A mesma família: o `.ts` e o `.tsx`
; leem estes padrões; o que só a gramática do TSX tem fica em tsx/.
(import_statement source: (string (string_fragment) @import))

; The names an import brings into the file (`limite` in
; `import { limite } from`): what it brought, not a use of it.
(import_specifier name: (_) @imported)
(import_specifier alias: (_) @imported)
(import_clause (identifier) @imported)
(namespace_import (identifier) @imported)

; O repasse: `export * from './x'` oferece a quem importa o arquivo tudo o que
; `./x` oferece, e `export { a } from './x'`, só `a`. Quem importa pede o nome
; novo de `export { a as b } from './x'`, e o repasse o tira de `./x` pelo de
; origem. O repasse é também import do arquivo.
(export_statement source: (string (string_fragment) @reexport))
(export_statement
  (export_clause (export_specifier name: (_) @imported !alias))
  source: (_))
(export_statement
  (export_clause (export_specifier name: (_) @reexport.original alias: (_) @imported))
  source: (_))

; `require('m')` importa `m` como o `import`, e o nome que recebe o que ele
; devolve é o que ele traz: `x` em `const x = require('m')`, `a` e `b` em
; `const { a, b: b } = require('m')`.
((call_expression
   function: (identifier) @_require
   arguments: (arguments . (string (string_fragment) @import)))
  (#eq? @_require "require"))
((variable_declarator
   name: (identifier) @imported
   value: (call_expression function: (identifier) @_require))
  (#eq? @_require "require"))
((variable_declarator
   name: (object_pattern (shorthand_property_identifier_pattern) @imported)
   value: (call_expression function: (identifier) @_require))
  (#eq? @_require "require"))
((variable_declarator
   name: (object_pattern (pair_pattern key: (_) @imported value: (identifier) @imported))
   value: (call_expression function: (identifier) @_require))
  (#eq? @_require "require"))

(class_declaration name: (_) @name) @definition.class
(abstract_class_declaration name: (_) @name) @definition.class
(interface_declaration name: (_) @name) @definition.interface
(enum_declaration name: (_) @name) @definition.enum
(type_alias_declaration name: (_) @name) @definition.type
(function_declaration name: (_) @name) @definition.function

; O JavaScript antigo exporta a função pondo-a no objeto `exports`, no topo
; do arquivo: a declaração leva o nome da propriedade
; (`exports.ler = function (req, res) {}`, `module.exports.ler = () => {}`)
; ou o da função (`module.exports = function ler() {}`).
; `module.exports = { ler, Carrinho }` não declara nada: os nomes já são
; declarados no arquivo.
(program
  (expression_statement
    (assignment_expression
      left: (member_expression
        object: (identifier) @_exports
        property: (property_identifier) @name)
      right: [(function_expression) (arrow_function)])) @definition.function
  (#eq? @_exports "exports"))
(program
  (expression_statement
    (assignment_expression
      left: (member_expression
        object: (member_expression
          object: (identifier) @_module
          property: (property_identifier) @_exports)
        property: (property_identifier) @name)
      right: [(function_expression) (arrow_function)])) @definition.function
  (#eq? @_module "module")
  (#eq? @_exports "exports"))
(program
  (expression_statement
    (assignment_expression
      left: (member_expression
        object: (identifier) @_module
        property: (property_identifier) @_exports)
      right: (function_expression name: (identifier) @name))) @definition.function
  (#eq? @_module "module")
  (#eq? @_exports "exports"))

; Exported top-level consts (e.g. `export const userTable = pgTable(...)`).
; This is the syntax hook a convention like Drizzle/GraphQL plugs into — the
; engine never knows the framework; it just sees a recurring `export const`.
; The declaration is the whole `export` statement, so the header reads
; `export const userTable`. The second pattern marks the value, where the
; header stops (`export const PRECOS = { ... }` reads `export const PRECOS`),
; except for an arrow function, whose parameters are its header.
(export_statement
  declaration: (lexical_declaration
    (variable_declarator name: (identifier) @name))) @definition.const
((export_statement
  declaration: (lexical_declaration
    (variable_declarator name: (identifier) @name value: (_) @value))) @definition.const
  (#not-match? @value "=>"))
; A `const` at the top of the file that is not exported is a constant too.
(program
  (lexical_declaration kind: "const"
    (variable_declarator name: (identifier) @name)) @definition.const)
((program
  (lexical_declaration kind: "const"
    (variable_declarator name: (identifier) @name value: (_) @value)) @definition.const)
  (#not-match? @value "=>"))

; Membros — métodos (de classe e de interface), campos de classe, propriedades
; de interface, membros de enum. Os kinds de membro chegam ao mapa com as
; outras declarações do arquivo, e o grafo lista cada um sob o tipo dono dele.
; Derivado do tags.scm do tree-sitter-typescript (MIT) — veja
; queries/README.md. O membro de enum simples é o próprio campo `name` do
; enum_body; o inicializado é um enum_assignment.
(method_definition name: (_) @name) @definition.method
(method_signature name: (_) @name) @definition.method
(abstract_method_signature name: (_) @name) @definition.method
(public_field_definition name: (_) @name) @definition.field
(property_signature name: (_) @name) @definition.property
(enum_body name: (property_identifier) @name @definition.enum_member)
(enum_assignment name: (_) @name) @definition.enum_member

; Decorations — a decorator (`@Component()`, `@Get()`) is not code of the
; declaration it adorns: the engine passes over it to find the doc comment
; above, starts the header after it, and reads no call out of it.
(decorator) @decoration

; Os nomes que o corpo de uma função liga: da linha seguinte até o fim da
; declaração, o mesmo nome escrito sozinho é deles.
(variable_declarator name: (identifier) @local)
(object_pattern (shorthand_property_identifier_pattern) @local)
(object_assignment_pattern left: (shorthand_property_identifier_pattern) @local)
(pair_pattern value: (identifier) @local)
(pair_pattern value: (assignment_pattern left: (identifier) @local))
(array_pattern (identifier) @local)
(required_parameter pattern: (identifier) @local)
(optional_parameter pattern: (identifier) @local)
(arrow_function parameter: (identifier) @local)
(rest_pattern (identifier) @local)
(for_in_statement left: (identifier) @local)
(catch_clause parameter: (identifier) @local)

; Os textos fixos: o literal de texto escrito no código. O motor guarda o que
; tem duas palavras ou forma de caminho ou chave, com a marca (log, erro ou
; texto) e a declaração que o contém.
(string) @text
(template_string) @text
