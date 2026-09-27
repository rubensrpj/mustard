; Rust — use imports and item definitions.
(use_declaration argument: (_) @import)

; The names `use m::limite` brings into the file: what it brought, not a use
; of it. The whole path is already the import's.
(use_declaration argument: (scoped_identifier name: (identifier) @imported))
(use_declaration argument: (identifier) @imported)
(use_list (identifier) @imported)
(use_list (scoped_identifier name: (identifier) @imported))
(use_as_clause alias: (identifier) @imported)

; O caminho de uma chamada escrita pelo nome completo, sem `use`: `crate::a::b`
; em `crate::a::b::f()`, já sem o nome chamado. Preso à chamada, e nunca a um
; caminho solto, que casaria dentro do `use` e em cada pedaço do caminho. O
; motor só o guarda como import quando ele começa por um dos `root_aliases` ou
; pelo `parent_alias`, e lê o nome chamado no nó nomeado logo depois dele.
(call_expression function: (scoped_identifier path: (_) @call.path))

; O módulo marcado como teste: o atributo `#[cfg(test)]` logo antes do `mod`.
; O que se importa, se chama e se cita dentro dele é do teste, e não uso do
; código do arquivo.
((attribute_item) @_marker . (mod_item) @test_block
  (#eq? @_marker "#[cfg(test)]"))

; Todo módulo com corpo escrito dentro do arquivo, o de teste incluído: o
; `super` escrito dentro de N deles sai primeiro desses N módulos, e só depois
; sobe pasta. O `mod x;` sem corpo mora em outro arquivo e não entra.
(mod_item body: (declaration_list)) @inner_module

(struct_item name: (type_identifier) @name) @definition.struct
(enum_item name: (type_identifier) @name) @definition.enum
(trait_item name: (type_identifier) @name) @definition.trait
(type_item name: (type_identifier) @name) @definition.type

; Constantes — `const` e `static`, as duas formas do Rust de um valor com nome
; fixado na compilação. Um kind só para as duas: nenhuma é chamada nem é tipo,
; e o grafo liga a constante só onde ela é citada, nunca como chamada.
(const_item name: (identifier) @name) @definition.constant
; The value of a `const` is not its header: `pub const LIMITE: u32 = 10;`
; reads `pub const LIMITE: u32`.
(const_item name: (identifier) @name value: (_) @value) @definition.constant
(static_item name: (identifier) @name) @definition.constant

; Functions — a free function is a UNIT, a method is a MEMBER. Rust spells both
; with the same `function_item` node, so the line is drawn by CONTEXT: each
; pattern is anchored on its PARENT, which makes them mutually exclusive (no two
; ever match the same node) and therefore independent of match order — the very
; hazard that once justified recording every fn as @definition.function. A trait
; method without a body is a `function_signature_item`, which that single
; pattern never captured at all.
(source_file (function_item name: (identifier) @name) @definition.function)
(mod_item body: (declaration_list (function_item name: (identifier) @name) @definition.function))
(impl_item body: (declaration_list (function_item name: (identifier) @name) @definition.method))

; O bloco `impl` não é declaração: o método e a constante escritos nele têm
; como dono o tipo da linha do `impl`, e o de `impl Traço for Tipo` cumpre o
; traço. Só a linha do `impl` é lida; o corpo dele não.
(impl_item type: (_) @owner body: (declaration_list (function_item name: (identifier) @name) @definition.method))
(impl_item trait: (_) @owner.contract body: (declaration_list (function_item name: (identifier) @name) @definition.method))
(impl_item type: (_) @owner body: (declaration_list (const_item name: (identifier) @name) @definition.constant))
(impl_item trait: (_) @owner.contract body: (declaration_list (const_item name: (identifier) @name) @definition.constant))
(trait_item body: (declaration_list (function_item name: (identifier) @name) @definition.method))
(trait_item body: (declaration_list (function_signature_item name: (identifier) @name) @definition.method))

; Membros — campos de struct e variantes de enum. Os kinds de membro chegam ao
; mapa com as outras declarações do arquivo, e o grafo lista cada um sob o
; tipo dono dele.
(field_declaration name: (field_identifier) @name) @definition.field
(enum_variant name: (identifier) @name) @definition.enum_member

; Decorations — an attribute is not code of the declaration it adorns: the
; engine passes over it to find the doc comment above, starts the header after
; it, and reads no call out of it (`#[derive(Debug)]` calls nothing).
(attribute_item) @decoration
(inner_attribute_item) @decoration

; O `self` de uma lista de import traz o último nome escrito antes dela (`fs`
; em `use std::fs::{self}`).
(use_list (self) @imported)

; Os nomes que o corpo de uma função liga: da linha seguinte até o fim da
; declaração, o mesmo nome escrito sozinho é deles.
(let_declaration pattern: (identifier) @local)
(let_declaration pattern: (tuple_pattern (identifier) @local))
(parameter pattern: (identifier) @local)
(closure_parameters (identifier) @local)
(closure_parameters (parameter pattern: (identifier) @local))
(for_expression pattern: (identifier) @local)

; Os textos fixos: o literal de texto escrito no código. O motor guarda o que
; tem duas palavras ou forma de caminho ou chave, com a marca (log, erro ou
; texto) e a declaração que o contém.
(string_literal) @text
(raw_string_literal) @text
