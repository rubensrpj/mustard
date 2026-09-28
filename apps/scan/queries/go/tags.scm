; Go — package (namespace), imports, top-level types and funcs.
(package_clause (package_identifier) @namespace)
(import_spec path: (interpreted_string_literal) @import)
; O apelido de `import s "strings"` é o nome que o import traz: o próprio
; pacote, escrito antes de outro nome (`s.Join()`).
(import_spec name: (package_identifier) @imported)

(type_spec name: (type_identifier) @name type: (struct_type)) @definition.struct
(type_spec name: (type_identifier) @name type: (interface_type)) @definition.interface
(type_alias name: (type_identifier) @name) @definition.type
(function_declaration name: (identifier) @name) @definition.function

; Constants — each name of a `const` spec, grouped or not; the value is not
; its header. A spec with no value repeats the one above it (`iota`).
; The names are the spec's own identifier children: the type is a type node
; and the value is inside its own list, and a `name:` field would give only
; the first name of `a, b = 1, 2`.
(const_spec (identifier) @name) @definition.const
(const_spec (identifier) @name value: (_) @value) @definition.const

; Membros — métodos com receptor e campos de struct. Os kinds de membro chegam
; ao mapa com as outras declarações do arquivo, e o grafo lista cada um sob o
; tipo dono dele. A tag de método segue o tags.scm do tree-sitter-go (MIT) —
; veja queries/README.md.
(method_declaration name: (field_identifier) @name) @definition.method
; O método com receptor é escrito fora do corpo do tipo: o dono dele é o tipo
; do receptor, com ou sem ponteiro e sem os argumentos de tipo.
(method_declaration
  receiver: (parameter_list (parameter_declaration type: [
    (type_identifier) @owner
    (pointer_type (type_identifier) @owner)
    (generic_type type: (type_identifier) @owner)
    (pointer_type (generic_type type: (type_identifier) @owner))]))
  name: (field_identifier) @name) @definition.method
(field_declaration name: (field_identifier) @name) @definition.field

; An interface method is a member like a receiver method; left uncaptured, its
; header was read as a call.
(method_elem name: (field_identifier) @name) @definition.method

; Os nomes que o corpo de uma função liga: da linha seguinte até o fim da
; declaração, o mesmo nome escrito sozinho é deles.
(short_var_declaration left: (expression_list (identifier) @local))
(var_spec name: (identifier) @local)
(parameter_declaration name: (identifier) @local)
(range_clause left: (expression_list (identifier) @local))

; Os textos fixos: o literal de texto escrito no código. O motor guarda o que
; tem duas palavras ou forma de caminho ou chave, com a marca (log, erro ou
; texto) e a declaração que o contém.
(interpreted_string_literal) @text
(raw_string_literal) @text
