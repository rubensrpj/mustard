; Rust — use imports and item definitions.
; O import é o caminho escrito no `use`. No `use a::b as c;` o caminho é só
; `a::b`: o apelido é o nome que ele traz ao arquivo, e não parte do lugar
; que o caminho nomeia (`c::f()` chama o `f` do arquivo `a/b.rs`).
(use_declaration
  argument: [(crate) (identifier) (metavariable) (scoped_identifier) (scoped_use_list) (self) (super) (use_list) (use_wildcard)] @import)
(use_declaration argument: (use_as_clause path: (_) @import))

; The names `use m::limite` brings into the file: what it brought, not a use
; of it. The whole path is already the import's.
(use_declaration argument: (scoped_identifier name: (identifier) @imported))
(use_declaration argument: (identifier) @imported)
(use_list (identifier) @imported)
(use_list (scoped_identifier name: (identifier) @imported))
(use_as_clause alias: (identifier) @imported)

; O repasse: o `use` visível de fora (`pub use io::leitor::Leitor`) oferece a
; quem importa o arquivo os nomes que traz, tirados do arquivo que o caminho
; nomeia; o `pub use x::*` oferece todos. O nome trazido ou oferecido com
; outro nome (`use a::B as C`, `pub use a::B as C`, `use a::{B as C}`) é
; pedido ao arquivo alvo pelo nome de origem.
(use_declaration
  (visibility_modifier)
  argument: [(crate) (identifier) (metavariable) (scoped_identifier) (scoped_use_list) (self) (super) (use_list) (use_wildcard)] @reexport)
(use_declaration (visibility_modifier) argument: (use_as_clause path: (_) @reexport))
(use_as_clause
  path: [(scoped_identifier name: (identifier) @imported.original) (identifier) @imported.original]
  alias: (identifier) @imported)

; O caminho de uma chamada escrita pelo nome completo, sem `use`: `crate::a::b`
; em `crate::a::b::f()`, já sem o nome chamado. Preso à chamada, e nunca a um
; caminho solto, que casaria dentro do `use` e em cada pedaço do caminho. O
; motor só o guarda como import quando ele começa por um dos `root_aliases` ou
; pelo `parent_alias`, e lê o nome chamado no nó nomeado logo depois dele. O
; outro caminho de duas partes ou mais (`std::fs` em `std::fs::read()`) fica
; guardado com a chamada: quando a raiz dele não é peça do projeto, a chamada
; é da biblioteca e não liga ao projeto, mesmo com uma pasta `fs` nele.
(call_expression function: (scoped_identifier path: (_) @call.path))

; A função entregue como valor, sem ser chamada ali: o nome escrito como
; argumento (`xs.map(dobro)`, `.map(Self::metade)`, `.map(calc::dobro)`), como
; valor do `let` (`let f = dobro;`), à direita de uma atribuição ou no campo
; de uma struct. O motor liga o nome só a uma função ou a um método à vista.
; O campo lido (`self.total`) não entra: no Rust o método nunca se escreve
; depois do `.` sem ser chamado. O caminho escrito antes do nome se lê como o
; da chamada (`crate::a::triplo`).
(arguments (identifier) @call.value)
(arguments (scoped_identifier name: (identifier) @call.value))
(arguments (scoped_identifier path: (_) @call.path))
(let_declaration value: (identifier) @call.value)
(let_declaration value: (scoped_identifier name: (identifier) @call.value))
(let_declaration value: (scoped_identifier path: (_) @call.path))
(assignment_expression right: (identifier) @call.value)
(assignment_expression right: (scoped_identifier name: (identifier) @call.value))
(field_initializer value: (identifier) @call.value)
(field_initializer value: (scoped_identifier name: (identifier) @call.value))

; O campo escrito depois do objeto, sem chamada ali (`self.total`,
; `pedido.total`). O motor liga o nome só a um campo, como liga a chamada de
; método escrita depois do mesmo objeto: depois do próprio objeto, o campo do
; tipo; depois de um valor, nada.
(field_expression field: (field_identifier) @member)

; O módulo marcado como teste: o atributo `#[cfg(test)]` em qualquer ponto da
; fila de atributos colada ao `mod`, com outros no meio (`#[allow(dead_code)]`);
; um item que não é atributo entre a marca e o `mod` corta a fila. O que se
; importa, se chama e se cita dentro dele é do teste, e não uso do código do
; arquivo.
((attribute_item) @_marker
  .
  (attribute_item)*
  .
  (mod_item) @test_block
  (#eq? @_marker "#[cfg(test)]"))

; Todo módulo com corpo escrito dentro do arquivo, o de teste incluído: o
; `super` escrito dentro de N deles sai primeiro desses N módulos, e só depois
; sobe pasta. O `mod x;` sem corpo mora em outro arquivo e não entra.
(mod_item body: (declaration_list)) @inner_module

; O `mod x;` marcado com `#[path = "..."]` mora no arquivo que o atributo
; nomeia, lido a partir da pasta de quem o escreve: o caminho é import do
; arquivo, e o que ele declara fica à vista de quem escreve o `mod`. O
; atributo vale em qualquer ponto da fila de atributos colada ao `mod`, com
; outros no meio (`#[allow(dead_code)]`, `#[cfg(unix)]`); um item que não é
; atributo entre ele e o `mod` corta a fila.
((attribute_item (attribute (identifier) @_attr value: (string_literal (string_content) @import)))
  .
  (attribute_item)*
  .
  (mod_item !body)
  (#eq? @_attr "path"))

; O mesmo `mod` com `#[cfg(test)]` em qualquer ponto da mesma fila, antes ou
; depois do `path`, é do teste: o atributo do caminho é trecho de teste, e o
; arquivo que ele nomeia, import do teste, e não do arquivo. O import que o
; padrão de cima também acha cai dentro desse trecho e fica só do teste.
((attribute_item) @_marker
  .
  (attribute_item)*
  .
  (attribute_item (attribute (identifier) @_attr value: (string_literal (string_content) @import))) @test_block
  .
  (attribute_item)*
  .
  (mod_item !body)
  (#eq? @_marker "#[cfg(test)]")
  (#eq? @_attr "path"))
((attribute_item (attribute (identifier) @_attr value: (string_literal (string_content) @import))) @test_block
  .
  (attribute_item)*
  .
  (attribute_item) @_marker
  .
  (attribute_item)*
  .
  (mod_item !body)
  (#eq? @_marker "#[cfg(test)]")
  (#eq? @_attr "path"))

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
