# Esquema de proveniência e atualização de artefatos gerenciados

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full
### Checkpoint: 2026-05-19T21:05:00Z
### Lang: pt

## PRD

## Contexto

O Mustard embarca dezenas de artefatos vendorados em `templates/` — skills, recipes,
refs, commands, hooks — e depende também de ferramentas externas instaladas na
máquina, como o RTK. Vários desses artefatos têm origem externa (a skill Hallmark, o
skill-creator, as skills do mattpocock, o próprio RTK) e continuam evoluindo nos
repositórios de onde vieram, mas nem a cópia vendorada nem a versão fixada da
ferramenta guardam vínculo com essa origem: não há registro de onde cada artefato
veio, em que versão está, nem sinal de que o upstream avançou. Na prática `templates/`
e a versão pinada do RTK apodrecem em silêncio e a defasagem só aparece quando um
mantenedor compara à mão. Com o `mustard-dashboard` assumindo o papel de canal de
instalação, o instalador apenas embeda o payload `templates/` e o consumidor nunca
busca atualização na rede — então a única forma de o usuário receber artefatos
frescos é o mantenedor manter tudo atualizado antes de cada build. Hoje esse trabalho
de manutenção não tem ferramenta nem rastro, e é fácil esquecer um artefato defasado
por meses.

## Usuários/Stakeholders

Mantenedores do Mustard, que precisam saber quando um artefato vendorado ou uma
ferramenta pinada ficou para trás do seu upstream. Indiretamente, todo usuário do
Mustard, que recebe artefatos mais frescos a cada release do dashboard. Solicitado por
Rubens.

## Métrica de sucesso

`mustard-rt run artifact-update --check` lista, em JSON, todo artefato com upstream
externo — skills E ferramentas como o RTK — e marca quais estão defasados,
substituindo a comparação manual por um comando único e repetível.

## Não-Objetivos

- Não atualizar instalações de usuário — o consumidor recebe o que o instalador do dashboard embedou (cf. b6); o esquema é 100% maintainer-side.
- Não criar canal de update para artefatos `first-party` (commands/recipes/refs/hooks) — versionam junto com a CLI; o manifest apenas os rastreia.
- Não construir UI — a superfície do dashboard (badge "update disponível", ação install/update) é escopo da b6, que consome o `.claude/mustard.json#version` já existente.
- Não remover `design-craft` nem outras skills — o Hallmark entra lado a lado.
- Não alterar como o RTK é instalado (`ensure_rtk()` mantém o fallback Scoop/Cargo) — só passa a ler a versão pinada do manifest.
- Não espelhar artefatos first-party em repositórios externos.

## Critérios de Aceitação

Critérios binários, executáveis e independentes. Cada um roda da raiz do projeto; exit 0 = passou.

- [x] AC-1: O workspace compila — Command: `cargo build -p mustard-core -p mustard-rt -p mustard-cli`
- [x] AC-2: Testes de core e rt passam — Command: `cargo test -p mustard-core -p mustard-rt`
- [x] AC-3: O manifest existe, é JSON válido e enumera ≥13 artefatos de skill — Command: `node -e "const m=require('./apps/cli/templates/.artifacts.json'); const s=m.artifacts.filter(a=>a.category==='skill'); if(s.length<13){process.exit(1)}"`
- [x] AC-4: O subcomando `artifact-update` está registrado — Command: `node -e "const fs=require('fs');if(!/ArtifactUpdate/.test(fs.readFileSync('apps/rt/src/run/mod.rs','utf8'))){process.exit(1)}"`
- [x] AC-5: A skill Hallmark foi vendorada — Command: `node -e "const fs=require('fs');if(!fs.existsSync('apps/cli/templates/skills/hallmark/SKILL.md')){process.exit(1)}"`
- [x] AC-6: O RTK está registrado como artefato `tool` no manifest — Command: `node -e "const m=require('./apps/cli/templates/.artifacts.json'); if(!m.artifacts.some(a=>a.category==='tool'&&/rtk/i.test(a.id))){process.exit(1)}"`
- [x] AC-7: `artifact-update --check` roda e emite JSON parseável — Command: `cargo run -q -p mustard-rt -- run artifact-update --check | node -e "let d='';process.stdin.on('data',c=>d+=c).on('end',()=>{JSON.parse(d)})"`

## Plano

## Informações da Entidade

`ArtifactManifest` / `ArtifactRecord` — entidade de configuração, não de schema de
banco. O manifest (`apps/cli/templates/.artifacts.json`) é uma lista de registros;
cada registro tem `id`, `category` (skill|recipe|ref|command|hook|tool), `source`
(objeto com `kind` + campos por kind), `version`/`ref` vendorada e `vendoredAt`. Para
artefatos vendorados (skill/recipe/ref/command/hook) há ainda `path` (relativo a
`templates/`) e `checksum` (sha256 da árvore); para `tool` esses dois campos são
ausentes — a ferramenta não é vendorada, só rastreada por versão. O modelo Rust vive
em `packages/core`. O manifest não é um CORE_FOLDER — permanece maintainer-side, não é
copiado para instalações.

## Arquivos

- `packages/core/src/model/provenance.rs` (novo) — structs `ArtifactManifest`, `ArtifactRecord` (`path`/`checksum` opcionais), enum `ArtifactSource`; helper de checksum sha256.
- `packages/core/src/model/mod.rs` (edição) — registrar o módulo.
- `packages/core/src/lib.rs` (edição, se necessário) — re-export.
- `apps/cli/templates/.artifacts.json` (novo) — o manifest, enumerando todos os artefatos gerenciados (skills, recipes, refs, commands, hooks + Hallmark + RTK).
- `apps/rt/src/run/artifact_update.rs` (novo) — subcomando `artifact-update` (`--check`/`--apply`).
- `apps/rt/src/run/mod.rs` (edição) — `mod artifact_update;` + variante `RunCmd::ArtifactUpdate`.
- `apps/cli/templates/skills/hallmark/**` (novo) — skill Hallmark vendorada de `nutlope/hallmark`.
- `apps/cli/templates/skills/design-craft/SKILL.md` (edição) — afinar a `description` para não competir com Hallmark no auto-load.
- `apps/cli/src/commands/init.rs` (edição) — `ensure_rtk()` lê a versão pinada do RTK a partir do manifest.
- `CLAUDE.md`, `apps/cli/templates/CLAUDE.md`, `apps/cli/CLAUDE.md` (edição) — corrigir a contagem de foundation skills.
- `apps/rt/tests/` ou testes inline — cobrir o parse do manifest e o `--check`.

## Tarefas

### general-purpose Agent (Wave 1 — Modelo de proveniência + manifest)

- [x] Criar `provenance.rs` em `packages/core/src/model/`: `ArtifactManifest { schema_version, artifacts: Vec<ArtifactRecord> }`, `ArtifactRecord { id, category, source, version, vendored_at, path: Option, checksum: Option }`, enum `ArtifactSource` com variantes `FirstParty`, `Git { repo, subdir, ref }`, `SkillsDirectory { slug }`, `Cargo { crate }`, `Manual`. Derivar serde Serialize/Deserialize.
- [x] Implementar helper de checksum sha256 sobre a árvore de um artefato vendorado (ordenação estável de paths).
- [x] Registrar o módulo em `model/mod.rs` e re-exportar no `lib.rs` conforme o padrão existente.
- [x] Autorar `apps/cli/templates/.artifacts.json`: enumerar as 12 skills + recipes + refs + commands + hooks; classificar cada `source.kind` (skills do mattpocock e skill-creator → `git`/`manual` conforme upstream; commands/recipes/refs/hooks → `first-party`). Incluir os registros do `hallmark` (`skills-directory`) e do `rtk` (`tool`, `cargo`) — os arquivos/versões finais são preenchidos pelas Waves 3 e 4.
- [x] `cargo build -p mustard-core` e `cargo test -p mustard-core`.

### general-purpose Agent (Wave 2 — Motor artifact-update) — depende da Wave 1

- [x] Criar `apps/rt/src/run/artifact_update.rs`: subcomando que lê `templates/.artifacts.json` via `mustard-core`. `--check` compara cada artefato externo ao upstream e emite relatório JSON (`{ artifact, status: up-to-date|stale|unknown, local, upstream }`); `--apply` puxa atualizações para `templates/` (vendorados) ou bumpa a versão pinada no manifest (`tool`).
- [x] Handlers de origem: `git` via `git ls-remote`/clone de subdir; `skills-directory` via HTTP ao registry; `cargo` via API do crates.io (versão mais recente). Fail-open: rede indisponível → `status: unknown`, nunca erro.
- [x] Registrar em `apps/rt/src/run/mod.rs`: `mod artifact_update;` + variante `RunCmd::ArtifactUpdate { check, apply, ... }`, seguindo o padrão dos subcomandos vizinhos.
- [x] Testes: parse do manifest + `--check` offline retorna `unknown` sem panic.
- [x] `cargo build -p mustard-rt` e `cargo test -p mustard-rt`.

### general-purpose Agent (Wave 3 — Adoção do Hallmark) — depende da Wave 1, paralela às Waves 2 e 4 `(parallel-safe)`

- [x] Obter a skill Hallmark do upstream `nutlope/hallmark` (skills.directory / GitHub) — buscar a versão mais recente antes de vendorar. Copiar para `apps/cli/templates/skills/hallmark/` (SKILL.md + recursos), preservando o frontmatter de origem e o cabeçalho `<!-- mustard:generated -->` na ordem correta.
- [x] Finalizar o registro `hallmark` no manifest: preencher `checksum` da árvore vendorada e a `version` obtida.
- [x] Afinar as `description` de `hallmark` e `design-craft` para escopos distintos (Hallmark = anti-slop/landing/macroestrutura; design-craft = design system amplo), evitando competição no auto-load.
- [x] Corrigir a contagem de foundation skills em `CLAUDE.md`, `apps/cli/templates/CLAUDE.md` e `apps/cli/CLAUDE.md`.
- [x] `cargo build -p mustard-cli` e validar a skill: `cargo run -p mustard-rt -- run skills validate`.

### general-purpose Agent (Wave 4 — Adoção do RTK como tool) — depende da Wave 1, paralela às Waves 2 e 3 `(parallel-safe)`

- [x] Ler `ensure_rtk()` em `apps/cli/src/commands/init.rs` para identificar a versão do RTK hoje pinada e o crate/source de instalação.
- [x] Finalizar o registro `rtk` no manifest: `category: tool`, `source.kind: cargo` (crate confirmado a partir de `ensure_rtk()`), `version` = versão atualmente pinada, sem `path`/`checksum`.
- [x] Rewirar `ensure_rtk()` para ler a versão pinada do RTK a partir do manifest (`templates/.artifacts.json`, resolvido via `resolve_templates_dir()`) em vez de uma constante embutida — fechando o loop: `artifact-update --apply` bumpa o manifest e o `init` passa a instalar a nova versão. Fail-open: manifest ausente/ilegível → comportamento atual.
- [x] `cargo build -p mustard-cli` e `cargo test -p mustard-cli`.

## Dependências

- Spec b6 (`2026-05-18-b6-dashboard-projects`) — governa o canal de instalação; consome `.claude/mustard.json#version` para o badge "update disponível". Esta spec não toca o dashboard.
- Upstream `nutlope/hallmark` — fonte da skill Hallmark; precisa estar acessível na Wave 3.
- Upstream do RTK (crates.io) — fonte da versão da ferramenta; consultado pelo handler `cargo`.
- `mustard-core` é dependência de `mustard-rt` e `mustard-cli` — a Wave 1 fecha antes das Waves 2/3/4.

## Limites

- `packages/core/src/model/`
- `apps/rt/src/run/`
- `apps/cli/templates/` (`.artifacts.json` + `skills/hallmark/` + `skills/design-craft/SKILL.md`)
- `apps/cli/src/commands/init.rs` (apenas `ensure_rtk()`)
- `CLAUDE.md`, `apps/cli/templates/CLAUDE.md`, `apps/cli/CLAUDE.md`
- **Fora dos limites:** `apps/dashboard/` (UI = escopo b6); o fluxo `init`/`update` da CLI além de `ensure_rtk()`; instalações de usuário.

## Preocupações

- **`artifact-update --apply` é parcial** (Wave 2): `--apply` atualiza `version`/`vendored_at`/`checksum` no manifest mas NÃO faz clone+extração da árvore upstream para artefatos `git`/`skills-directory` — os arquivos vendorados ficam na revisão antiga enquanto o manifest declara a nova. Limitação documentada no doc-comment de `apply_vendored`. `--check` (a métrica de sucesso) é totalmente funcional. Follow-up: implementar o fetch real da árvore, ou emitir um aviso no JSON de `--apply` para fontes vendoradas.
- **Description do Hallmark longa** (Wave 3): ~830 chars, acima do típico de foundation skills (~100-200) e do warn do size-gate (>600). Risco de degradar o auto-load por descrição. Follow-up: enxugar para ≤300 chars, movendo as condições de disparo para o corpo do SKILL.md.
- **Skills existentes classificadas `manual`** (Wave 1): as 12 skills pré-existentes declaram `source: manual` no frontmatter e não carregam coordenadas de upstream — `artifact-update --check` só tem upstream real para `hallmark` e `rtk`. Follow-up: identificar e registrar os upstreams reais (mattpocock/skills, skill-creator) para reclassificar de `manual` → `git`.
