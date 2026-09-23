# grain

Aprende o **veio** (grain) de um codebase a partir do que **se repete** e o expõe
como um **modelo rico** (`grain.model.json`) — e, por tarefa, uma **spec de
implementação autossuficiente**. **Agnóstico a framework e a linguagem** — não
conhece React, .NET, GraphQL nem nada. Ele descobre as convenções do *seu* projeto.

## A ideia

Uma convenção é, por definição, uma **repetição**. Um time só tem "um jeito de
fazer CRUD" porque fez igual em `Order`, `Product`, `Customer`. Então grain não
pergunta *"isto é GraphQL?"* (catálogo do que eu previ) — pergunta *"o que se
repete com a mesma forma?"* (descoberta). A segunda não precisa conhecer
framework nenhum.

## Como funciona (o pipeline)

```
ingest → extract → graph → mine → condense → grain.model.json → [grain spec]
└──────────────── tudo determinístico, cego a framework e linguagem ──────────────┘
```

O coração são **dois mineradores**, ambos sem regex de framework:

1. **Papéis por frequência de nome** (`mine.rs`). Tokeniza todo nome
   (`CreateOrderHandler` → `[Create, Order, Handler]`). Um afixo que se emparelha
   com **muitos restos distintos** é um papel — `Repository` aparece com `Order`,
   `Product`, `Customer`. **Sufixo-primeiro**: o que sobra depois de remover o
   papel é a *entidade*. Isso resolve o problema de confundir entidade com papel,
   e acha `*Controller`, `*Resolver`, `*Store`, `*Bloc`, prefixo `use*` — o que
   existir no seu repo.

2. **Slices recorrentes + clustering** (`mine.rs`). Agrupa símbolos por entidade
   → um *slice* (corte vertical). Em vez de exigir formas idênticas (o que
   fragmentaria o CRUD em dezenas de variações), **clusteriza entidades por
   similaridade de forma (Jaccard)**. Cada cluster vira **uma** convenção com um
   **núcleo** (papéis presentes na maioria) + **opcionais** (recorrem, mas não em
   todas). Daí saem: a *regra* "da entidade ao slice completo" (passos do núcleo
   ordenados por dependência, opcionais marcados), e **três exemplares reais por
   complexidade** — simples, médio e complexo — pra o agente copiar o gabarito do
   tamanho do que precisa criar.

**Camadas emergentes (sem vocabulário).** O grafo de dependências define a
estratificação: condensa ciclos num DAG e a profundidade de cada módulo é o
maior encadeamento de dependências (L0 = mais dependido / mais interno). Não há
lista de "domain/application/infra" cravada — os tiers saem da direção real dos
imports. A única quebra de direção que a topologia prova sem nomear camada é um
**ciclo de dependência**, então é isso que ele conta.

**Papéis também saem das pastas e de classes aninhadas.** Além do sufixo do nome
de arquivo, o grain detecta (a) **nomes de pasta que recorrem** sob muitos pais
distintos (`DTOs/`, `Mappers/`, `Services/` sob cada módulo) — aí a entidade vem
da pasta-mãe; e (b) **classes de nome único que recorrem** em muitos módulos
(ex.: um `Validator` aninhado dentro do próprio arquivo de DTO). Um layout que
centraliza um tipo (`Domain/Entities`) tem pai único e cai naturalmente na
mineração por sufixo — o grain se adapta ao layout sem ser avisado de qual é.

**Colaboradores e desambiguação.** Cada papel ganha uma linha "Collaborates
with" — os namespaces que ele mais puxa (minerados dos imports, filtrando os de
muitos papéis, tipo System) — dando o fio pra efeitos colaterais (ex.: Service →
`Notification.Services`, `UnitOfWork`). E quando um mesmo sufixo cobre coisas
diferentes (um `Channel` de notificação que implementa `INotificationChannel` vs
um `Channel` de domínio que estende `EntityBase`), o glossário avisa e o exemplo
da role skill prefere o slice onde o papel é core.

**Contratos compartilhados.** A partir dos `supertypes` (preenchidos via
tree-sitter), o grain minera, por frequência, os tipos-base que muitas entidades
estendem/implementam (`EntityBase`, `RepositoryBase`, `IServiceBase`,
`AbstractValidator`…) e os lista no playbook como a fundação que todo slice usa;
o glossário passa a mostrar "usually implements X" por papel. Tudo por
recorrência — nomes de domínio (que são entidades mineradas) são excluídos, então
não há catálogo.

**Extração via tree-sitter, genérica e plugável.** A Layer 2 é **um único motor
tree-sitter** (`extract.rs`), agnóstico por construção: ele não conhece nenhuma
linguagem nem nome de nó de gramática. Cada linguagem é **dado** — uma linha em
`languages.toml` (nome, extensões, gramática) + arquivos de query `.scm` sob
`queries/<lang>/`. As queries usam um vocabulário de captura genérico, listado
inteiro em [`queries/README.md`](queries/README.md#capturas); o motor
só entende essas capturas e devolve o mesmo `Decl`/`Extracted` que o minerador
(Layer 4) já consome. Assim captura aninhamento real, genéricos, listas de base
multilinha e **`supertypes`** — `class X : Base, IFoo` (C#), `impl Trait for T`
(Rust), `extends`/`implements` (TS), `class Foo(Base)` (Python) — sem um `impl`
por linguagem no código. Um nome-base que muitas entidades estendem é um contrato
compartilhado, detectado por frequência, sem catálogo. **Detecção de linguagem**
também é dado: vem da tabela de extensões do mesmo registro, não de um `match`.

**Consciência de projeto.** Cada manifesto (`.csproj`, `package.json`, `go.mod`…)
vira um projeto; cada arquivo é atribuído ao projeto de prefixo mais longo. O
playbook lista o layout da solution e avisa que uma fatia de entidade costuma
atravessar vários projetos (ex.: domínio em `DataAccess`, serviços em
`Application`, endpoints em `Backend`).

**Relatório de cobertura.** Todo `scan`/`forge` imprime o que foi lido por
diretório de topo, quais pastas de build foram puladas (`bin`, `obj`…) e quais
extensões foram vistas mas não mineradas (`.sql`, `.json`…) — resposta
verificável para "li todos os diretórios?".

**Nada de catálogo embutido.** O grain não conhece framework, sufixo nem
estrutura de pasta. Os papéis saem da frequência de nomes; os caminhos da regra
são **abstraídos por entidade** — ele remove o token da entidade de *qualquer*
caminho, então um projeto que aninha por feature vira `…/<Name>s/Services/` e um
que centraliza fica `…/Entities/`, sem nenhuma suposição. Até as "dependências"
no playbook são lidas cruas do manifesto do projeto, não mapeadas por uma lista
minha. Um repositório de exemplo (`sample/`) acompanha o código apenas para
demonstração — ele não é base de nada.

Como o agrupamento é por nome de entidade, grain correlaciona **através da
stack**: o slice do `Order` junta o `OrderController` (C#) e o `OrderList` (React)
porque é a mesma entidade — algo que nenhum detector hardcoded faria.

As convenções são nomeadas pelo **vocabulário do próprio repo** — ex.:
*"DTO+Service+Repository+EndPoint slice"*. Honesto e determinístico; o Rust nunca
enumera framework. Qualquer nome semântico/prosa fica a cargo de quem consome o
modelo (a etapa de IA), não do grain.

## Build

```bash
cargo build --release   # offline, determinístico
```

> Requer Rust estável recente (verificado no 1.95) e um compilador C — as
> gramáticas do tree-sitter são compiladas pelo crate `cc` (no Windows, as
> ferramentas C++ do Visual Studio; em Linux/macOS, gcc/clang). Não há rede em
> tempo de execução: gramáticas e queries são embutidas no binário em tempo de
> build (ver `build.rs`).

## Uso

```bash
# o produto: minera e grava o modelo (JSON)
grain scan ./meu-projeto --out grain.model.json

# por tarefa: compila uma SPEC de implementação autossuficiente a partir do modelo
#   --entity: a entidade a criar    --like: entidade existente a espelhar
#   --ops:    operações além do CRUD base (ex.: approve)
grain spec ./meu-projeto --entity Invoice --like Order --ops create,approve --out invoice.spec.md
# (aceita um diretório p/ escanear OU um grain.model.json pronto)
```

> Tudo é determinístico e offline; **grain nunca chama um modelo**. A `spec` nasce
> como **rascunho hipotético** (banner "não verificado no código" + bifurcação +
> âncoras a ler); a etapa de IA que a lapida — ler os âncoras + resolver a
> bifurcação — vive no **consumidor** (ex.: o orquestrador), não no grain.

### A SPEC traz, por papel
folder-alvo (`<Name>` → sua entidade) · `implementa` (contrato) · `espelhe` (arquivo
real) · **exemplos básico/médio/complexo** (links de código) · colaboradores —
tudo **inline** (não depende de skills externas). Mais: a **bifurcação** (entidade
nova vs variante/tipo de uma existente), os **âncoras a ler** (capados), os **pontos
de registro** (DI/menu) e os **scripts/codegen** dos projetos envolvidos, e os
**critérios de aceite** (o gate).

## Usando de dentro do Claude Code

`grain scan` te dá o mapa do repo; `grain spec <dir> --entity X --like Y` te dá o
molde da tarefa. O consumidor (uma sessão/orquestrador) lê os **âncoras** que a spec
aponta, resolve a **bifurcação**, e então implementa seguindo o molde — criando os
mesmos arquivos, nas mesmas pastas, no formato das entidades existentes.

## Sintonia

Em `mine.rs`:
- `MIN_ROLE_PARTNERS` (default 2): com quantas entidades distintas um afixo
  precisa aparecer pra virar papel.
- `JACCARD_MERGE` (default 0.5): quão parecidas duas entidades precisam ser pra
  caírem na mesma convenção. Mais alto → convenções mais específicas (separa
  REST de GraphQL, por ex.); mais baixo → mais consolidação.
- `MIN_CLUSTER` (default 2): mínimo de entidades pra um cluster virar convenção.

Repos maiores toleram thresholds mais altos (menos ruído).

## Linguagens

Entregue funcionando: **C#, TypeScript/TSX, Python, Rust, Go**. Adicionar mais é
trivial — a language-pack do tree-sitter cobre centenas. Nada de linguagem está
cravado no código: a detecção e a extração são puramente **dados**.

### Adicionar uma linguagem nova (só dados/queries)

Nenhuma mudança na **lógica** do grain é necessária — `src/` não contém nome de
linguagem, extensão nem nó de gramática. O fluxo:

1. **Query** — crie `queries/<lang>/tags.scm` (e, opcional, `supertypes.scm`)
   usando o vocabulário de captura genérico, listado inteiro, com o que cada
   captura faz, em [`queries/README.md`](queries/README.md#capturas).

   Exemplo (C#): `(class_declaration name: (identifier) @name (base_list (_) @supertype)) @definition.class`.

   Convenções de framework (Drizzle, GraphQL, ORM…) **não** entram aqui: elas
   *emergem* da mineração. O que você adiciona à query é só a **forma de sintaxe**
   daquela linguagem (ex.: `export const X = call(...)`, decorators) — o grain
   nunca "sabe" o que é Drizzle.

2. **Registro** — adicione um `[[language]]` em `languages.toml`
   (`name`, `extensions`, `dir`, `grammar`). A detecção por extensão sai daí
   automaticamente.

3. **Gramática** — *só se a gramática ainda não estiver linkada*: adicione o crate
   ao `Cargo.toml` com um alias neutro (ex.:
   `grammar_kt = { package = "tree-sitter-kotlin", version = "…" }`) e aponte o
   campo `grammar` da `languages.toml` para a constante `LanguageFn`
   (ex.: `grammar = "grammar_kt::LANGUAGE"`).

O `build.rs` lê `languages.toml` + `queries/` e embute tudo no binário; o motor
genérico em `extract.rs` roda a query e devolve o mesmo `Decl`/`Extracted`. Se a
gramática já estava linkada, é mudança 100% de dados; se for nova, recompila para
linkar — mas **a lógica em `src/` não muda**. Padrões de query que não casarem com
a versão da gramática são pulados individualmente (resiliência), nunca derrubam a
linguagem inteira.

### Auditoria de agnosticidade

```bash
grep -rinE 'csharp|typescript|"\.cs"|"\.rs"|class_declaration|base_list' src/
# (sem resultados — nenhum vocabulário de linguagem na lógica)
```

## Fluxo, IA e confiança

O **produto é o `grain.model.json`** — o mapa minerado (papéis, contratos, slices,
shared_contracts, touchpoints de registro, tooling/codegen, quebra por projeto).
Tudo no grain é **determinístico; o grain nunca chama IA**.

São só **dois comandos**:
- `grain scan` → o modelo (o produto durável).
- `grain spec` → um **rascunho HIPOTÉTICO e autossuficiente** por tarefa: o banner
  *"não verificado no código"*, a **bifurcação a resolver** (a entidade é nova ou
  uma **variante/tipo** de uma existente? — sinalizado pelos próprios módulos, ex.:
  `*Type`/`*Assignment` indicam tabela de tipo / N:N), os **âncoras a ler** (um
  punhado de arquivos, **capado**), e, por papel, o contrato + os 3 exemplos
  (básico/médio/complexo) + colaboradores **inline** (não depende de skill externa).

> Geração de skills/agentes (.md) e o forge antigo foram **removidos** — eram
> índice/wrapper raso e a spec autossuficiente os torna desnecessários. O par
> **modelo + spec** basta.

A **IA entra em um lugar**: a **lapidação** — lê **apenas os âncoras** que o grain
aponta (não o repositório), resolve a bifurcação e produz a **spec final**. Depois,
o **subagente implementa**. Consumo é baixo por construção: o modelo (determinístico)
encolhe o trabalho da IA a uma leitura focada + uma escrita escopada por projeto.

> Por que o banner: a mineração por recorrência **não distingue** "entidade nova"
> de "variante de uma existente" — ela vê que tudo tem Service/Repo/DTO e assume um
> vertical novo. Por isso o plano nasce como **hipótese** e a leitura dos âncoras é
> **obrigatória** antes de criar arquivos. É onde a acertividade salta.

Fechamento (o gate): `grain verify <projeto> --entity X --like Y --ops …` recomputa os
**critérios de aceite** (mesma seleção da `spec`) e confere, no projeto, se cada local
exigido já tem arquivo da entidade — reportando `obrigatorios: N/M (%)`. Roda **depois**
de implementar; é o que transforma "acho que tá pronto" em "passou o checklist".

## Licença

MIT.
