# Achatamento de spec/ + multi-colaborador

### Status: completed
### Phase: CLOSE
### Scope: full (wave plan)
### Checkpoint: 2026-05-21T00:00:00Z
### Lang: pt
### Total waves: 6

## PRD

## Contexto

Hoje o Mustard mantém o ciclo de vida de cada spec em dois lados — qual pasta está na árvore (`spec/active/`, `spec/completed/`, `spec/superseded/`) e qual o último evento `pipeline.status` no SQLite. Sempre que um dos lados muda sem o outro acompanhar (mv manual, archive sem emit, evento sem update de header) o painel vira espelho de uma realidade que não bate com o disco; nesta sessão sozinha apareceram seis fantasmas e um preso. A divisão também atrapalha colaboração: o SQLite local é privado por máquina, então outro desenvolvedor que dá pull vê o `spec.md` com `### Status: completed`, mas o painel dele mostra "sem eventos". O ciclo de vida precisa ter uma única fonte de verdade para cada eixo (qualificação ⇒ SQLite local, canon cross-dev ⇒ header da `spec.md` versionado em git, conteúdo ⇒ markdown versionado em git), e a árvore precisa parar de tentar codificar status pela pasta em que a spec mora.

## Usuários/Stakeholders

Quem usa o Mustard em projeto compartilhado: o desenvolvedor que abre uma spec localmente e quem dá pull no mesmo repo depois. Pedido nasceu nesta sessão da dor de ver fantasmas em "Ativas" e quatro itens em "Follow-up" sem pasta correspondente, e da pergunta direta sobre como o SQLite se comporta em time.

## Métrica de sucesso

Painel mostra exatamente o conjunto de specs presentes em `spec/{slug}/`, classificadas pelo SQLite local quando ele tem evento e pelo header da spec.md quando não tem — zero fantasmas, zero presos. Outro colaborador faz pull do repo, abre o painel e vê o estado correto da spec sem ter rodado nada localmente.

## Não-Objetivos

- Backend remoto / sincronização multi-máquina em tempo real (opção C da discussão). Fica fora.
- Mudar o formato do conteúdo de `spec.md` (estrutura PRD/Plano + wave-plan + sub-specs continua igual).
- Substituir o SQLite local por outro store. Continua sendo a fonte de telemetria e história rica local.
- Mover artefatos da CLI (`apps/cli/templates/`) que não citam buckets de spec.
- Migrar projetos de terceiros automaticamente — só este repo é convertido nesta passagem.

## Critérios de Aceitação

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: Build full do workspace passa — Command: `cargo build --workspace`
- [x] AC-2: Testes do core + rt + dashboard passam — Command: `cargo test -p mustard-core -p mustard-rt --bin mustard-rt`
- [x] AC-3: Dashboard tipa e builda — Command: `pnpm --filter mustard-dashboard build`
- [x] AC-4: Não existe mais `.claude/spec/active/`, `.claude/spec/completed/`, `.claude/spec/superseded/` no repo — Command: `node -e "const f=require('fs');const bad=['active','completed','superseded'].filter(b=>f.existsSync('.claude/spec/'+b));process.exit(bad.length===0?0:1)"`
- [x] AC-5: Toda spec em `spec/{name}/{spec.md|wave-plan.md}` tem status no header alinhado com o último `pipeline.status.to` do SQLite (excluindo equivalências lifecycle: draft/approved ↔ planning, closed ↔ completed, superseded ↔ cancelled) — Command: `mustard-rt run rebuild-specs && node -e "const {DatabaseSync}=require('node:sqlite');const fs=require('fs');const path=require('path');const db=new DatabaseSync('.claude/.harness/mustard.db');const EQ={draft:'planning',approved:'planning',closed:'completed',superseded:'cancelled'};const drift=[];for(const d of fs.readdirSync('.claude/spec')){const base=path.join('.claude/spec',d);const md=fs.existsSync(path.join(base,'spec.md'))?path.join(base,'spec.md'):fs.existsSync(path.join(base,'wave-plan.md'))?path.join(base,'wave-plan.md'):null;if(!md)continue;const m=fs.readFileSync(md,'utf8').match(/^###\\s*Status:\\s*(\\S+)/m);if(!m)continue;const header=m[1].toLowerCase();const row=db.prepare('SELECT status FROM specs WHERE name=?').get(d);if(!row)continue;const sqlite=row.status.toLowerCase();if(header===sqlite)continue;if((EQ[header]||header)===sqlite)continue;if(header==='closed-followup'&&sqlite==='closed-followup')continue;drift.push(d+' header='+header+' sqlite='+sqlite);}process.exit(drift.length===0?0:(console.error('drift:',drift),1))"`
- [x] AC-6: Dashboard mostra zero fantasmas (specs ativas/follow-up sem pasta — aceita spec.md OU wave-plan.md como evidência de "on disk") — Command: `node -e "const {DatabaseSync}=require('node:sqlite');const fs=require('fs');const db=new DatabaseSync('.claude/.harness/mustard.db');const onDisk=new Set(fs.readdirSync('.claude/spec').filter(d=>{const b='.claude/spec/'+d;return fs.statSync(b).isDirectory()&&(fs.existsSync(b+'/spec.md')||fs.existsSync(b+'/wave-plan.md'));}));const active=db.prepare(\"SELECT name FROM specs WHERE status IN ('planning','implementing','reviewing','qa','blocked','wave-failed','closed-followup')\").all().map(r=>r.name);const ghosts=active.filter(n=>!onDisk.has(n));process.exit(ghosts.length===0?0:(console.error('ghosts:',ghosts),1))"`

## Plano

## Informações da Entidade

`SpecStatus` (já existe em `packages/core/src/model/view/spec.rs`) — variant `Abandoned` adicionado anteriormente. Sem mudança de schema neste plano.
`HarnessEvent` — sem mudança; eventos `pipeline.status`, `pipeline.phase`, `pipeline.scope`, `pipeline.complete` continuam idênticos.
Diretório de specs — novo formato `spec/{name}/`; sem subbuckets.

## Arquivos

Distribuídos por wave (cada wave-N tem `## Arquivos` próprio com a lista exata). Resumo cross-wave:

```
packages/core/src/projection/card.rs    — wave 1: fallback header→status
packages/core/src/reader/sqlite.rs       — wave 1: passar caminho da spec ao fold
apps/rt/src/run/complete_spec.rs         — wave 2: remover archive/mv, manter emit
apps/rt/src/run/spec_extract.rs          — wave 2: resolve flat
apps/rt/src/run/qa_run.rs                — wave 2: resolve flat
apps/rt/src/run/wave_tree.rs             — wave 2: resolve flat
apps/rt/src/run/wikilink_extract.rs      — wave 2: resolve flat
apps/rt/src/run/emit_pipeline.rs         — wave 2: sync header da spec.md
apps/rt/src/hooks/session_cleanup.rs     — wave 2: remover mv-loop
apps/dashboard/src-tauri/src/spec_views.rs — wave 3: spec_action emit-only + resolve_spec_dir flat
apps/cli/templates/commands/mustard/*    — wave 4: SKILLs sem buckets
apps/cli/src/commands/init.rs            — wave 4: criar só spec/
apps/cli/src/commands/update.rs          — wave 4: idem
apps/cli/templates/pipeline-config.md    — wave 4
(MIGRATION)                              — wave 5: mover dirs + backfill events
.claude/CLAUDE.md, apps/*/CLAUDE.md      — wave 6
.claude/refs/feature/*, refs/close/*     — wave 6
.claude/.docs-audit.json                 — wave 6
```

## Tarefas

Wave-by-wave; detalhes vivem em cada `wave-N-{role}/spec.md`. Resumo da árvore de dependências:

```
wave-1 (core fallback)  ─┬─►  wave-2 (rt)         ─┐
                         └─►  wave-3 (dashboard)  ─┴─►  wave-4 (CLI + templates)  ─┬─►  wave-5 (migration)
                                                                                   └─►  wave-6 (docs)
```

## Limites

- `.claude/spec/active/`, `.claude/spec/completed/`, `.claude/spec/superseded/` — bucket dirs
- `packages/core/src/projection/card.rs`, `packages/core/src/reader/sqlite.rs`, `packages/core/src/model/view/spec.rs`
- `apps/rt/src/run/{complete_spec,spec_extract,qa_run,wave_tree,wikilink_extract,emit_pipeline}.rs`
- `apps/rt/src/hooks/session_cleanup.rs`
- `apps/dashboard/src-tauri/src/spec_views.rs`
- `apps/cli/src/commands/{init,update}.rs`
- `apps/cli/templates/commands/mustard/{close,resume,feature,bugfix,tactical-fix,qa,approve}/SKILL.md`
- `apps/cli/templates/pipeline-config.md`
- `.claude/CLAUDE.md`, `apps/*/CLAUDE.md`, `apps/cli/templates/CLAUDE.md`
- `.claude/refs/feature/*`, `.claude/refs/close/*`
- `.claude/.docs-audit.json`

Out-of-boundary explicit: backend remoto (não fazemos), formato de spec.md (não mexemos no PRD/Plano shape), repo de terceiros (não migra).

## Cobertura de Críticas

Cada item levantado nesta sessão e seu destino:

| Crítica do usuário | Bucket | Onde |
|---|---|---|
| "specs ativas mostrando como concluídas no painel" | Já corrigido em sessão (backfill + fix em mark_followup) — relevante apenas como sintoma motivador desta feature | Contexto |
| "_bugfix_test e órfãos no painel" | Já corrigido em sessão (`SpecStatus::Abandoned` + reclassificação) — sintoma motivador | Contexto |
| "Encerradas com phase execute parecia errado" | Já corrigido em sessão (`mark_followup` emite `pipeline.phase: CLOSE`) — sintoma motivador | Contexto |
| "Cancelado precisa ser separado de Concluído" | Já corrigido em sessão (abas dedicadas) — sintoma motivador | Contexto |
| "pasta vs SQLite divergente" — fantasmas/presos | Coberto | Waves 1, 2, 3, 5 |
| "elimina divisão de pastas, tudo em spec/{name}/" | Coberto | Waves 2, 3, 4, 5 |
| "header da spec.md como canon cross-dev" | Coberto | Wave 1 (fallback) + Wave 2 (emit sincroniza header) |
| "importar pro SQLite caso não exista evento" | Coberto | Wave 1 (auto-import no fold) + Wave 5 (backfill no repo atual) |
| "wikilinks e sub-specs continuam ligando" | Não-Goal (sem mudança) | Não-Objetivos |
| "sincronização multi-máquina em tempo real" | Não-Goal | Não-Objetivos |
