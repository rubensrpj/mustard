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
| `@import` | Um import ou `using`, ou o texto passado ao `require('m')`, que importa como o `import`; o texto é limpo até virar o caminho, que a resolução do grafo liga aos arquivos do projeto. O que fica dentro da captura nunca vira chamada nem citação: é o caminho do import, não um uso. |
| `@import.global` | Um import que vale para todo arquivo da mesma linguagem sob a pasta do manifesto mais próximo acima de quem o escreve (sem manifesto, sob a pasta do próprio arquivo). Vai para `Module.global_imports`; o mesmo nó continua sendo `@import` do arquivo que o escreve, e a aresta do grafo de import fica só nele — e, como todo import de namespace, liga só aos arquivos que declaram um nome que esse arquivo chama ou cita. |
| `@imported` | Um nome que o import traz para o arquivo (`limite` em `import { limite } from`, `from m import limite`, `use m::limite`, `use function`/`use const`, `show limite`, `x`, `a` e `b` em `const x = require('m')` e `const { a, b } = require('m')`). Diz o que o arquivo trouxe; como no `@import`, o nome escrito ali não é uso: o uso de verdade vem depois, no corpo do arquivo. Capturado no mesmo pattern de um `@import` feito só do separador relativo da língua (`relative_import` no `languages.toml`, como o `.` de `from . import models`), o nome é um arquivo da pasta, e o import vira o separador seguido dele (`.models`). O nome que a língua usa para trazer o próprio caminho (`import_self` no `languages.toml`, como o `self` de `use std::fs::{self}`) traz o último nome escrito antes da lista (`fs`). O nome trazido por um import que não nomeia nenhum arquivo do projeto é de fora: escrito sozinho ou antes de outro nome (`fs::remove_dir_all()`), não liga a nada do projeto. O apelido que o import dá ao módulo inteiro também é nome trazido: `s` em `import loja.servico as s` (Python), `s` em `import s "strings"` (Go), o prefixo `c` de `import 'x.dart' as c;` (Dart), `u` em `import * as u from './util'`. O nome trazido por um import do projeto que nenhum arquivo dele declara é o próprio módulo: escrito antes de outro nome (`u.join()`), nomeia os arquivos desse import e os que declaram o nome a partir deles, seguidos os repasses; o de uma letra só, que fora disso é valor, fica nome quando um import do arquivo o trouxe. Para o nome escrito sozinho, o arquivo que só imports que trazem nomes alcançam fica à vista só dos nomes que eles trouxeram; o import que não traz nenhum (`import 'x.dart';`, `using X;`, `use x::*;`, `from x import *`, `import x` no Python) põe o arquivo inteiro à vista. |
| `@reexport` | O caminho de um repasse: o arquivo oferece a quem o importa nomes tirados de outro (`export * from './x'`, `export { a } from './x'`, `pub use io::leitor::Leitor`). É import do arquivo, como o `@import`, e o caminho, limpo do mesmo jeito, vai para `Module.reexports` com os nomes que ele oferece: os `@imported` do mesmo comando, e todos (`*`) quando não escreve nenhum (`export *`, `pub use x::*`). Na língua sem `export`, em que o nome que um módulo importa é importável dele, todo import que traz nomes é repasse (`from x import a` e `from x import *` no Python). O repasse escrito dentro de um módulo do arquivo (`@inner_module`) não é do arquivo e não entra. Quem importa o arquivo e traz um nome que ele não declara, mas repassa, liga ao arquivo que declara o nome, seguindo quantos repasses houver até ele, sem voltar a um já visto; o nome que nenhum declara fica no arquivo importado, e o arquivo que repassa segue como alvo só pelos nomes que ele mesmo declara ou pelo import que não traz nome. |
| `@imported.original` | O nome que o arquivo de origem dá ao nome que o import ou o repasse do mesmo pattern traz com outro (`Leitor` em `import { Leitor as L } from './pasta'`, `a` em `export { a as b } from './x'`, `ler` em `const { ler: lerPedido } = require('./x')`, `B` em `use a::B as C` e em `use a::{B as C}`, `a` em `from m import a as b`), capturado junto do `@imported` do nome novo. Vai para `Module.brought` com o nome trazido, e o nome sem troca vai com ele mesmo. O arquivo alvo é pedido pelo nome de origem, que o apelido ninguém declara, e o nome novo escrito no corpo, sozinho (`L()`) ou antes de outro (`L::novo()`), liga às declarações pelo nome que elas têm no arquivo aonde a resolução chegou, seguidas também as trocas feitas nos repasses do caminho. No repasse, quem importa pede o nome novo (`b`), e o repasse o tira do arquivo de origem pelo de origem (`a`). A origem `*` diz que o nome novo é o arquivo inteiro que o caminho nomeia (`util` em `export * as util from './util'`): quem pede `util` chega a esse arquivo, sem descer nele, e o que se escreve depois dele (`util.soma()`) liga às declarações dele. O apelido que nomeia o módulo inteiro (`import x as y`, o apelido do Go, o prefixo do Dart) não tem esta captura e fica com o próprio nome. Como no `@import`, o nome escrito ali não é uso. |
| `@local` | Um nome que o corpo de uma função liga: variável, parâmetro, parâmetro de função anônima, nome de laço, nome tirado de desestruturação. Da linha seguinte até o fim da declaração que o contém, também dentro de uma declaração escrita ali, o mesmo nome escrito sozinho é dele, e não chamada nem citação de outra coisa do projeto; na própria linha, não (`let hoje = hoje(None);` chama a função). Fora de toda declaração, não corta nada. O nome local escrito antes de uma chamada é do arquivo: a chamada aberta por ele nunca é tomada por chamada de biblioteca (ver `@call.path`). |
| `@call.path` | O caminho escrito antes do nome numa chamada qualificada, já sem o nome chamado (`crate::a::b` em `crate::a::b::f()`). Vira import do arquivo, como o `@import`, só quando a primeira parte dele, cortada nos `qualified_separators` da língua, é um dos `root_aliases` ou o `parent_alias` do `languages.toml`; aí o nome escrito ali também não é uso. Os argumentos de tipo escritos no caminho saem dele (`crate::a::Caixa::<u8>` vira `crate::a::Caixa`). O nome chamado é o nó nomeado logo depois do caminho: a chamada vai, com o caminho, para `Module.call_paths`, e liga só às declarações do arquivo que o caminho nomeia (o próprio arquivo só quando o caminho o nomeia). O caminho de uma parte só (`Vec::new()`) é ignorado, e o nome dele segue como qualificador da chamada. O outro caminho de duas partes ou mais (`std::fs` em `std::fs::read()`) vai, com a chamada escrita por ele, para `Module.other_call_paths`, sem virar import: quando a raiz dele não é peça do projeto (nenhum apelido, pacote, arquivo ou pasta dele, nem nome que um import do projeto trouxe), a chamada é da biblioteca e não liga a nada do projeto, mesmo quando o nome de perto (`fs`) é uma pasta dele. A mesma regra vale para a chamada sem caminho capturado cujo qualificador abre a cadeia (`File` em `File.ReadAllText()`): o nome que abre a cadeia decide, e ele é peça do projeto quando é declaração, namespace, apelido, pacote, arquivo ou pasta dele, ou nome que um import do projeto trouxe, e do arquivo quando é nome local (`@local`), o próprio objeto (`self_receivers`) ou o objeto visto pelo tipo de cima (`parent_receivers`). Fora disso, a chamada é da biblioteca e não liga. O qualificador escrito depois de outro nome (`repo` em `this.repo.salvar()`) não abre a cadeia, e a regra não o julga. |
| `@test_block` | Um trecho de teste escrito dentro do arquivo (o módulo marcado como teste). O import escrito nele, pelo `@import` ou pelo `@call.path`, vai para `Module.test_imports`, fora de `imports`, e por isso fora de `deps` e do grafo; resolvido, vai para `Module.test_deps`, e o arquivo importado lista este entre os testes que o cobrem. A chamada e a citação escritas nele (`Module.test_lines`) não entram em quem usa a declaração. |
| `@inner_module` | Um módulo com corpo escrito dentro do arquivo, o trecho de teste incluído. As linhas dele vão para `Module.module_lines`, e as de cada import escrito ao menos uma vez dentro de um deles, para `Module.import_lines`. O caminho que começa pelo `parent_alias` do `languages.toml` e é escrito dentro de N desses módulos sai primeiro deles, com os N primeiros apelidos, e só com os outros sobe pasta. |
| `@namespace` | O nome do namespace ou do pacote que o arquivo declara. Como o namespace se enxerga entre os arquivos é o campo `namespace_scope` do `languages.toml`. Como no `@import`, o nome escrito ali não é uso. |
| `@definition.<kind>` | Uma declaração; o sufixo `<kind>` vira `Decl.kind` literalmente. |
| `@name` | O nome da `@definition.*` do mesmo pattern. |
| `@supertype` | Um tipo-base, interface ou trait; o motor o liga, pelo nome, à declaração de mesmo `@name`, mesmo quando capturado num nó separado dela. |
| `@owner` | O tipo dono escrito fora da `@definition.*` do mesmo pattern: o tipo do bloco `impl` do Rust, o receptor do método do Go. Só o nome, limpo como o do `@supertype`; o corpo em volta não é lido. Vai para `Decl.owner` depois das declarações do mesmo arquivo cuja faixa contém a dela, que são os donos dela sem captura nenhuma, da mais interna para a mais externa (a mesma faixa não conta). |
| `@owner.contract` | O contrato que a `@definition.*` do mesmo pattern cumpre por onde foi escrita: o traço de `impl Traço for Tipo`. Vai para `Decl.contract`. |
| `@decoration` | Um atributo ou anotação: o cabeçalho da declaração começa depois dele, o comentário acima passa por cima dele, e nada dentro dele vira chamada nem citação. |
| `@body` | O corpo que a gramática põe ao lado da declaração, e não dentro dela: a declaração termina onde o corpo termina. |
| `@value` | O valor dado à declaração: o cabeçalho para onde ele começa, e o `=` que sobra no fim sai (`export const PRECOS = { ... }` fica `export const PRECOS`). |
| `@doc` | A documentação que a linguagem escreve dentro da declaração, e não em cima dela (a docstring do Python); o texto da captura já vem sem as aspas. O motor a junta à declaração do mesmo pattern, e o comentário de cima, quando existe, vale mais. Sem `@definition.*` no pattern, ela só diz que o literal que a contém é documentação, e não texto fixo. |
| `@text` | Um literal de texto escrito no código, com as aspas. O motor guarda o valor, sem as aspas e o prefixo, numa linha e cortado em 300 caracteres, quando ele tem duas palavras ou mais (palavra é o trecho entre espaços com duas letras ou mais) ou forma de caminho ou chave (sem espaço nem aspas, com `/`, `.`, `_` ou `-` e dois pedaços de duas letras ou mais). O literal que cai num import, num trecho de teste ou num `@doc` não entra. Cada um vai para `Module.texts` com a linha, a declaração que contém a linha e uma marca: log, quando o nome mais próximo escrito antes dele, subindo pela árvore, casa com `log_calls` do `languages.toml`; erro, quando casa com `error_forms`; texto, no resto. |
| `@text.plain` | Um texto escrito sem aspas, como o texto solto entre as marcas de uma tela. Segue o caminho do `@text` (uma linha, 300 caracteres, a mesma cara de texto, os mesmos trechos de fora, a declaração que contém a linha e a marca), mas o valor é guardado como está escrito: sem aspas a tirar, o `'` do começo de `It's empty` fica. |

Qualquer outro nome de captura é ignorado.

Do dono de cada declaração o grafo tira as ligações, refeitas do projeto
inteiro a cada passada: os membros de um tipo (`class`, `struct`, `record`,
`interface`, `trait`, `enum`, `type`) são as declarações cujo dono mais interno
é ele, os métodos primeiro, e no enum só `enum_member`, `method`, `function`,
`constant` e `const`; o método com o nome de um método do contrato o implementa
(`Decl.implements`, e do outro lado `Decl.implemented_by`). O contrato é o
`@owner.contract`; sem ele, quando o tipo dono contém o método, os
`@supertype` do dono. Um nome de tipo repetido no projeto vale o do mesmo
arquivo; senão, o de caminho com mais pastas em comum no começo; empatado, a
ligação não entra.

## kinds-manifest.toml

Inventário declarado de `@definition.<kind>` por query set. O teste de
paridade escaneia a fixture `tests/fixtures/graph_<dir>/` de cada entrada e
verifica nas duas direções: todo kind declarado produz ≥ 1 declaração, e todo
kind produzido está declarado. É essa a rede que pega um pattern que parou de
compilar contra a versão da gramática (o motor descarta pattern ruim em
silêncio, por design).

Kinds de membro (`method`, `property`, `field`, `enum_member`) chegam ao mapa
junto com as outras declarações do arquivo, e o grafo lista cada um sob o tipo
dono dele (veja `@owner` acima).

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
