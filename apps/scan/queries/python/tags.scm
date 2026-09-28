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
; `from m import a as b` traz `b`, que o repasse tira de `m` pelo nome `a`.
(import_from_statement
  name: (aliased_import name: (dotted_name) @reexport.original alias: (identifier) @imported))

; Functions — a module-level function is a UNIT, a method is a MEMBER. Python
; spells both with `function_definition`, so the line is drawn by CONTEXT: a
; module-level function is a DIRECT child of `module`, a method is a direct
; child of a class body's `block`. A node has one parent, so the two patterns
; are mutually exclusive and the recorded kind never depends on match order —
; the hazard that once justified recording every def as @definition.function.
; A function nested inside another function is neither: a local closure is not
; an architectural unit.
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

; Os nomes que o corpo de uma função liga: da linha seguinte até o fim da
; declaração, o mesmo nome escrito sozinho é deles.
(assignment left: (identifier) @local)
(assignment left: (pattern_list (identifier) @local))
(parameters (identifier) @local)
(default_parameter name: (identifier) @local)
(typed_parameter (identifier) @local)
(typed_default_parameter name: (identifier) @local)
(lambda_parameters (identifier) @local)
(for_statement left: (identifier) @local)

; Os textos fixos: o literal de texto escrito no código. O motor guarda o que
; tem duas palavras ou forma de caminho ou chave, com a marca (log, erro ou
; texto) e a declaração que o contém.
(string) @text

; A string escrita sozinha no começo do módulo é a documentação dele, e não
; texto fixo.
(module . (expression_statement (string (string_content) @doc)))
