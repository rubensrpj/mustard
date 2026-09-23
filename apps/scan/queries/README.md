# queries/ — consultas tree-sitter (DADO, nunca lógica)

Cada subdiretório é um *query set*: os arquivos `.scm` que definem o que o
motor genérico (`extract.rs`) extrai daquela gramática. O motor só entende o
vocabulário genérico de captura da lista abaixo, que é o único lugar onde ela
está escrita. Nenhum nome de nó de gramática existe em `src/`; adicionar linguagem é dado:
uma linha em `languages.toml` + `.scm` aqui + fixture `graph_<dir>` + entrada
no `kinds-manifest.toml` — o teste de paridade (`tests/kinds_parity.rs`) acusa
lacuna sozinho.

## Capturas

| Captura | O que o motor faz com ela |
|---------|---------------------------|
| `@import` | Um import ou `using`; o texto é limpo até virar o caminho, que a resolução do grafo liga aos arquivos do projeto. O que fica dentro da captura nunca vira chamada nem citação: é o caminho do import, não um uso. |
| `@import.global` | Um import que vale para todo arquivo da mesma linguagem sob a pasta do manifesto mais próximo acima de quem o escreve (sem manifesto, sob a pasta do próprio arquivo). Vai para `Module.global_imports`; o mesmo nó continua sendo `@import` do arquivo que o escreve, e a aresta do grafo de import fica só nele. |
| `@namespace` | O nome do namespace ou do pacote que o arquivo declara. Como o namespace se enxerga entre os arquivos é o campo `namespace_scope` do `languages.toml`. Como no `@import`, o nome escrito ali não é uso. |
| `@definition.<kind>` | Uma declaração; o sufixo `<kind>` vira `Decl.kind` literalmente. |
| `@name` | O nome da `@definition.*` do mesmo pattern. |
| `@supertype` | Um tipo-base, interface ou trait; o motor o liga, pelo nome, à declaração de mesmo `@name`, mesmo quando capturado num nó separado dela. |
| `@decoration` | Um atributo ou anotação: o cabeçalho da declaração começa depois dele, o comentário acima passa por cima dele, e nada dentro dele vira chamada nem citação. |
| `@body` | O corpo que a gramática põe ao lado da declaração, e não dentro dela: a declaração termina onde o corpo termina. |
| `@value` | O valor dado à declaração: o cabeçalho para onde ele começa, e o `=` que sobra no fim sai (`export const PRECOS = { ... }` fica `export const PRECOS`). |
| `@doc` | A documentação que a linguagem escreve dentro da declaração, e não em cima dela (a docstring do Python); o texto da captura já vem sem as aspas. O motor a junta à declaração do mesmo pattern, e o comentário de cima, quando existe, vale mais. |

Qualquer outro nome de captura é ignorado.

## kinds-manifest.toml

Inventário declarado de `@definition.<kind>` por query set. O teste de
paridade escaneia a fixture `tests/fixtures/graph_<dir>/` de cada entrada e
verifica nas duas direções: todo kind declarado produz ≥ 1 declaração, e todo
kind produzido está declarado. É essa a rede que pega um pattern que parou de
compilar contra a versão da gramática (o motor descarta pattern ruim em
silêncio, por design).

Kinds de membro (`method`, `property`, `field`, `enum_member`) alimentam só o
índice de termos do digest; a allowlist `is_significant` (mine.rs) é por kind
e não os inclui — a mineração de papéis continua cega a membros.

## Proveniência e licença

Os patterns partem do `queries/tags.scm` upstream de cada gramática (todas
MIT) onde o upstream cobre o caso, adaptados ao vocabulário de captura do
motor (o upstream usa `@definition.*`/`@name` da convenção de *code
navigation* do tree-sitter; mantivemos os sufixos de kind compatíveis):

| Query set    | Gramática upstream (crate)                  | Licença | Origem dos patterns |
|--------------|---------------------------------------------|---------|---------------------|
| `csharp/`    | tree-sitter/tree-sitter-c-sharp (0.23)      | MIT     | tags de tipo e membro derivadas do upstream; `field` é local (o upstream não taga fields) |
| `typescript/`| tree-sitter/tree-sitter-typescript (0.23)   | MIT     | métodos/abstract do upstream; `property`/`field`/`enum_member`/`const` locais |
| `go/`        | tree-sitter/tree-sitter-go (0.25)           | MIT     | `function`/`method` do upstream; `field`/`type`/`struct`/`interface` locais |
| `python/`    | tree-sitter/tree-sitter-python (0.25)       | MIT     | `class`/`function` do upstream (que também não separa method de function); `field` local |
| `rust/`      | tree-sitter/tree-sitter-rust (0.24)         | MIT     | itens do upstream (que também não separa method de function); `field`/`enum_member` locais |
| `php/`       | tree-sitter/tree-sitter-php (0.24)          | MIT     | `class`/`interface`/`trait`/`function`/`method` do upstream; `property`/`enum_member` locais |

Notas de decisão (por que não há kind `method` em python/rust): nessas
gramáticas o método é o MESMO nó da função (`function_definition` /
`function_item`); um segundo pattern sobre o mesmo nó deixaria o kind gravado
dependente da ordem de match do tree-sitter — não determinístico. O upstream
faz a mesma escolha. Em go/php o método é nó próprio (`method_declaration`),
então segue o upstream como `@definition.method`.
