; PHP — imports, namespace, and definitions. Generic capture vocabulary only:
;   @import            a `use` import (the imported qualified name is the path)
;   @namespace         the declared `namespace App\Http;` name
;   @name              the identifier of the enclosing @definition.*
;   @definition.<kind> a declaration; <kind> becomes Decl.kind verbatim
;   @decoration        an attribute or annotation, never code of the declaration
; The engine knows ONLY these capture names — never a node name or a language.

(namespace_use_clause [(qualified_name) (name)] @import)

(namespace_definition name: (namespace_name) @namespace)

(class_declaration name: (name) @name) @definition.class
(interface_declaration name: (name) @name) @definition.interface
(trait_declaration name: (name) @name) @definition.trait
(enum_declaration name: (name) @name) @definition.enum

(function_definition name: (name) @name) @definition.function

; Constants — each element of a `const` statement, in a class or at the top of
; the file; the value is not its header.
(const_declaration (const_element (name) @name (_) @value)) @definition.const

; The names `use function` and `use const` bring into the file: what it
; brought, not a use of it.
(namespace_use_clause type: _ (qualified_name (name) @imported))
(namespace_use_clause type: _ (name) @imported)
(namespace_use_declaration type: _
  body: (namespace_use_group (namespace_use_clause [(qualified_name (name) @imported) (name) @imported])))

; Membros — métodos, propriedades tipadas, casos de enum. Os kinds de membro
; chegam ao mapa com as outras declarações do arquivo, e o grafo lista cada um
; sob o tipo dono dele. A tag de método segue o tags.scm do tree-sitter-php
; (MIT) — veja queries/README.md.
(method_declaration name: (name) @name) @definition.method
(property_declaration (property_element name: (variable_name (name) @name))) @definition.property
(enum_case name: (name) @name) @definition.enum_member

; Os testes. No PHPUnit, o método `test*` da classe de teste (a de nome
; `*Test` ou a que herda de um `TestCase`), o método com `@test` no comentário
; de documentação colado a ele e o com o atributo `#[Test]`, com o caminho ou
; sem ele. A declaração sai marcada como teste. No Pest, a chamada
; `test('...', fn)` e `it('...', fn)` entra no mapa como declaração de teste,
; com o texto do primeiro argumento sem as aspas; o grupo (`describe`) não é
; teste.
((class_declaration name: (name) @_class body: (declaration_list (method_declaration name: (name) @_name) @test_unit))
  (#match? @_class "Test$")
  (#match? @_name "^test"))
((class_declaration (base_clause (_) @_base) body: (declaration_list (method_declaration name: (name) @_name) @test_unit))
  (#match? @_base "TestCase$")
  (#match? @_name "^test"))
((comment) @_doc . (method_declaration) @test_unit
  (#match? @_doc "(?s)^/\\*\\*.*@test\\b"))
((method_declaration attributes: (attribute_list (attribute_group (attribute [(name) (qualified_name)] @_attr)))) @test_unit
  (#match? @_attr "(^|\\\\)Test$"))
((function_call_expression
  function: (name) @_callee
  arguments: (arguments . (argument [(string) (encapsed_string)] @name.text) (argument [(anonymous_function) (arrow_function)]))) @definition.test @test_unit
  (#match? @_callee "^(test|it)$"))

; Decorations — an attribute list (`#[Route('/lista')]`) is not code of the
; declaration it adorns: the engine starts the header after it and reads no
; call out of it.
(attribute_list) @decoration

; Os nomes que o corpo de uma função liga: no alcance que a língua dá a cada
; um, marcado mais abaixo, o mesmo nome escrito sozinho é deles.
(assignment_expression left: (variable_name (name) @local))
(simple_parameter name: (variable_name (name) @local))
(anonymous_function_use_clause (variable_name (name) @local))
; Os outros jeitos de ligar variável: o valor e a chave do `foreach`, também
; por referência e desmontados; a exceção do `catch`; o `list(...)` e o
; `[...]` à esquerda da atribuição, em qualquer profundidade; o `global` e o
; `static`; o parâmetro variádico e o promovido a propriedade; a atribuição
; por referência e a composta (`$x ??= ...`).
(foreach_statement (_) . (variable_name (name) @local))
(foreach_statement (pair (variable_name (name) @local)))
(by_ref (variable_name (name) @local))
(list_literal (variable_name (name) @local))
(catch_clause name: (variable_name (name) @local))
(global_declaration (variable_name (name) @local))
(static_variable_declaration name: (variable_name (name) @local))
(variadic_parameter name: (variable_name (name) @local))
(property_promotion_parameter name: (variable_name (name) @local))
(reference_assignment_expression left: (variable_name (name) @local))
(augmented_assignment_expression left: (variable_name (name) @local))

; Até onde vale cada variável ligada: a função, o método, a closure e a função
; de seta fecham o alcance; o bloco não, porque a língua dá alcance de função.
; A variável se escreve com `$`, e a língua a separa da função e da classe de
; mesmo nome: a variável local alcança só a variável, e `config()` segue
; sendo a função mesmo depois de `$config`.
(function_definition) @local.scope
(method_declaration) @local.scope
(anonymous_function) @local.scope
(arrow_function) @local.scope
(variable_name (name) @variable)

; A propriedade escrita depois do objeto, sem chamada ali: a lida ou escrita
; (`$this->total`). O motor liga o nome só a uma propriedade, como liga a
; chamada de método escrita depois do mesmo objeto.
(member_access_expression name: (name) @member)

; Os textos fixos: o literal de texto escrito no código. O motor guarda o que
; tem duas palavras ou forma de caminho ou chave, com a marca (log, erro ou
; texto) e a declaração que o contém.
(string) @text
(encapsed_string) @text

; A declaração que o PHP deixa ver fora do arquivo: a classe, a interface, o
; traço, o enum, a função e a constante do topo, que são globais, e o membro
; de um tipo, público quando não diz o contrário. Quem está fora do projeto
; pode usá-la, e a falta de uso dentro dele não diz que ela sobra. O membro
; `private` ou `protected` (a palavra em qualquer caixa) fica dentro do tipo;
; o `private(set)` só fecha a escrita.
[(class_declaration)
 (interface_declaration)
 (trait_declaration)
 (enum_declaration)
 (function_definition)
 (const_declaration)
 (method_declaration)
 (property_declaration)
 (enum_case)] @visible
((_ (visibility_modifier) @_access) @hidden
  (#match? @_access "^(?i)(?:private|protected)$"))
