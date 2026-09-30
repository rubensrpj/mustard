# scan

Lê um projeto e grava o **mapa** dele: um banco SQLite (`grain.db`) com o censo
do projeto, os arquivos, as declarações, o grafo de dependências e a história do
git. **Agnóstico a framework e a linguagem** — não conhece React, .NET, GraphQL
nem nada. Tudo o que é próprio de uma linguagem, de um sistema de build ou de um
gerador de código é **dado** (tabelas e consultas embutidas no binário), nunca
lógica em `src/`.

## Como funciona (o pipeline)

```
ingest → extract → graph → condense → grain.db
└────── tudo determinístico, cego a framework e linguagem ──────┘
```

1. **Varredura** (`ingest.rs`). Percorre a árvore respeitando o `.gitignore`,
   pula sempre as pastas que nunca guardam código do projeto (`skip_dirs` em
   `manifests.toml`: `.git`, `.claude`) e as de saída e de dependência
   (`output_dirs`: `build`, `bin`, `vendor`, `node_modules`…) quando o índice
   do git não guarda nelas nenhum arquivo de código — fora do git, sempre. Cada
   pasta pulada vai à cobertura do mapa pelo caminho. Detecta a linguagem de
   cada arquivo pela extensão, conta as linhas e lê os manifestos de build.
2. **Extração por consulta** (`extract.rs`). Um motor tree-sitter só roda as
   consultas `.scm` de cada linguagem e tira de cada arquivo os imports, os
   namespaces, as declarações, as chamadas e as citações (ver abaixo).
3. **Grafo** (`graph.rs`). Liga os imports aos arquivos do projeto, conta as
   arestas, acha as camadas e liga as declarações entre si.
4. **Esqueleto** (`condense.rs`). As 25 pastas (até o segundo nível) com mais
   arquivos, cada uma com a camada média dos arquivos dela.

Depois disso o scan junta os testes de cada arquivo, a história do git, as
pilhas detectadas e o estado da leitura, e grava tudo no banco.

**Extração via tree-sitter, genérica e plugável.** A extração é **um único motor
tree-sitter** (`extract.rs`), agnóstico por construção: ele não conhece nenhuma
linguagem nem nome de nó de gramática. Cada linguagem é **dado** — uma linha em
`languages.toml` (nome, extensões, gramática) + arquivos de consulta `.scm` sob
`queries/<dir>/`. As consultas usam um vocabulário de captura genérico, listado
inteiro em [`queries/README.md`](queries/README.md#capturas); o motor só
entende essas capturas e devolve a mesma forma para toda linguagem. De cada
declaração fica o tipo, o nome, as linhas de começo e de fim, os **supertipos**
— `class X : Base, IFoo` (C#), `impl Trait for T` (Rust), `extends`/`implements`
(TS), `class Foo(Base)` (Python) —, a documentação (o comentário escrito acima
dela ou, na linguagem que a escreve dentro, a docstring), a assinatura (o
cabeçalho, sem o corpo), o **dono** — o tipo que a contém ou, quando a
linguagem escreve o dono fora dela, o da captura `@owner`, como o tipo do bloco
`impl` do Rust e o receptor do método do Go — e o **contrato**, a interface ou
trait que ela cumpre por onde foi escrita (a captura `@owner.contract`, como o
traço de `impl Traço for Tipo`). De cada arquivo ficam também as
chamadas e as citações de nomes, com a linha, e os trechos de teste escritos
dentro dele. **Detecção de linguagem** também é dado: vem da tabela de extensões
do mesmo registro, não de um `match`.

**Grafo e camadas emergentes (sem vocabulário).** Os imports são resolvidos para
os arquivos do projeto por formas genéricas (namespace, caminho de arquivo,
apelido de pasta da configuração do projeto, pacote do próprio workspace, entre
outras), e o que é próprio de uma linguagem nessas formas vem do registro; o
que não resolve para nada interno é dependência externa. O grafo define a
estratificação: condensa ciclos num DAG e a profundidade de cada arquivo é o
maior encadeamento de dependências (L0 = mais dependido / mais interno). Não há
lista de "domain/application/infra" cravada — as camadas saem da direção real
dos imports. A única quebra de direção que a topologia prova sem nomear camada é
um **ciclo de dependência**, e o resumo impresso diz se há um. Do grafo saem
ainda os arquivos mais importados (fan-in) e os **pontos de registro**: arquivos
que importam de muitas pastas (container de injeção, menu, arquivo que
reexporta), onde se mexe ao acrescentar algo.

**Declarações ligadas.** Cada declaração guarda quais declarações do projeto ela
chama e cada lugar que a usa, com o arquivo, a linha e a declaração de onde a
chamada parte. Um nome só se liga às declarações que o arquivo que o escreve
enxerga — as do próprio arquivo, as dos arquivos que ele importa, as que um
import global da linguagem põe à vista e as do mesmo namespace na mesma
linguagem. O nome escrito sozinho que o próprio arquivo declara é o dele, e
nenhum outro; o import que diz o que traz põe à vista desse nome só o que
trouxe; e a chamada sem nada à vista fica suspeita entre as da mesma família de
linguagem, mesmo com uma só. A chamada escrita num trecho de teste é do teste e não conta como
uso. Cada tipo guarda os **membros** dele, os métodos primeiro, e cada método
guarda as **implementações**: o método do tipo diz qual método do contrato ele
implementa (`implements`), e o método do contrato diz quem o implementa
(`implemented_by`). Membros e implementações saem do dono e do contrato de
cada declaração e, como os usos, se refazem do projeto inteiro a cada passada.
A regra de quem é membro e de qual contrato vale está em
[`queries/README.md`](queries/README.md#capturas), junto das capturas.

**Testes de cada arquivo** (`testmap.rs`). Um arquivo é coberto pelo teste que o
importa e pelo teste que muda junto com ele no git. O arquivo que traz o próprio
teste (um marcador dos dados de teste do núcleo) diz isso sozinho, e cobre o que
o trecho de teste dele importa.

**Consciência de projeto.** Cada manifesto (`.csproj`, `package.json`, `go.mod`…,
registrados em `manifests.toml`) vira um projeto; cada arquivo é atribuído ao
projeto de prefixo mais longo, e dois manifestos na mesma pasta viram um projeto
só. As dependências e os scripts de build de cada projeto são lidos crus dos
manifestos dele, sem lista de framework; as dependências mais frequentes, na
ordem em que o manifesto as escreve no empate, viram a lista de frameworks do
projeto.

**Pilhas detectadas.** As pilhas do projeto, e as de cada subprojeto, saem da
convergência de três evidências — as dependências dos manifestos, os caminhos
dos arquivos e as assinaturas no código —, pelo registro de pilhas do
`mustard-core`. A evidência de um arquivo de teste não conta: ela diz o que o
projeto testa, não o que ele é. Arquivo de teste é o que a regra do núcleo
(`is_test_path`) diz que é, pelos dados de
`packages/core/src/domain/ast/test-files.toml` — pela pasta (`tests/`,
`fixtures/`, `e2e/`…), pela pasta de projeto de teste (`MeuApp.Tests/`) ou pelo
nome (`foo_test.go`, `x.spec.ts`, `login.cy.ts`…) —, a mesma regra que o mapa
de testes e os pontos de registro leem. O mesmo arquivo guarda os marcadores de
teste dentro do arquivo.

**Arquivos escritos por máquina** (`classify.rs`). Cada arquivo gerado,
vendorizado, lockfile ou minificado é marcado, com o marcador que decidiu. O
catálogo é dado (`generated-markers.toml`), e o `.gitattributes`
(`linguist-generated`) e o `.editorconfig` (`generated_code`) do próprio
repositório vencem o catálogo nos dois sentidos. A marca não tira o arquivo do
mapa; quem lê o mapa é que o deixa fora da busca e dos exemplos.

**História.** Os commits vêm do repositório local (`git log`), nunca da rede:
cada commit com a data e os arquivos que ele criou e mudou. A primeira passada lê
a história inteira; as seguintes, só os commits novos.

**História por declaração.** `scan history-all <raiz> --out <mapa>` lê a
história de todo arquivo do mapa que ainda não a tem, na branch de partida,
numa passada só pelo projeto (`git log --reverse -p -U0`, do commit mais antigo
ao mais novo, só os trechos que mudaram): cada linha se acompanha pelo texto
dela, sem os espaços das pontas, e pelas renomeações que o git vê. Da versão da
ponta sai, de cada declaração, a lista dos commits que escreveram as linhas
dela, do mais novo ao mais velho, cada um com o título e o número do pull
request; o nascimento é o commit mais antigo entre as linhas. A linha que só
fecha um bloco ou escreve um `else` não conta, porque se repete pelo arquivo
todo. A linha mudada vai para a declaração mais interna que a contém, e a entre
declarações não vai a nenhuma; a linha que muda de arquivo no mesmo commit leva
a história dela junto, até `map.historyMoves` vezes seguidas, e a que só existe
numa junção é do commit da junção. O commit que só muda espaços, ou que o
projeto lista no `.git-blame-ignore-revs`, fica marcado como só de forma.
`scan history <raiz> --out <mapa> --file <arquivo>` segue o mesmo caminho e o
mesmo limite, para um arquivo só, e o relato dele traz `read` e `limited`
como o do projeto inteiro.

A primeira passada lê no máximo os `--newest` commits mais novos que mexem nos
arquivos (5.000) e o relato diz `limited` quando cortou: a linha que nenhum
deles escreveu é mais velha que todos e fica com o mais antigo dos lidos. As
seguintes partem das listas guardadas — cada uma guarda o commit da ponta em
que foi lida — e leem só o que veio depois dele, o campo `read` do relato
dizendo quantos commits o git deu: a lista somada leva aos commits novos os que
a declaração já tinha, e por isso uma declaração de que só parte das linhas
mudou pode listar mais commits que uma leitura do começo. Se a base foi
reescrita, ou o arquivo foi renomeado por cima de outro, ou o arquivo não tem
lista e já existia onde a leitura anterior parou, a passada volta ao começo. A
montagem não roda esta passada: `mustard-rt run map history` a roda na primeira
pergunta sobre o arquivo, e de novo só quando a base, a versão do scan ou o
commit mais novo do arquivo mudam.

O erro da leitura que só soma o que é novo cresce a cada uma delas. O mapa
guarda quantos commits elas leram desde a última leitura do projeto inteiro
desde o começo; quando a soma chega a 50, a `history-all` seguinte relê todos
os arquivos desde o começo, com o mesmo `--newest`, e a soma volta a zero; a
história guardada dos arquivos que já saíram do mapa sai junto, e a lista de
todos fica a de uma leitura feita do zero. A busca segue lendo o que está
gravado durante essa releitura, que grava em lotes como qualquer outra. A
`history-all` baixa a própria prioridade (10, no Unix) ao começar, e o git que
ela abre herda: a busca passa na frente dela.

**Leitura incremental.** Com um mapa anterior do mesmo projeto no `--out`, o
scan relê só os arquivos que mudaram desde a passada anterior e toma o resto do
mapa como estava; o resultado é o mesmo mapa que uma leitura completa daria. Só
os blocos do banco que mudaram se regravam, numa transação só, e sem nada mudado
o arquivo fica intocado. O que mudou se decide pelo **blob do git** de cada
arquivo: o do índice, para o arquivo comitado e intocado, ou calculado sobre o
conteúdo de agora, quando o arquivo está sujo ou é novo. O arquivo cujo blob é
o mesmo que a passada anterior leu é tomado do mapa como estava, em qualquer
ramo, comitado ou não. Com o mesmo commit, sem arquivo a reler e sem arquivo
de código ou manifesto que entrou ou saiu — só um `README.md` editado, um
`artisan` novo —, o scan nem abre o mapa inteiro: lê só o estado, caminha pela
pasta sem abrir arquivo e regrava só o censo, com a marca da listagem nova, as
pastas de build e as pilhas refeitas pelos caminhos de agora; as declarações,
o grafo e a história ficam como estavam. Fora do git não há blob, e tudo se
relê; tudo se relê também quando muda um arquivo que muda a leitura de todos
os outros. Um mapa
gravado por um scan compilado de outras fontes (outro motor, outras consultas
ou outras tabelas de dados) é relido inteiro. `--all` relê tudo.

O commit lido e a marca da listagem (uma marca curta de todos os pares caminho
e blob daquela passada) moram no censo. Antes de cada pergunta ao mapa, o
Mustard os confere com os de agora, sem ler o mapa inteiro, e, se um dos dois
mudou, roda o scan, que relê só os arquivos de blob novo.

**Relatório de cobertura.** Todo `scan` imprime o que foi lido por diretório de
topo, quais pastas de build foram puladas (`bin`, `obj`…) e quais extensões
foram vistas mas não lidas (`.sql`, `.json`…) — resposta verificável para "li
todos os diretórios?".

**Nada de catálogo embutido.** O scan não conhece framework nem gerador. O que
nomeia uma linguagem, um sistema de build ou um gerador mora nas tabelas de
dados (`languages.toml`, `manifests.toml`, `generated-markers.toml` e, no
núcleo, `test-files.toml`) e nas consultas de `queries/`; `src/` só tem o motor
genérico.

## Build

```bash
cargo build --release -p scan   # na raiz do repositório
```

> Compila com o Rust fixado em `rust-toolchain.toml`, na raiz, e requer um
> compilador C — as gramáticas do tree-sitter são compiladas pelo crate `cc` (no
> Windows, as ferramentas C++ do Visual Studio; em Linux/macOS, gcc/clang). Não
> há rede em tempo de execução: gramáticas e consultas são embutidas no binário
> em tempo de build (ver `build.rs`).

## Uso

```bash
# lê o projeto e grava o mapa (o banco SQLite)
scan scan ./meu-projeto --out grain.db

# relê todos os arquivos, ignorando o mapa anterior
scan scan ./meu-projeto --out grain.db --all

# uma linha JSON com o que foi lido, no lugar do resumo
scan scan ./meu-projeto --out grain.db --json

# a marca que a passada grava em cada bloco: a versão e o resumo das fontes do scan
scan format
```

O `mustard-rt` compara essa marca com a dos blocos do mapa antes de responder:
o mapa que outra compilação do scan gravou é lido de novo por inteiro, mesmo
com o projeto parado no mesmo commit e com o mesmo conteúdo.

Depois de gravar o banco, o scan apaga o mapa de antes dele (`grain.model.json`)
na mesma pasta.

Tudo é determinístico e offline; **o scan nunca chama um modelo de IA**.

## Linguagens

Entregue funcionando: **C#, TypeScript/TSX, Python, Rust, Go, PHP, Dart**.
Adicionar mais é trivial — a language-pack do tree-sitter cobre centenas. Nada
de linguagem está cravado no código: a detecção e a extração são puramente
**dados**.

### Adicionar uma linguagem nova (só dados/consultas)

Nenhuma mudança na **lógica** do scan é necessária — `src/` não contém nome de
linguagem, extensão nem nó de gramática. O fluxo:

1. **Consulta** — crie `queries/<dir>/tags.scm` (e, opcional, `supertypes.scm`)
   usando o vocabulário de captura genérico, listado inteiro, com o que cada
   captura faz, em [`queries/README.md`](queries/README.md#capturas).

   Exemplo (C#): `(class_declaration name: (identifier) @name (base_list (_) @supertype)) @definition.class`.

   Convenções de framework (Drizzle, GraphQL, ORM…) **não** entram aqui. O que
   você adiciona à consulta é só a **forma de sintaxe** daquela linguagem (ex.:
   `export const X = call(...)`, decorators) — o scan nunca "sabe" o que é
   Drizzle.

2. **Registro** — adicione um `[[language]]` em `languages.toml`
   (`name`, `extensions`, `dir`, `grammar`; os campos opcionais estão descritos
   no começo do arquivo). A detecção por extensão sai daí automaticamente.

3. **Gramática** — *só se a gramática ainda não estiver linkada*: adicione o crate
   ao `Cargo.toml` com um alias neutro (ex.:
   `grammar_kt = { package = "tree-sitter-kotlin", version = "…" }`) e aponte o
   campo `grammar` da `languages.toml` para a constante `LanguageFn`
   (ex.: `grammar = "grammar_kt::LANGUAGE"`).

4. **Paridade** — declare os tipos de declaração da consulta em
   `queries/kinds-manifest.toml` e crie a fixture `tests/fixtures/graph_<dir>/`;
   o teste de paridade (`tests/kinds_parity.rs`) acusa a lacuna sozinho.

O `build.rs` lê `languages.toml` + `queries/` e embute tudo no binário; o motor
genérico em `extract.rs` roda a consulta e devolve a mesma forma de sempre. Se a
gramática já estava linkada, é mudança 100% de dados; se for nova, recompila
para linkar — mas **a lógica em `src/` não muda**. Padrões de consulta que não
casarem com a versão da gramática são pulados individualmente (resiliência),
nunca derrubam a linguagem inteira.

### Auditoria de agnosticidade

```bash
grep -rinE 'csharp|typescript|"\.cs"|"\.rs"|class_declaration|base_list' src/
# (sem resultados — nenhum vocabulário de linguagem na lógica)
```

O teste `tests/engine_is_language_blind.rs` faz a mesma conferência com todo
nome e toda pasta que o `languages.toml` declara.

## O mapa

O **produto é o `grain.db`** — o mapa, um banco SQLite em blocos, cada bloco com
as tabelas dele e a marca do scan que o gravou:

- **census** — a raiz, o estado da leitura (o commit lido, a marca da listagem
  e o blob dos arquivos que decidem a releitura sem ser código: os manifestos,
  os que mudam a leitura de todos e os que não se decodificaram), as
  dependências, as pilhas detectadas, as pastas puladas, os projetos, as
  linguagens, os manifestos e o esqueleto das pastas;
- **files** — cada arquivo, com o blob do git do conteúdo lido, a linguagem, as
  linhas, a classe de arquivo gerado, os namespaces, os imports e os trechos
  de teste;
- **decls** — cada declaração, com o arquivo, o tipo, o nome, as linhas, a
  assinatura, a documentação, os supertipos, o que ela chama, quem a usa, o
  dono, o contrato, os membros (num tipo) e as implementações (num método); e
  o índice da busca do mapa, refeito sempre que os arquivos ou as declarações
  se regravam: as palavras de cada declaração e de cada arquivo, campo a
  campo, preparadas nas línguas do `mustard.json` da raiz lida, e os nomes das
  declarações para a busca por pedaço do nome. O arquivo escrito por máquina
  fica fora dele;
- **graph** — de cada arquivo, o que ele importa do projeto, os testes que o
  cobrem, as chamadas e as citações; e os números do grafo: o tamanho, os mais
  importados, as camadas e os pontos de registro;
- **history** — os commits, cada um com a data e os arquivos que criou e mudou,
  e a tabela dos caminhos que eles citam;
- **lineage** — a história por declaração de cada arquivo que alguém já
  perguntou: a base, o commit mais novo do arquivo e a versão do scan de quando
  se leu; os commits (começo do hash, data, título e número); e, de cada
  declaração (nome e ordem no arquivo), os commits dela, com a marca de só
  forma. Nada de código antigo. A montagem não grava este bloco; a passada
  `history` troca só as linhas do arquivo que leu.

`mustard-rt run map dump` mostra o banco tabela por tabela, fora o índice da
busca, que se refaz do mapa.

## Licença

MIT.
