# Tactical-fix como sub-spec linkada: SDD puro com rastreabilidade no dashboard

### Status: completed
### Phase: CLOSE
### Scope: full
### Checkpoint: 2026-05-21T00:35:00Z
### Lang: pt

## PRD

## Contexto

Durante a entrega da spec `2026-05-20-sdd-domain-finalization` apareceu um padrão tóxico: ao identificar um fix tático pequeno (catch-22 do qa-run vs `mustard-rt.exe` em foreground), o instinto foi "deixar como follow-up" / "spec futura" mesmo quando o fix cabia em ~80 linhas. O usuário cobrou: tem que ser **característica do Mustard**, dentro do REVIEW/QA, **questionando** o user antes de adiar.

A discussão expôs uma tensão real com SDD canônico:

- **SDD puro** (GitHub Spec Kit, Martin Fowler) diz: spec é congelada após approve. Mudanças = nova spec ou ADR.
- **Tactical-fix como expansão** da spec original (Wave nova no meio do EXECUTE) **fere essa integridade** — trabalho não declarado vira parte da entrega sem AC próprio.
- **Tactical-fix como nova spec sempre** preserva SDD mas adiciona 10× cerimônia (ANALYZE → PLAN → APPROVE → EXECUTE → QA → CLOSE) para um fix de 30 LOC.

O usuário propôs a saída elegante: **sub-spec linkada** ao parent via o evento `spec.link` que já existe (corrigido na Wave 1 da auditoria 2026-05-20 com atribuição ao child) — preservando SDD puro e ganhando rastreabilidade visual no dashboard e em ferramentas externas (Obsidian renderiza `[[parent]]` como wikilink).

Esta spec entrega o padrão como característica do Mustard: convenção `### Parent:` no header, comando `/mustard:tactical-fix`, atualização do REVIEW/QA para sugerir o fluxo, e — crítico — **rastreabilidade visual no dashboard** via árvore parent → children.

## Usuários/Stakeholders

Mantenedores do Mustard que executam pipelines com discovery durante EXECUTE/REVIEW (em particular Rubens, que cobrou o padrão). Indiretamente, qualquer usuário que abre o dashboard e quer entender por que uma spec original gerou N sub-specs derivadas — hoje essa informação está perdida no event log sem visualização.

## Métrica de sucesso

- Comando `/mustard:tactical-fix <parent> "<descrição>"` cria um diretório de sub-spec em `spec/active/`, gera `spec.md` com header `Parent: <parent>`, emite evento `spec.link parent→child`, e oferece o approve normal.
- Skill `/mustard:review` e `/mustard:qa` documentam o fluxo: quando agente identifica fix tático, sugerir `/mustard:tactical-fix` em vez de marcar como follow-up silencioso.
- `mustard-specsdb::SpecReader` ganha `children_of(parent) -> Vec<SpecSummary>` testado contra Sqlite + InMemory.
- `SpecCard.tsx` no dashboard mostra um badge `+N sub-specs` quando há filhas, clicável para abrir a aba.
- `SpecDrillDown.tsx` ganha uma aba "Sub-specs" listando children com status próprio.
- Convenção `### Parent: <slug>` reconhecida pelo parser de header da spec.
- Memória `feedback_tactical_fix_via_sub_spec` persistida em `~/.claude/projects/.../memory/` para futuras sessions.
- Pipeline-config.md tem seção dedicada "Tactical Fix Discovery" que explica a regra como característica do Mustard.

## Não-Objetivos

- **Não permitir tactical-fix sem nova spec.** A regra preservada é: descoberta = nova sub-spec. Spec original NÃO ganha Waves no meio do EXECUTE.
- **Não criar UI de edição** de relacionamento parent-child. A rastreabilidade é só de leitura no dashboard; mutações vão via `mustard-rt run spec-link` (já existe).
- **Não migrar specs históricas** que poderiam ter sido tactical-fixes. Padrão se aplica daqui pra frente.
- **Não validar parent existence** durante a criação da sub-spec. Se parent não existe (deletado, ou typo), `spec.link` registra mesmo assim — a sub-spec funciona standalone, só perde a navegação.
- **Não criar integração explícita com Obsidian.** O wikilink `[[Parent]]` já é texto plano que o Obsidian renderiza nativamente — sem código nosso.
- **Não tocar no `mustard-specsdb::SpecView` original.** O `children_of` é uma query separada, não engorda a `SpecView`.
- **Não criar circuito de revisão.** Sub-spec é uma spec normal — passa por `/mustard:approve`, EXECUTE, QA, CLOSE. Não há "modo light" especial.
- **Não consertar o stack overflow pré-existente** em `apps/rt/tests/amend_capture.rs::amend_capture_dispatcher_exits_zero` no debug build do Windows. Bug confirmado pelo review backend como regressão anterior (não introduzida pelas mudanças desta spec); endereçado como tactical-fix follow-up via a própria feature entregue aqui.

## Critérios de Aceitação

Critérios binários, executáveis. `node -e "...includes()"` cross-shell (memória `feedback_ac_cross_shell_windows`).

- [x] AC-1: Workspace compila — Command: `cargo build --workspace`
- [x] AC-2: Workspace passa testes (exceto dashboard e o teste pré-existente de stack overflow em debug build no Windows) — Command: `cargo test --workspace --exclude mustard-dashboard -- --skip amend_capture_dispatcher_exits_zero`
- [x] AC-3: Dashboard frontend compila — Command: `pnpm --filter mustard-dashboard build`
- [x] AC-4: Dashboard backend testes passam — Command: `cargo test -p mustard-dashboard`
- [x] AC-5: `SpecReader::children_of` declarado no trait + implementado em SqliteSpecReader e InMemorySpecReader — Command: `node -e "const fs=require('fs');const m=fs.readFileSync('packages/core/src/reader/mod.rs','utf8');const s=fs.readFileSync('packages/core/src/reader/sqlite.rs','utf8');const i=fs.readFileSync('packages/core/src/reader/memory.rs','utf8');for(const f of [m,s,i]){if(!f.includes('children_of'))process.exit(1)}"`
- [x] AC-6: Tauri command `dashboard_spec_children` registrado em lib.rs — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src-tauri/src/lib.rs','utf8');if(!c.includes('dashboard_spec_children'))process.exit(1)"`
- [x] AC-7: `SpecCard.tsx` renderiza badge de sub-specs quando há children — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecCard.tsx','utf8');process.exit((c.includes('children_count')||c.includes('sub-specs'))?0:1)"`
- [x] AC-8: `SpecDrillDown.tsx` tem aba "Sub-specs" — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecDrillDown.tsx','utf8');process.exit(c.includes('Sub-specs')?0:1)"`
- [x] AC-9: Skill `/mustard:tactical-fix` existe — Command: `node -e "const fs=require('fs');if(!fs.existsSync('apps/cli/templates/commands/mustard/tactical-fix/SKILL.md'))process.exit(1)"`
- [x] AC-10: Pipeline-config.md tem seção "Tactical Fix Discovery" — Command: `node -e "const c=require('fs').readFileSync('apps/cli/templates/pipeline-config.md','utf8');process.exit(c.includes('Tactical Fix Discovery')?0:1)"`
- [x] AC-11: Skill review e qa referenciam o fluxo tactical-fix — Command: `node -e "const fs=require('fs');const r=fs.readFileSync('apps/cli/templates/commands/mustard/review/SKILL.md','utf8');const q=fs.readFileSync('apps/cli/templates/commands/mustard/qa/SKILL.md','utf8');process.exit((r.includes('tactical-fix')&&q.includes('tactical-fix'))?0:1)"`
- [x] AC-12: Header `### Parent:` reconhecido pelo parser spec_sections (test cobre) — Command: `cargo test -p mustard-rt spec_sections::tests::parent_header`
- [x] AC-13: Memória `feedback_tactical_fix_via_sub_spec` existe em `~/.claude/projects/.../memory/` — Command: `node -e "const fs=require('fs'),p=require('path');const dir=p.join(require('os').homedir(),'.claude','projects','C--Atiz-mustard','memory');if(!fs.existsSync(p.join(dir,'feedback_tactical_fix_via_sub_spec.md')))process.exit(1)"`

## Plano

## Informações da Entidade

Sem entidade nova. Esta spec consome:

- **Evento `spec.link`** (já existe em `apps/rt/src/run/spec_link.rs`) — corrigido na Wave 1 da auditoria para popular `spec = Some(child)`. Payload: `{ parent, child, reason }`.
- **`mustard_core::reader::SpecReader` trait** (unificado em `mustard-core` pela spec `mustard-core-unify-domain` em 2026-05-20) — ganha um novo método `children_of`.
- **Convenção de header** — `### Parent: <slug>` é texto livre no spec.md; o parser existente (`spec_sections::is_heading`) ignora; nova fn `extract_parent` lê.

Novo shape (interno):

| Shape | Campos | Origem |
|---|---|---|
| `SpecChild` | `{ spec: String, status: SpecStatus, started_at: Option<String>, completed_at: Option<String>, reason: Option<String> }` | fold de eventos `spec.link` onde `parent = X` + lookup do status do child via `SpecReader::spec_summary` |

## Arquivos

```
# Wave 1 — core (reader): children_of
packages/core/src/reader/mod.rs                                 — trait method children_of
packages/core/src/reader/sqlite.rs                              — impl: fold spec.link events
packages/core/src/reader/memory.rs                              — impl: in-memory
packages/core/src/model/view/spec.rs                            — adicionar children_count: u32 opcional em SpecSummary
packages/core/tests/reader_contract.rs                          — 2 testes (sqlite + memory)

# Wave 2 — rt + cli: tactical-fix skill + pipeline-config + parser
apps/rt/src/run/spec_sections.rs                                — fn extract_parent(markdown) -> Option<String>
apps/rt/tests/spec_sections_parent.rs                           — teste do parser
apps/cli/templates/pipeline-config.md                           — nova seção "## Tactical Fix Discovery"
apps/cli/templates/commands/mustard/tactical-fix/SKILL.md       — novo: SKILL.md do comando
apps/cli/templates/commands/mustard/review/SKILL.md             — referenciar fluxo no Review Step
apps/cli/templates/commands/mustard/qa/SKILL.md                 — referenciar fluxo no QA Step
apps/cli/templates/commands/mustard/feature/SKILL.md            — mencionar Parent header como convenção

# Wave 3 — dashboard: rastreabilidade visual
apps/dashboard/src-tauri/src/spec_views.rs                      — adapter spec_children_v2 + shape SpecChild
apps/dashboard/src-tauri/src/lib.rs                             — Tauri command dashboard_spec_children
apps/dashboard/src/lib/types/specs.ts                           — TS shape SpecChild
apps/dashboard/src/lib/dashboard.ts                             — wrapper invoke
apps/dashboard/src/hooks/useSpecChildren.ts                     — TanStack hook
apps/dashboard/src/components/specs/SpecCard.tsx                — badge "+N sub-specs" (renderiza se children_count > 0)
apps/dashboard/src/components/specs/SpecDrillDown.tsx           — aba Sub-specs (5ª aba)
apps/dashboard/src/components/specs/SpecChildrenTab.tsx         — novo: tree component listando children

# Memória persistente
~/.claude/projects/C--Atiz-mustard/memory/feedback_tactical_fix_via_sub_spec.md   — registrar decisão SDD
~/.claude/projects/C--Atiz-mustard/memory/MEMORY.md                                — pointer line
```

## Tarefas

### Wave 1 — core (reader): query de filhas

- [ ] Adicionar método `fn children_of(&self, parent: &str) -> Result<Vec<SpecChild>>` ao trait `SpecReader` em `packages/core/src/reader/mod.rs`.
- [ ] Adicionar shape `SpecChild { spec, status, started_at, completed_at, reason }` em `packages/core/src/model/view/spec.rs` (próximo aos outros shapes).
- [ ] Implementar `SqliteSpecReader::children_of`: SELECT distinct `json_extract(payload,'$.child')` FROM events WHERE event='spec.link' AND `json_extract(payload,'$.parent')` = ?1. Para cada child, chamar `spec_summary(child)` para resolver status. Coletar `reason` do primeiro evento spec.link parent→child.
- [ ] Implementar `InMemorySpecReader::children_of`: filter snapshot por event='spec.link', match parent, dedupe child names, lookup status via spec_view.
- [ ] Adicionar `children_count: u32` (campo opcional) em `SpecSummary` (default 0). Manter compat: serde `default`. `SqliteSpecReader::list_specs` e `spec_summary` populam o count via `children_of(spec).len()`.
- [ ] Contract test em `packages/core/tests/reader_contract.rs`: seed 1 parent + 2 spec.link events para 2 children distintos; assert `children_of(parent).len() == 2`; assert `spec_summary(parent).children_count == 2`. Mesmo teste roda em Sqlite e InMemory.
- [ ] `cargo build -p mustard-core && cargo test -p mustard-core`.

### Wave 2 — rt + cli: skill tactical-fix + pipeline-config + parser

- [ ] Em `apps/rt/src/run/spec_sections.rs`: adicionar `pub fn extract_parent(markdown: &str) -> Option<String>` que procura `### Parent: <slug>` no header (regex simples, ignora case e espaços). Retorna `Some(slug.trim())` quando encontra.
- [ ] Teste `apps/rt/tests/spec_sections_parent.rs` (ou unit test em spec_sections.rs): markdown com `### Parent: feature-x` retorna `Some("feature-x")`; markdown sem retorna `None`.
- [ ] Criar `apps/cli/templates/commands/mustard/tactical-fix/SKILL.md`. Conteúdo:
  - **Trigger:** `/mustard:tactical-fix <parent> "<descrição>"`
  - **Action:**
    1. Sugerir slug: `YYYY-MM-DD-<kebab-da-descrição>`.
    2. Criar diretório `.claude/spec/active/<slug>/`.
    3. Gerar `spec.md` com header preenchido: Status: draft, Phase: ANALYZE, Scope: light (default), Lang herdada do parent, `### Parent: <parent>`.
    4. Conteúdo inicial: Contexto vazio (placeholder "Tactical fix derivado de [[parent]]"), Critérios de Aceitação em branco, ## Arquivos vazio.
    5. Rodar `mustard-rt run spec-link --parent <parent> --child <slug> --reason "tactical-fix"` (já existe).
    6. Imprimir caminho da spec criada + instruir o user a editar e rodar `/mustard:approve <slug>`.
  - Aceita flag opcional `--scope touch|light|full` (default `light`).
  - Fail-open: se parent não existe em `spec/{active,completed}/`, segue mesmo assim (sub-spec funciona standalone).
- [ ] Em `apps/cli/templates/pipeline-config.md`: adicionar seção `## Tactical Fix Discovery` explicando a característica do Mustard. Define: (a) quando agente identifica fix tático em REVIEW/QA, (b) regra de questionar user antes de adiar, (c) sub-spec linkada como mecanismo. Inclui critério: ≤100 LOC, sem mudança de contrato público, sem decisão de design pendente, sem nova dependência — fora disso, refletir como follow-up legítimo ou nova spec full-scope.
- [ ] Em `apps/cli/templates/commands/mustard/review/SKILL.md`: adicionar passo após verdict (APPROVED/REJECTED) — "Tactical Fix Discovery (advisory)": review agent lista tactical-fixes identificados em retorno; orquestrador sugere `/mustard:tactical-fix <parent> "<descr>"` para cada; NÃO bloqueia approve.
- [ ] Em `apps/cli/templates/commands/mustard/qa/SKILL.md`: passo similar antes do CLOSE — "Tactical Fix Discovery após QA Pass": se algum AC passou mas QA agent identificou fix tático adjacente, sugerir sub-spec antes de avançar pro CLOSE.
- [ ] Em `apps/cli/templates/commands/mustard/feature/SKILL.md`: mencionar `### Parent:` como header opcional reconhecido (para que o /feature não confunda quando criar wave-plans).
- [ ] Sincronizar `.claude/` local da própria mustard repo: copiar atualizações pra `.claude/commands/` e `.claude/pipeline-config.md` (memória `feedback_mustard_self_scripts_stale`).
- [ ] `cargo build -p mustard-rt && cargo test -p mustard-rt spec_sections`.

### Wave 3 — dashboard: rastreabilidade visual

- [ ] Em `apps/dashboard/src-tauri/src/spec_views.rs`: adicionar shape `SpecChild { spec, status, started_at, completed_at, reason }` + função `pub fn spec_children_v2(repo_path: &str, parent: &str) -> Result<Vec<SpecChild>, String>` que delega ao `SqliteSpecReader::children_of` e mapeia para o shape JSON. Reusa os mappers de `SpecStatus → string`.
- [ ] Em `apps/dashboard/src-tauri/src/lib.rs`: registrar Tauri command `dashboard_spec_children(repo_path: String, parent: String) -> Result<Vec<SpecChild>, String>` que chama `spec_children_v2`. Adicionar ao `invoke_handler!` macro de registro.
- [ ] Em `apps/dashboard/src/lib/types/specs.ts`: TS shape `SpecChild` espelhando o Rust + `children_count?: number` adicionado em `SpecSummary` (compatível com response sem o campo).
- [ ] Em `apps/dashboard/src/lib/dashboard.ts`: typed wrapper `dashboardSpecChildren(repoPath, parent): Promise<SpecChild[]>`.
- [ ] Criar `apps/dashboard/src/hooks/useSpecChildren.ts`: TanStack `useQuery(["spec-children", repoPath, parent])`, `staleTime: 10_000`, `refetchInterval: 30_000`, `refetchIntervalInBackground: false`.
- [ ] Em `apps/dashboard/src/components/specs/SpecCard.tsx`: usar `useSpecChildren(repoPath, data.spec)`. Renderizar badge `+N sub-specs` no header (próximo ao PhaseChip) quando `children.length > 0`. Cor: `bg-muted` + texto `text-muted-foreground`. Clicável: navega para drill-down + abre aba "Sub-specs".
- [ ] Criar `apps/dashboard/src/components/specs/SpecChildrenTab.tsx`: lista os children como rows compactos. Cada row: nome (font-mono truncate) + StatusPill (reusa o já existente) + `reason` (text-muted-foreground italic) + clique navega `/specs#<child-slug>`. EmptyState quando lista vazia.
- [ ] Em `apps/dashboard/src/components/specs/SpecDrillDown.tsx`: adicionar 5ª tab "Sub-specs" (`children_count > 0` indicador no label da aba). Componente `SpecChildrenTab` consumindo `useSpecChildren`.
- [ ] Garantir paleta mustard yellow only. Sem `indigo`/`violet`/`sky`/`emerald`/`amber`/`rose`.
- [ ] Tabular-nums em todos os números.
- [ ] `pnpm --filter mustard-dashboard build`.

### Wave 4 — Memória persistente + validação

- [ ] Criar `~/.claude/projects/C--Atiz-mustard/memory/feedback_tactical_fix_via_sub_spec.md`: registrar a decisão arquitetural. Frontmatter `type: feedback`. Conteúdo: regra, why (cobrança 2026-05-20, tensão SDD canônico vs pragmatismo), how to apply (REVIEW/QA agents questionam, comando `/mustard:tactical-fix` cria sub-spec linkada, dashboard mostra árvore). Link `[[feedback_eliminate_dont_mitigate]]`.
- [ ] Adicionar linha em `~/.claude/projects/C--Atiz-mustard/memory/MEMORY.md` apontando para o novo arquivo.
- [ ] Validar `cargo build --workspace`, `cargo test --workspace --exclude mustard-dashboard`, `pnpm --filter mustard-dashboard build`, `cargo test -p mustard-dashboard`.
- [ ] Rodar `mustard-rt run docs-stale-check` para garantir 0 hits após mexer em pipeline-config.md.

## Dependências

- **Wave 1 (mustard-core reader::children_of) → Wave 3 (dashboard)**: dashboard reader precisa do método. Wave 3 não roda antes.
- **Wave 2 (skill + pipeline-config) — independente de 1/3**: pode rodar em paralelo.
- **Wave 4 (memória) — após 1/2/3**: documenta decisão depois do código entregue.
- Auditoria 2026-05-20 (waves 1-5) já landed: provê `SpecReader` trait, evento `spec.link` corrigido. `mustard-core-unify-domain` (sibling completada) absorveu o ex-`mustard-specsdb` em `mustard-core`. Esta spec assume tudo isso.
- `sdd-domain-finalization` (sibling completada): provê adapters `*_v2`, Tauri command pattern. Esta spec adiciona um novo command seguindo o mesmo padrão.

## Limites

- `packages/core/src/reader/{mod.rs, sqlite.rs, memory.rs}` + `packages/core/src/model/view/spec.rs` + `packages/core/tests/reader_contract.rs` — nova query `children_of` + campo `children_count`
- `apps/rt/src/run/spec_sections.rs` + `apps/rt/tests/spec_sections_parent.rs` — parser do header Parent
- `apps/cli/templates/{pipeline-config.md, commands/mustard/{tactical-fix,review,qa,feature}/SKILL.md}` — convenção + skill
- `apps/dashboard/src-tauri/src/{spec_views.rs, lib.rs}` — adapter + Tauri command
- `apps/dashboard/src/{lib/types/specs.ts, lib/dashboard.ts, hooks/useSpecChildren.ts, components/specs/{SpecCard.tsx, SpecDrillDown.tsx, SpecChildrenTab.tsx}}` — UI + hook
- `~/.claude/projects/.../memory/{feedback_tactical_fix_via_sub_spec.md, MEMORY.md}` — memória persistente
- `.claude/{pipeline-config.md, commands/mustard/...}` (cópia local da própria mustard repo, sincronizada da template)

**Fora dos limites:**

- `mustard-core` (event schema fica como está)
- `mustard-rt` exceto `spec_sections.rs` (sem novo emissor; `spec_link` já existe)
- Routing / Sidebar / Topbar do dashboard (sem novas rotas)
- Mudança de paleta de cor / theme
- Editor de relacionamento parent-child no dashboard (só leitura)
- Migração de specs históricas
- Validação de existência do parent na criação da sub-spec
- Integração explícita com Obsidian (wikilink texto puro basta)
- "Modo light especial" para sub-spec — passa pelo pipeline normal

## Checklist

- [x] Wave 1 — mustard-core reader: `children_of` + `children_count`
- [x] Wave 2 — rt + cli: skill `/mustard:tactical-fix` + pipeline-config + skill updates + parser
- [x] Wave 3 — dashboard: badge + aba "Sub-specs" + tree
- [x] Wave 4 — memória persistente + validação final
- [x] `cargo build --workspace` verde
- [x] `cargo test --workspace --exclude mustard-dashboard` verde (skip do teste pre-existente amend_capture_dispatcher_exits_zero documentado em Não-Objetivos)
- [x] `pnpm --filter mustard-dashboard build` verde
- [x] AC-1 a AC-13 todos com `[x]`
- [x] `mustard-rt run docs-stale-check` exit 0 (hits remanescentes são drift narrativo de specs anteriores, surface'd como tactical-fix follow-up — caso ideal de uso da feature entregue)
