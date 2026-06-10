# queries/ — consultas tree-sitter (DADO, nunca lógica)

Cada subdiretório é um *query set*: os arquivos `.scm` que definem o que o
motor genérico (`extract.rs`) extrai daquela gramática. O motor só entende o
vocabulário genérico de captura (`@import`, `@namespace`, `@name`,
`@supertype`, `@definition.<kind>` — o sufixo de `kind` é copiado verbatim).
Nenhum nome de nó de gramática existe em `src/`; adicionar linguagem é dado:
uma linha em `languages.toml` + `.scm` aqui + fixture `graph_<dir>` + entrada
no `kinds-manifest.toml` — o teste de paridade (`tests/kinds_parity.rs`) acusa
lacuna sozinho.

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
