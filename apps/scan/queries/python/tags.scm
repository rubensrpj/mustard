; Python — imports and definitions. A module is a file, so no @namespace.
(import_statement name: (dotted_name) @import)
; Sem apelido, `import util` traz ao arquivo só o primeiro nome escrito, o do
; módulo (`loja` em `import loja.servico`): `util.ler()` alcança o módulo, e
; `ler()` sozinho não o vê.
(import_statement name: (dotted_name . (identifier) @imported))
; O apelido de `import loja.servico as s` é o nome que o import traz: o
; próprio módulo, escrito antes de outro nome (`s.buscar()`).
(import_statement name: (aliased_import name: (dotted_name) @import alias: (identifier) @imported))

; O repasse: a língua não tem `export`, e o nome que um módulo importa é
; importável dele. Por isso todo `from x import a, b`, com ou sem ponto na
; frente, oferece a quem importa o arquivo os nomes que traz, e o
; `from x import *`, tudo o que `x` oferece. O repasse é também import do
; arquivo.
(import_from_statement module_name: (dotted_name) @reexport)
(import_from_statement module_name: (relative_import (dotted_name)) @reexport)

; `from . import models`: o import é só os pontos, uma pasta, e cada nome que
; ele traz é um arquivo dela. Capturados juntos, o motor lê o import como
; `.models` (veja `relative_import` no languages.toml).
(import_from_statement
  module_name: (relative_import . (import_prefix) .) @import
  name: (dotted_name) @imported)
(import_from_statement
  module_name: (relative_import . (import_prefix) .) @import
  name: (aliased_import name: (dotted_name) @imported))
; `from . import *` não traz arquivo pelo nome: o import fica a pasta.
(import_from_statement
  module_name: (relative_import . (import_prefix) .) @import
  (wildcard_import))

(class_definition name: (identifier) @name) @definition.class

; Constants — a name given a value at the top of the module, a direct child of
; `module`. Inside a function it is a local, and inside a class a field.
(module
  (expression_statement
    (assignment left: (identifier) @name right: (_) @value) @definition.const))

; The names `from m import limite` brings into the file: what it brought, not
; a use of it.
(import_from_statement name: (dotted_name) @imported)
; `from m import a as b` traz `b`, que o import e o repasse tiram de `m`
; pelo nome `a`.
(import_from_statement
  name: (aliased_import name: (dotted_name) @imported.original alias: (identifier) @imported))

; Functions — a module-level function is a UNIT, a method is a MEMBER. Python
; spells both with `function_definition`, so the line is drawn by CONTEXT: a
; module-level function is a DIRECT child of `module`, a method is a direct
; child of a class body's `block`. A node has one parent, so the two patterns
; are mutually exclusive and the recorded kind never depends on match order —
; the hazard that once justified recording every def as @definition.function.
; Named local functions also need their own searchable ranges. The general
; fallback below follows these context-specific patterns, so methods keep
; their kind regardless of query match order.
(module (function_definition name: (identifier) @name) @definition.function)
(class_definition
  body: (block (function_definition name: (identifier) @name) @definition.method))

; Membros — atributos no corpo da classe (`name = ""` / `name: str = ""`), o
; mais perto que o Python tem de uma declaração de campo. Os kinds de membro
; chegam ao mapa com as outras declarações do arquivo, e o grafo lista cada um
; sob o tipo dono dele.
(class_definition
  body: (block
    (expression_statement
      (assignment left: (identifier) @name) @definition.field)))

; Decorated functions — a decorator wraps the def in a `decorated_definition`,
; so the two patterns above never see it. The same line is drawn by the
; wrapper's parent: the module for a function, a class body for a method.
(module
  (decorated_definition
    definition: (function_definition name: (identifier) @name) @definition.function))
(class_definition
  body: (block
    (decorated_definition
      definition: (function_definition name: (identifier) @name) @definition.method)))

; Docstrings — the first string of a body documents the def or the class. One
; pattern per form above, each with the same kind, so the docstring joins the
; declaration the pattern above already gives. `string_content` is the text
; without its quotes.
(class_definition
  name: (identifier) @name
  body: (block . (expression_statement (string (string_content) @doc)))) @definition.class
(module
  (function_definition
    name: (identifier) @name
    body: (block . (expression_statement (string (string_content) @doc)))) @definition.function)
(class_definition
  body: (block
    (function_definition
      name: (identifier) @name
      body: (block . (expression_statement (string (string_content) @doc)))) @definition.method))
(module
  (decorated_definition
    definition: (function_definition
      name: (identifier) @name
      body: (block . (expression_statement (string (string_content) @doc)))) @definition.function))
(class_definition
  body: (block
    (decorated_definition
      definition: (function_definition
        name: (identifier) @name
        body: (block . (expression_statement (string (string_content) @doc)))) @definition.method)))

; Decorations — a decorator (`@app.get("/")`, `@dataclass`) is not code of the
; declaration it adorns: the engine passes over it to find the comment above
; and reads no call out of it.
(decorator) @decoration

; General fallback includes named functions inside functions and control-flow
; blocks. Earlier method patterns retain precedence; declarations are deduped
; by node/name. Lexical containment is recorded by the shared extractor.
(function_definition name: (identifier) @name) @definition.function
(function_definition
  name: (identifier) @name
  body: (block . (expression_statement (string (string_content) @doc)))) @definition.function

; Os nomes que o corpo de uma função liga: no alcance que a língua dá a cada
; um, marcado mais abaixo, o mesmo nome escrito sozinho é deles.
(assignment left: (identifier) @local)
(parameters (identifier) @local)
(default_parameter name: (identifier) @local)
(typed_parameter (identifier) @local)
(typed_default_parameter name: (identifier) @local)
(lambda_parameters (identifier) @local)
(for_statement left: (identifier) @local)

; Os outros jeitos de ligar nome: o alvo desmontado, em qualquer profundidade
; (`a, (b, c) = ...`, `[a, b] = ...`, `for i, x in ...`), também com `*` e,
; no parâmetro, com `**`; o alvo da compreensão; o `:=`; o `as` do `with` e do
; `except`, também com tupla; e o `case`, onde o nome sozinho, sem ponto, é
; sempre captura (`case x:`, `case [a, x]:`, `case P(campo=x):`, `case {"k":
; x}:`), como o nome depois do `as` e o do `*` e do `**`.
(pattern_list (identifier) @local)
(tuple_pattern (identifier) @local)
(list_pattern (identifier) @local)
(list_splat_pattern (identifier) @local)
(dictionary_splat_pattern (identifier) @local)
(for_in_clause left: (identifier) @local)
(named_expression name: (identifier) @local)
(with_item value: (as_pattern alias: (as_pattern_target (identifier) @local)))
(with_item value: (as_pattern alias: (as_pattern_target [(tuple (identifier) @local) (list (identifier) @local)])))
(except_clause value: (as_pattern alias: (as_pattern_target (identifier) @local))) @local.scope
(except_clause value: (as_pattern alias: (as_pattern_target [(tuple (identifier) @local) (list (identifier) @local)]))) @local.scope
(case_pattern (dotted_name . (identifier) @local .))
(keyword_pattern (dotted_name . (identifier) @local .))
(case_pattern (as_pattern (identifier) @local))
(splat_pattern (identifier) @local)

; Até onde vale cada nome ligado: a função, a lambda e a classe fecham o
; alcance; o bloco não, porque a língua dá alcance de função. A compreensão é
; o alcance inteiro dos nomes que ela liga, também o valor escrito antes do
; `for`; o iterável que cada `for` dela lê e o valor padrão do parâmetro ficam
; fora. O nome do `except ... as` vale só até o fim do `except` (acima).
(function_definition) @local.scope
(lambda) @local.scope
(class_definition) @local.scope
(list_comprehension) @local.scope.whole
(set_comprehension) @local.scope.whole
(dictionary_comprehension) @local.scope.whole
(generator_expression) @local.scope.whole
(for_in_clause right: (_) @local.outside)
(default_parameter value: (_) @local.outside)
(typed_default_parameter value: (_) @local.outside)

; A função entregue como valor, sem ser chamada ali: o nome escrito como
; argumento (`map(dobro, xs)`, `sorted(xs, key=chave)`,
; `Thread(target=self.rodar)`) ou à direita de uma atribuição (`f = dobro`). O
; motor liga o nome só a uma função ou a um método à vista.
(argument_list (identifier) @call.value)
(argument_list (attribute attribute: (identifier) @call.value))
(keyword_argument value: (identifier) @call.value)
(keyword_argument value: (attribute attribute: (identifier) @call.value))
(assignment right: (identifier) @call.value)
(assignment right: (attribute attribute: (identifier) @call.value))

; O atributo escrito depois do objeto, sem chamada ali: o campo lido ou
; escrito (`pedido.total`, `self.total`). O motor liga o nome só a um campo,
; como liga a chamada de método escrita depois do mesmo objeto.
(attribute attribute: (identifier) @member)

; Os textos fixos: o literal de texto escrito no código. O motor guarda o que
; tem duas palavras ou forma de caminho ou chave, com a marca (log, erro ou
; texto) e a declaração que o contém.
(string) @text

; A string escrita sozinha no começo do módulo é a documentação dele, e não
; texto fixo.
(module . (expression_statement (string (string_content) @doc)))

; A declaração que o Python deixa ver fora do módulo: a de nome sem
; sublinhado na frente, e o nome especial entre sublinhados duplos
; (`__init__`, `__all__`), que a própria língua chama. Quem está fora do
; projeto pode usá-la, e a falta de uso dentro dele não diz que ela sobra. A
; classe, a função e a constante valem no topo do módulo, porque as escritas
; dentro de uma função são locais dela; o membro vale no corpo de uma classe.
((module [
    (class_definition name: (identifier) @_name) @visible
    (function_definition name: (identifier) @_name) @visible
    (decorated_definition definition: [
      (class_definition name: (identifier) @_name)
      (function_definition name: (identifier) @_name)] @visible)
    (expression_statement (assignment left: (identifier) @_name) @visible)])
  (#match? @_name "^(?:[^_]|__.+__$)"))
((class_definition body: (block [
    (class_definition name: (identifier) @_name) @visible
    (function_definition name: (identifier) @_name) @visible
    (decorated_definition definition: [
      (class_definition name: (identifier) @_name)
      (function_definition name: (identifier) @_name)] @visible)
    (expression_statement (assignment left: (identifier) @_name) @visible)]))
  (#match? @_name "^(?:[^_]|__.+__$)"))

; O nome que o módulo deixa ver fora pela lista `__all__` escrita no topo dele
; (`__all__ = ['_ler']`, `__all__ += ('_gravar',)`, em lista ou em tupla): a
; declaração do topo do módulo com esse nome é de fora, ainda que comece por
; sublinhado.
((module (expression_statement [
    (assignment left: (identifier) @_all
      right: [(list (string (string_content) @visible.name)) (tuple (string (string_content) @visible.name))])
    (augmented_assignment left: (identifier) @_all
      right: [(list (string (string_content) @visible.name)) (tuple (string (string_content) @visible.name))])]))
  (#eq? @_all "__all__"))
