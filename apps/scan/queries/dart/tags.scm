; Dart — imports and definitions. Generic capture vocabulary only:
;   @import            an import directive's URI (the `package:`/relative path)
;   @name              the identifier of the enclosing @definition.*
;   @definition.<kind> a declaration; <kind> becomes Decl.kind verbatim
;   @decoration        an attribute or annotation, never code of the declaration
;   @body              the body of a declaration kept beside it, not inside it
; The engine knows ONLY these capture names — never a node name or a language.
;
; Verified against tree-sitter-dart-orchard 0.3 node-types.json:
;   library_import -> import_specification -> (uri (string_literal)); capturing
;     the (uri) keeps the path out of any `as`/`show`/`hide` combinator, and the
;     engine's clean_import strips the surrounding quotes.
;   class_definition / mixin_declaration / enum_declaration /
;     extension_declaration each expose a `name:` field of type (identifier).
;   a class body member is a (function_signature name: (identifier)) — the
;     closest the grammar has to a method declaration node.
(import_specification (configurable_uri (uri) @import))
(import_specification (uri) @import)
; O prefixo de `import 'x.dart' as c;` é o nome que o import traz: a própria
; biblioteca, escrita antes de outro nome (`c.jsonEncode()`).
(import_specification (identifier) @imported)

; O repasse: `export 'src/x.dart';`, com `show` ou `hide` ou sem, oferece a
; quem importa o arquivo o que `src/x.dart` oferece; com `show`, só os nomes
; escritos nele. O caminho escrito ali não é texto fixo do arquivo.
(library_export (configurable_uri (uri) @reexport))

(class_definition name: (identifier) @name) @definition.class
; `mixin_declaration` has NO `name:` field (node-types.json: fields = {}); the
; mixin name is a positional (identifier) child — a `name:` pattern would fail
; to compile and be dropped silently, so match it positionally.
(mixin_declaration (identifier) @name) @definition.mixin
(enum_declaration name: (identifier) @name) @definition.enum
(extension_declaration name: (identifier) @name) @definition.extension

; Constants — a `const` outside a class and a `static const` inside one. The
; grammar writes the top-level `const` and its names as siblings, so the
; pattern is anchored on the `const` right before them, with or without the
; type between; inside a class the whole declaration is the constant.
(program (const_builtin) . (static_final_declaration_list
  (static_final_declaration (identifier) @name (_) @value) @definition.const))
(program (const_builtin) . (_) . (static_final_declaration_list
  (static_final_declaration (identifier) @name (_) @value) @definition.const))
(declaration (const_builtin) (static_final_declaration_list
  (static_final_declaration (identifier) @name (_) @value))) @definition.const

; The names `show limite` brings into the file: what it brought, not a use of
; it.
(combinator "show" (identifier) @imported)

; Functions — a library-level function is a UNIT, a member is a MEMBER. Dart
; spells both with `function_signature`, so the line is drawn by CONTEXT: a
; library function is a DIRECT child of `program`, while a member is always
; wrapped — in `method_signature` (class and extension bodies) or in
; `declaration` (a mixin's abstract member). A node has one parent, so the three
; patterns are mutually exclusive and the recorded kind never depends on match
; order. Before this, every library function was recorded as a member, and the
; map never listed it as a function.
(program (function_signature name: (identifier) @name) @definition.function)
(method_signature (function_signature name: (identifier) @name) @definition.method)
(declaration (function_signature name: (identifier) @name) @definition.method)

; Constructors — a member like a method; left uncaptured, the header was read
; as a call and the class appeared to use itself. The name is the identifier
; right before the parameter list, so a named constructor (`Caixa.vazia()`)
; records its own name.
(constructor_signature (identifier) @name . (formal_parameter_list)) @definition.method
(constant_constructor_signature (identifier) @name . (formal_parameter_list)) @definition.method
(factory_constructor_signature (identifier) @name . (formal_parameter_list)) @definition.method

; Bodies — Dart keeps a function's body as the NEXT SIBLING of its signature,
; not inside it, so the signature alone ends on its own line. @body marks the
; body that belongs to the declaration, and the declaration ends where it ends:
; a call inside the body is then used by the function that encloses it.
(program (function_signature name: (identifier) @name) @definition.function . (function_body) @body)
;
; A member keeps its body beside the `method_signature` that wraps it, and this
; holds in a class, a mixin, an extension and an enum alike, so the pattern is
; anchored on the wrapper and not on the body that holds it. Each form of member
; has its own line: the method, the constructor (plain and named), the factory,
; the getter and the setter. The setter is captured only here: its name is a
; declaration, and its header is no longer read as a call.
((method_signature (function_signature name: (identifier) @name) @definition.method) . (function_body) @body)
((method_signature (constructor_signature (identifier) @name . (formal_parameter_list)) @definition.method) . (function_body) @body)
((method_signature (factory_constructor_signature (identifier) @name . (formal_parameter_list)) @definition.method) . (function_body) @body)
((method_signature (getter_signature name: (identifier) @name) @definition.property) . (function_body) @body)
((method_signature (setter_signature name: (identifier) @name) @definition.property) . (function_body) @body)

; Campos, propriedades e itens de enumeração — membros do tipo que os
; contém, como os métodos. Cada nome declarado no corpo de uma classe, de um
; mixin, de uma extensão ou de uma enumeração é um campo (`final int limite =
; 10;`, `late String nome;`, os dois de `int a, b;`, o `static final`); o
; `static const` segue constante, no padrão de cima. O `get` e o `set` sem
; corpo (o abstrato) são propriedade, como os com corpo, logo acima. Cada item
; de uma enumeração é um membro dela. O tipo que os contém vai escrito como
; dono (`@owner`), porque o tipo escrito numa linha só (`enum Estado { aberto,
; fechado }`) tem a mesma faixa que eles.
(class_definition name: (identifier) @owner body: (class_body (declaration
  (initialized_identifier_list (initialized_identifier . (identifier) @name))) @definition.field))
(class_definition name: (identifier) @owner body: (class_body (declaration (final_builtin)
  (static_final_declaration_list (static_final_declaration . (identifier) @name))) @definition.field))
(mixin_declaration (identifier) @owner (class_body (declaration
  (initialized_identifier_list (initialized_identifier . (identifier) @name))) @definition.field))
(mixin_declaration (identifier) @owner (class_body (declaration (final_builtin)
  (static_final_declaration_list (static_final_declaration . (identifier) @name))) @definition.field))
(extension_declaration name: (identifier) @owner body: (extension_body (declaration
  (initialized_identifier_list (initialized_identifier . (identifier) @name))) @definition.field))
(extension_declaration name: (identifier) @owner body: (extension_body (declaration (final_builtin)
  (static_final_declaration_list (static_final_declaration . (identifier) @name))) @definition.field))
(enum_declaration name: (identifier) @owner body: (enum_body (declaration
  (initialized_identifier_list (initialized_identifier . (identifier) @name))) @definition.field))
(enum_declaration name: (identifier) @owner body: (enum_body (declaration (final_builtin)
  (static_final_declaration_list (static_final_declaration . (identifier) @name))) @definition.field))
(enum_declaration name: (identifier) @owner body: (enum_body
  (enum_constant name: (identifier) @name) @definition.enum_member))
(declaration (getter_signature name: (identifier) @name) @definition.property)
(declaration (setter_signature name: (identifier) @name) @definition.property)

; Library — a `part` file shares one library with its owner and imports
; nothing, so each side is an import of the other: `part 'x.dart';` in the
; owner, `part of 'owner.dart';` (by file) or `part of loja.caixa;` (by the
; library name, answered by the owner's `library loja.caixa;` namespace).
(part_directive (uri) @import)
(part_of_directive (uri) @import)
(part_of_directive (dotted_identifier_list) @import)
(library_name (dotted_identifier_list) @namespace)

; Decorations — an annotation (`@override`, `@immutable`) is not code of the
; declaration it adorns: the engine passes over it to find the doc comment
; above, starts the header after it, and reads no call out of it.
(annotation) @decoration

; Os nomes que o corpo de uma função liga: no alcance que a língua dá a cada
; um, marcado mais abaixo, o mesmo nome escrito sozinho é deles.
(initialized_variable_definition name: (identifier) @local)
; O parâmetro, com tipo ou sem ele (`(x) { }`), e o parâmetro com forma de
; função (`int x()`).
(formal_parameter (identifier) @local)
; Os outros jeitos de ligar nome: os outros nomes da mesma declaração
; (`var a = 1, x = 2;`), o nome do `for (var x in ...)`, os do `catch (e, s)`
; e os do padrão do Dart 3. A variável do padrão (`var x`, `int x`) liga em
; todo padrão; o nome sozinho no padrão só liga numa declaração (`var (a, x)
; = ...`, `for (var (a, x) in ...)`), até dois padrões de profundidade: no
; `case`, ele é a constante que o padrão compara.
(initialized_variable_definition (initialized_identifier . (identifier) @local))
(for_loop_parts name: (identifier) @local)
(catch_parameters (identifier) @local)
(variable_pattern (identifier) @local)
(pattern_variable_declaration
  [(record_pattern (constant_pattern (identifier) @local))
   (list_pattern (constant_pattern (identifier) @local))
   (map_pattern (constant_pattern (identifier) @local))
   (object_pattern (constant_pattern (identifier) @local))])
(for_loop_parts
  [(record_pattern (constant_pattern (identifier) @local))
   (list_pattern (constant_pattern (identifier) @local))
   (map_pattern (constant_pattern (identifier) @local))
   (object_pattern (constant_pattern (identifier) @local))])
(pattern_variable_declaration
  [(record_pattern [(record_pattern (constant_pattern (identifier) @local)) (list_pattern (constant_pattern (identifier) @local)) (rest_pattern (constant_pattern (identifier) @local))])
   (list_pattern [(record_pattern (constant_pattern (identifier) @local)) (list_pattern (constant_pattern (identifier) @local)) (rest_pattern (constant_pattern (identifier) @local))])])
(for_loop_parts
  [(record_pattern [(record_pattern (constant_pattern (identifier) @local)) (list_pattern (constant_pattern (identifier) @local))])
   (list_pattern [(record_pattern (constant_pattern (identifier) @local)) (list_pattern (constant_pattern (identifier) @local))])])

; Até onde vale cada nome ligado: o bloco, o `for` (também o da coleção), o
; `if` com padrão, cada caso do `switch` e da expressão `switch`, a função
; anônima e a função local fecham o alcance. O corpo que a gramática põe ao
; lado da assinatura fecha o do parâmetro, e o bloco ao lado do `catch` o dos
; nomes dele. O iterável que o `for` lê e o `else` do `if` com padrão ficam
; fora do alcance dos nomes que eles ligam.
(block) @local.scope
(for_statement) @local.scope
(for_element) @local.scope
(if_statement) @local.scope
(switch_statement_case) @local.scope
(switch_expression_case) @local.scope
(function_expression) @local.scope
(lambda_expression) @local.scope
((function_signature
  (formal_parameter_list
    [(formal_parameter (identifier) @local)
     (optional_formal_parameters (formal_parameter (identifier) @local))])) . (function_body) @local.scope)
((method_signature
  (_ (formal_parameter_list
    [(formal_parameter (identifier) @local)
     (optional_formal_parameters (formal_parameter (identifier) @local))]))) . (function_body) @local.scope)
((catch_clause (catch_parameters (identifier) @local)) . (block) @local.scope)
(for_loop_parts value: (_) @local.outside)
(if_statement alternative: (_) @local.outside)

; A função entregue como valor, sem ser chamada ali: o nome escrito como
; argumento (`xs.map(dobro)`, `onPressed: salvar`), como valor de uma
; variável (`final f = dobro;`) ou à direita de uma atribuição. O motor liga o
; nome só a uma função ou a um método à vista.
(argument (identifier) @call.value)
(named_argument (identifier) @call.value)
(initialized_variable_definition value: (identifier) @call.value)
(assignment_expression right: (identifier) @call.value)

; O membro escrito depois do objeto, sem chamada ali: o campo ou a
; propriedade lida ou escrita (`pedido.total`, `this.total`), também pelo
; acesso opcional (`pedido?.total`). O motor liga o nome só a um campo ou a
; uma propriedade, como liga a chamada de método escrita depois do mesmo
; objeto.
(unconditional_assignable_selector (assignable_operator) . (identifier) @member)
(conditional_assignable_selector (assignable_operator) . (identifier) @member)

; Os textos fixos: o literal de texto escrito no código. O motor guarda o que
; tem duas palavras ou forma de caminho ou chave, com a marca (log, erro ou
; texto) e a declaração que o contém.
(string_literal) @text

; A declaração que o Dart deixa ver fora da biblioteca: a de nome sem
; sublinhado na frente, que é a regra da língua, nome a nome, também para o
; membro de um tipo. Quem está fora do projeto pode usá-la, e a falta de uso
; dentro dele não diz que ela sobra. O nome do construtor é o que vem antes
; dos parâmetros (`vazia` em `Caixa.vazia()`, `_` em `Caixa._()`).
([(class_definition name: (identifier) @_name)
  (mixin_declaration (identifier) @_name)
  (enum_declaration name: (identifier) @_name)
  (extension_declaration name: (identifier) @_name)
  (function_signature name: (identifier) @_name)
  (getter_signature name: (identifier) @_name)
  (setter_signature name: (identifier) @_name)
  (constructor_signature (identifier) @_name . (formal_parameter_list))
  (constant_constructor_signature (identifier) @_name . (formal_parameter_list))
  (factory_constructor_signature (identifier) @_name . (formal_parameter_list))
  (static_final_declaration . (identifier) @_name)
  (declaration (initialized_identifier_list (initialized_identifier . (identifier) @_name)))
  (declaration (static_final_declaration_list (static_final_declaration . (identifier) @_name)))
  (enum_constant name: (identifier) @_name)] @visible
  (#match? @_name "^[^_]"))
