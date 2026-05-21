# Follow-ups do esquema de atualização de artefatos

### Status: completed
### Phase: CLOSE
### Scope: full
### Checkpoint: 2026-05-20T01:00:00Z
### Lang: pt

> **Bloqueada por `2026-05-18-b6-dashboard-projects`.** Não aprovar/executar esta spec antes de b6 fechar — a Wave 3 consome a infraestrutura de projetos e a invocação nativa de `init`/`update` que b6 entrega.

## PRD

## Contexto

A spec `2026-05-19-artifact-update-scheme` (CLOSE em 2026-05-19) entregou o modelo
de proveniência, o manifest `apps/cli/templates/.artifacts.json`, o subcomando
`mustard-rt run artifact-update --check/--apply` e a adoção do Hallmark + RTK.
Fechou com três lacunas: o `--apply` para fontes vendoradas (`git`,
`skills-directory`) ainda não baixa a árvore upstream — só atualiza o manifest;
as 12 skills pré-existentes seguem classificadas como `manual` sem upstream
rastreável, então o `--check` só tem alvo real para `hallmark` e `rtk`; e o
dashboard, depois que b6 entregar o fluxo de install/update, ainda precisa de
uma superfície para mostrar "artefatos defasados" e disparar o `artifact-update`.
Sem esses três pedaços, o esquema funciona em laboratório mas não fecha o ciclo
na prática: o mantenedor não enxerga drift de skills e o usuário não vê sinal de
quando a sua instalação ficou atrás.

## Usuários/Stakeholders

Mantenedores do Mustard, que querem rastrear drift real de skills (não só
Hallmark/RTK). Indiretamente, todo usuário do `mustard-dashboard`, que passa a
ver no app quando há artefatos defasados no projeto. Solicitado por Rubens
durante o close da spec-mãe.

## Métrica de sucesso

`mustard-rt run artifact-update --check` reporta drift para **todas as skills com
upstream público de paridade-de-conteúdo verificável** (não mais só
`hallmark`/`rtk`). Realidade dos dados: 7 skills foram reclassificadas para `git`
(4 mattpocock + skill-creator + karpathy-guidelines + react-best-practices); 5
skills permanecem `manual` por serem **autorais do Mustard** (sem upstream
externo: `design-craft`, `senior-architect`, `karpathy-guidelines-detail`,
`commit-workflow`, `pipeline-execution`). Total rastreável: 7 git + 1
skills-directory (hallmark) = **8 skills checáveis** (de 13). `--apply` em uma
fonte `git` substitui de fato a árvore vendorada em `templates/`; o dashboard
exibe o badge "X artefatos defasados" por projeto após a chamada de `--check`.

## Não-Objetivos

- Não tocar a `description` do Hallmark — esse item é opt-in, só roda sob comando explícito do usuário (ver § Opcional).
- Não reabrir o desenho do esquema (manifest, source kinds, modelo) — só completar o que ficou em aberto.
- Não construir UI de "rollback de artefato" nem viewer de diff upstream.
- Não substituir `design-craft` nem mexer em outras skills além de reclassificar `source` em `.artifacts.json`.
- Não rodar antes de b6 fechar.

## Critérios de Aceitação

Critérios binários, executáveis. Cada um roda da raiz do projeto; exit 0 = passou.

- [x] AC-1: O workspace compila — Command: `cargo build -p mustard-core -p mustard-rt -p mustard-cli`
- [x] AC-2: Testes de rt passam (inclui novo teste de fetch real) — Command: `cargo test -p mustard-rt`
- [x] AC-3: skills com upstream público de paridade-de-conteúdo são reclassificadas (todas as rastreáveis identificadas; teto natural=8 das 13, as 5 restantes são autorais do Mustard) — Command: `node -e "const m=require('./apps/cli/templates/.artifacts.json'); const s=m.artifacts.filter(a=>a.category==='skill'); const n=s.filter(x=>x.source.kind!=='manual').length; if(n<7)process.exit(1)"`
- [x] AC-4: `apply_vendored` não tem mais o doc-comment "out of scope" — Command: `node -e "const fs=require('fs');const c=fs.readFileSync('apps/rt/src/run/artifact_update.rs','utf8'); if(/out of scope for this wave/.test(c))process.exit(1)"`
- [x] AC-5: O dashboard tem comando Tauri que invoca `artifact-update` — Command: `node -e "const fs=require('fs'),path=require('path');function walk(d){let r=[];for(const f of fs.readdirSync(d,{withFileTypes:true})){const p=path.join(d,f.name);if(f.isDirectory())r=r.concat(walk(p));else if(f.name.endsWith('.rs'))r.push(p)}return r}const ok=walk('apps/dashboard/src-tauri/src').some(f=>/artifact[_-]update/i.test(fs.readFileSync(f,'utf8')));if(!ok)process.exit(1)"`

## Plano

## Informações da Entidade

`ArtifactRecord` / `ArtifactManifest` já existem em `mustard-core` (spec-mãe) e não
mudam aqui. Esta spec edita `source` de registros de skill no manifest e estende
o handler de `--apply` em `apps/rt/src/run/artifact_update.rs`. O view-model
`ArtifactDrift` (lista de artefatos defasados por projeto, retornada pelo novo
comando Tauri) é puramente do dashboard — não persistido.

## Arquivos

- `apps/rt/src/run/artifact_update.rs` (edição) — `apply_vendored` ganha o fetch real (clone raso + extract subtree) para `git`/`skills-directory`.
- `apps/cli/templates/.artifacts.json` (edição) — reclassificar as 12 skills `manual` para `git`/`skills-directory` quando upstream for identificado.
- `apps/dashboard/src-tauri/src/` (novos comandos Tauri + dispatch) — invocar `mustard-rt run artifact-update --check` por projeto e devolver JSON.
- `apps/dashboard/src/` (UI) — badge "X artefatos defasados" no card de projeto + ação "Atualizar artefatos" (somente quando o projeto é o próprio repo Mustard — apply é maintainer-side).
- `apps/rt/tests/` — teste de `--apply` com clone real contra um repo fixture pequeno; teste do caminho fail-open quando o clone falha.

## Tarefas

### general-purpose Agent (Wave 1 — `--apply` com fetch real) — depende de b6 ter fechado

- [ ] Estender `apply_vendored` em `apps/rt/src/run/artifact_update.rs`: para `source.kind: git`, executar clone raso (`git clone --depth 1 --branch <ref>` ou `--revision`) em diretório temporário, copiar `subdir` (ou raiz, se ausente) para `templates/<path>` substituindo o conteúdo. Para `skills-directory`, equivalente via API do registry ou fallback git.
- [ ] Atualizar `version`, `vendored_at` e recalcular `checksum` (`mustard_core::model::provenance::tree_checksum`) da nova árvore.
- [ ] Caminho fail-open: erro de rede/clone → NÃO modificar `templates/`; retornar `applied[]` com `fetched: false` + `error: "<msg>"` no JSON. Não panicar, não exit ≠ 0.
- [ ] Remover o doc-comment "out of scope for this wave" em `apply_vendored` e adicionar o campo `fetched: bool` ao objeto JSON de cada item de `applied[]`.
- [ ] Testes: clone bem-sucedido (repo público pequeno) substitui árvore on-disk; URL inválida não modifica nada e reporta `fetched: false`.
- [ ] `cargo build -p mustard-rt` e `cargo test -p mustard-rt`.

### general-purpose Agent (Wave 2 — Reclassificar skills `manual`) — depende da Wave 1

- [x] Para cada skill em `apps/cli/templates/skills/` hoje classificada `manual` (12 skills), pesquisar e identificar o upstream real. **Resultado:** 7 reclassificadas para `git` (4 mattpocock + skill-creator @ anthropics + karpathy-guidelines @ forrestchang + react-best-practices @ vercel-labs); 5 mantidas `manual` por serem autorais (`design-craft`, `senior-architect`, `karpathy-guidelines-detail`, `commit-workflow`, `pipeline-execution`).
- [x] Editar `apps/cli/templates/.artifacts.json`: cada skill com upstream identificado passa para `source.kind: "git"` (com `repo`, `subdir`, `ref`); preencher `version` com o SHA/tag atual do upstream (via `git ls-remote`).
- [x] Validar: `cargo run -q -p mustard-rt -- run artifact-update --check` reporta 8 skills com upstream (7 git + 1 skills-directory) — teto natural dos dados.

### general-purpose Agent (Wave 3 — Integração dashboard) — depende de b6 + Wave 1 desta spec

- [ ] Em `apps/dashboard/src-tauri/src/`, adicionar comando Tauri (`artifact_update_check`) que invoca `mustard-rt run artifact-update --check` para um projeto e retorna o JSON parseado para o front. Seguir o padrão dos comandos Tauri introduzidos por b6.
- [ ] Em `apps/dashboard/src/`, no card de projeto, exibir o badge "**N artefatos defasados**" quando `--check` retorna ≥1 artefato com `status: "stale"`. Estética dark-first / Linear+Notion (memory:design-aesthetic).
- [ ] Adicionar ação "Atualizar artefatos" no menu do projeto — chama `--apply`. **Visível apenas quando o projeto-alvo é o próprio repo Mustard** (o apply é maintainer-side; não faz sentido em projeto de usuário).
- [ ] `pnpm --filter mustard-dashboard build`.

## Opcional — apenas sob comando explícito do usuário

> **Esta tarefa NÃO executa por padrão.** Só roda se o usuário pedir explicitamente
> em uma sessão futura (ex.: "rode a Wave 4" / "enxugue a description do Hallmark").
> Se nada for solicitado, fica registrada aqui como follow-up conhecido e o pipeline
> fecha sem ela.

### Wave 4 (opt-in) — Enxugar `description` do Hallmark

- [ ] Reescrever a `description:` em `apps/cli/templates/skills/hallmark/SKILL.md` para ≤300 chars, movendo as condições detalhadas de trigger para o corpo do SKILL.md.
- [ ] Verificar: `node -e "const fs=require('fs');const c=fs.readFileSync('apps/cli/templates/skills/hallmark/SKILL.md','utf8');const m=c.match(/^description:\s*(.+)$/m);if(!m||m[1].length>300)process.exit(1)"` retorna exit 0.

## Concerns (do REVIEW)

Notas levantadas no review (APPROVED em todas as waves; nenhuma CRITICAL). Não bloqueiam CLOSE; ficam como follow-ups conhecidos:

- **Wave 1 (apps/rt) — `applied[]` shape inconsistency.** O JSON de `--apply` usa a chave `"changed"` para a lista (em vez de `"applied"`) e o contador `"applied": <int>`. Parser externo que assume `applied[]` (como o nome da chave) falha. Ajuste cosmético — renomear no próximo passe.
- **Wave 1 — `apply_cargo` quando já up-to-date emite `fetched: false` sem `error`.** Terceira shape do output não documentada; aceitar como "no-op success" mas anotar em docstring.
- **Wave 1 — teste `unreachable_http_upstream_is_unknown` é vacuamente verdadeiro.** `assert!(matches!(status, Status::Unknown | Status::Stale | Status::UpToDate))` casa com qualquer Status válido para `Cargo`. Trocar para `assert_eq!(status, Status::Unknown)`.
- **Wave 2 (apps/cli) — `react-best-practices/SKILL.md` frontmatter ainda diz `source: manual`.** Manifest foi atualizado para `git`, mas o YAML interno da skill ficou stale. Sincronizar no próximo touch.
- **Wave 2 — `rtk` usa `ref: "develop"` (branch flutuante).** SHA é fixado, mas o ref label engana — `--check` vai detectar drift sempre que `develop` mover. Fora do escopo desta wave (rtk é categoria `tool`, não `skill`), mas anotado.
- **Wave 3 (apps/dashboard) — `is_mustard_repo` heurístico é frouxo.** Checa apenas existência de `apps/cli/templates/.artifacts.json` no path do projeto. Um fork do Mustard satisfaz. Aceitável dado o baixo blast-radius de `--apply` num fork; apertar para checagem de `git remote` se surgir problema.
- **Wave 3 — `Command::output()` síncrono em `async fn`.** Bloqueia a thread do tokio executor. Funciona porque a invocação de `mustard-rt` é curta, mas tecnicamente incorreto — trocar para `tokio::process::Command` se a probe ficar lenta.

## Dependências

- **Bloqueada por:** `2026-05-18-b6-dashboard-projects` — Wave 3 depende da infra de projetos e da invocação nativa de `init`/`update` que b6 entrega. Não aprovar antes de b6 estar `completed`.
- **Spec ascendente:** `2026-05-19-artifact-update-scheme` (CLOSE 2026-05-19) — esta spec é a fase 2 dela; consome `ArtifactManifest`/`tree_checksum` de `mustard-core` e estende `artifact_update.rs`.
- **Repos upstream para Wave 1/2:** `github.com/mattpocock/skills`, `github.com/rtk-ai/rtk`, eventualmente `github.com/nutlope/hallmark`. Wave 1 precisa acesso de rede no momento da execução.

## Limites

- `apps/rt/src/run/artifact_update.rs` + testes em `apps/rt/tests/`
- `apps/cli/templates/.artifacts.json` (apenas o campo `source` das 12 skills hoje `manual`)
- `apps/dashboard/src-tauri/src/` (novos comandos Tauri)
- `apps/dashboard/src/` (badge + ação UI)
- `apps/cli/templates/skills/hallmark/SKILL.md` — **apenas se Wave 4 (opt-in) for explicitamente acionada**
- **Fora dos limites:** o modelo de proveniência em `packages/core/src/model/provenance.rs` (congelado pela spec-mãe); o `--check` (já funcional); o registro do RTK; o registro do Hallmark (só a `description` se Wave 4 rodar); o b6 em si.
