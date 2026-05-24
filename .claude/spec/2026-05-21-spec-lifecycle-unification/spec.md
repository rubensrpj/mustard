# Spec lifecycle unification + Linear-style /specs redesign

### Wave-plan: true

## Resumo

Unifica o ciclo de vida de uma spec em **uma única dimensão observável** (`Stage + Outcome + Flags`) eliminando a sobreposição atual entre `Status` (11 valores) e `Phase` (5 valores em uma vista, 7 na outra). Reformula a rota `/specs` do dashboard para uma lista densa estilo Linear, com agrupamento por Stage, bullet de progresso e árvore expansível de children (waves + AC + sub-specs). Adiciona um hook de higiene que detecta e auto-fecha órfãs com gate de segurança. Inclui camada de observação no Workspace ("Saúde do workspace" + badges inline) e migração criteriosa das specs já existentes.

A página de detalhe (`/spec/{name}` drill-down) **permanece intocada** — apenas a lista e o modelo de estado mudam.

## Motivação (contexto da conversa)

Durante a sessão 2026-05-21 ficou evidente que:
- O dashboard renderizava chips contraditórios ("PLANEJAMENTO" + ícone "Executar" aceso) na mesma spec porque **duas dimensões** (Status do header e Phase do header) eram lidas por componentes diferentes.
- A spec `2026-05-21-tf-skill-mirror` ficou órfã com `Status: approved + Phase: EXECUTE` mesmo após todas as ACs marcadas `[x]` e o commit feito — porque `/mustard:close` nunca foi chamado.
- Os 11 valores de `SpecStatus` misturam **fase no pipeline** (Planning, Implementing, Reviewing, Qa) com **desfecho terminal** (Completed, Cancelled, Abandoned) com **qualificadores ortogonais** (Blocked, WaveFailed) com **janela temporal** (ClosedFollowup). Uma dimensão só não comporta os três conceitos sem ambiguidade.
- A lista atual (`SpecCard`) usa ~150px verticais por item; cabem 3 specs por tela. Linear, em comparação, exibe 15-20 itens densamente com bullet de progresso e agrupamento natural.

## Cobertura (auditoria contra a conversa)

Cada decisão tomada na sessão de design é mapeada a uma wave abaixo ou justificada como **non-goal**:

| Decisão da conversa | Onde está atendida |
|---|---|
| Unificar `SpecStatus` + `Phase` em `Stage` + `Outcome` + `Flags` | Wave 1 |
| Fundir `Review` em `QaReview` (5 stages, não 6) | Wave 1 |
| `ClosedFollowup` vira flag `followup_open`, não stage | Wave 1 |
| `Blocked` e `WaveFailed` viram flags ortogonais | Wave 1 |
| Parser tolerante (lê headers legados durante transição) | Wave 1 |
| Comando `spec_children_tree` (waves + AC + sub-specs em 1 round-trip) | Wave 2 |
| AC detalhado por AC (não só agregado `ac_passed/ac_total`) | Wave 2 |
| `SpecRow` substitui `SpecCard` — 1 linha por spec | Wave 3 |
| `StageBullet` SVG ring com 5 segmentos | Wave 3 |
| Agrupamento por Stage com count colapsável | Wave 3 |
| Grupos vazios visíveis como `▸ ... 0` | Wave 3 |
| Árvore expansível: waves + AC + sub-specs | Wave 3 (frontend) + Wave 2 (backend) |
| **Tarefas (`- [ ]`) NÃO entram na árvore** | Non-goal — ficam no drill-down |
| Default colapsado + lazy load ao expandir | Wave 3 |
| Sub-spec aninha 1 nível só | Wave 3 |
| Click no child abre drill-down do parent | Wave 3 |
| Tipo do child (`wave`/`ac`/`sub-spec`) como coluna sutil | Wave 3 |
| 18 SKILLs `/mustard:*` emitem novos kinds de evento | Wave 4 |
| Hook `spec_hygiene` detecta + auto-close protegido por gate | Wave 5 |
| Eventos `hygiene.detected`/`hygiene.autoclose`/`hygiene.skipped` | Wave 5 |
| Card "Saúde do workspace" no `/workspace` | Wave 6 |
| Badges inline + filtro `Suspeitas` em `/specs` | Wave 6 |
| Rota nova `/health` separada | **Non-goal** — evita drift; volume não justifica |
| Migração criteriosa das specs existentes | Wave 7 |
| Invariante: `Stage::Close && Outcome::Active` é proibido | Wave 7 |
| Página `/spec/{name}` drill-down | **Non-goal** — intocada |
| Avatar/assignee como Linear | **Non-goal** — Mustard é single-user |
| ID artificial tipo `SIA-56` | **Non-goal** — slug `YYYY-MM-DD-name` já é ID natural |
| Tag obrigatória `Feature`/`Bug` como Linear | **Non-goal** — inferido do nome do skill que originou |
| Tasks na árvore | **Non-goal** — dezenas por spec, vão no drill-down |

## Modelo de dados final (Wave 1)

```rust
enum Stage {
    Analyze,
    Plan,
    Execute,
    QaReview,   // Review + QA fundidos; janela de follow-up vive aqui ou em Close+flag
    Close,
}

enum Outcome {
    Active,         // ainda viva
    Completed,      // fechou OK
    Cancelled,      // aborto deliberado (após eventos reais)
    Abandoned,      // ghost — nunca foi pipeline de verdade
}

struct SpecState {
    stage: Stage,
    outcome: Outcome,
    flags: Flags,
}

struct Flags {
    blocked: bool,
    wave_failed: bool,
    followup_open: bool,
}
```

**Invariantes** (testadas em Wave 7):
- `Outcome != Active` ⇒ `Stage == Close` (não-active sempre fechado).
- `Outcome == Abandoned` ⇒ não existem eventos reais (`tool.use`/`pipeline.phase`) para o slug.
- `flags.followup_open == true` ⇒ `Stage == Close && Outcome == Active`.
- `flags.wave_failed == true` ⇒ `Stage == Execute` (waves só rodam em Execute).

## Header da spec (formato novo, escrito a partir de Wave 4)

<!-- doc-example: keys spelled with trailing space before colon to dodge the
     `^### Key:` strip pattern; the legacy header format is documented here
     only for posterity (Wave 3 of mustard-unification moved these to meta.json). -->

```
### Stage : Execute
### Outcome : Active
### Flags : blocked, wave_failed
### Lang : pt
### Checkpoint : 2026-05-21T00:00:00Z
```

Campos `Status` e `Phase` legados continuam **legíveis** pelo parser (Wave 1) durante a transição. Wave 7 reescreve todos os headers existentes para o formato novo. *Nota: a partir do Wave 3 de mustard-unification, os campos acima vivem em `meta.json` lateral — o exemplo permanece como referência histórica do formato in-md.*

## Wave plan

Detalhe em `wave-plan.md`. Resumo:

1. **Wave 1 — core**: `Stage`/`Outcome`/`Flags` em `mustard-core` + parser tolerante.
2. **Wave 2 — rt**: comando `spec_children_tree` + AC detalhado no projection.
3. **Wave 3 — dashboard**: `SpecRow`, `StageBullet`, agrupamento, árvore expansível.
4. **Wave 4 — skills**: 18 SKILLs `/mustard:*` emitem `pipeline.stage`/`pipeline.outcome`/`pipeline.flag.*` e escrevem header novo.
5. **Wave 5 — hygiene**: hook `spec_hygiene` + eventos `hygiene.*` + auto-close com gate.
6. **Wave 6 — observability**: card "Saúde" + badges inline + filtro `Suspeitas`.
7. **Wave 7 — migration**: comando `migrate-spec-headers` + execução + testes de invariante.

## Acceptance Criteria (parent-level)

ACs específicos de cada wave estão no respectivo `wave-N-*/spec.md`. ACs **transversais** (validação do conjunto):

- [ ] AC-P-1: `rg -n '### Status:' .claude/spec/` retorna apenas specs em formato legado pré-migração ou **vazio** após Wave 7.
- [ ] AC-P-2: `rg -n '### Phase:' .claude/spec/` mesmo critério de AC-P-1.
- [ ] AC-P-3: `rg -n '### Stage:' .claude/spec/` retorna **todas** as specs pós-migração (Wave 7).
- [ ] AC-P-4: Build do workspace verde: `cargo build && pnpm --filter mustard-dashboard build`.
- [ ] AC-P-5: Lint verde: `cargo clippy --workspace -- -D warnings` e `pnpm --filter mustard-dashboard lint`.
- [ ] AC-P-6: Testes do core passam: `cargo test -p mustard-core`.
- [ ] AC-P-7: Smoke visual `/specs` no `pnpm tauri:dev` mostra lista densa estilo Linear com bullets coloridos, agrupamento por Stage, e ao expandir o `2026-05-21-tf-skill-mirror` aparecem AC + sub-spec.
- [ ] AC-P-8: Spec `2026-05-21-tf-skill-mirror` está **fechada** após Wave 5 (auto-close por hygiene) ou Wave 7 (migração) — cabeçalho `Stage: Close, Outcome: Completed`.

## Limites

**IN:**
- `packages/core/src/model/view/spec.rs` + `pipeline.rs` + `view/mod.rs`
- `packages/core/src/projection/card.rs` + `reader/sqlite.rs`
- `packages/core/tests/reader_contract.rs`
- `apps/rt/src/run/spec_sections.rs` + `metrics_wave_status.rs` + `rebuild_specs.rs`
- `apps/rt/src/run/migrate_spec_headers.rs` (novo)
- `apps/rt/src/hooks/spec_hygiene.rs` (novo)
- `apps/rt/src/hooks/post_edit.rs` (ajuste — auto-marca AC sem reverter Outcome)
- `apps/dashboard/src-tauri/src/spec_views.rs` + `lib.rs` + `telemetry.rs`
- `apps/dashboard/src/components/specs/*` (SpecCard → SpecRow, StageBullet novo)
- `apps/dashboard/src/components/page/PhaseChip.tsx` (deprecar ou reaproveitar)
- `apps/dashboard/src/pages/Specs.tsx` + `Workspace.tsx` (Saúde card)
- `apps/dashboard/src/lib/dashboard.ts` + `lib/types/specs.ts`
- `apps/dashboard/src/i18n.ts` (strings novas)
- `apps/cli/templates/commands/mustard/*/SKILL.md` (18 skills emitindo novos kinds)
- `.claude/commands/mustard/*/SKILL.md` (cópias instaladas — Wave 4 ou tactical-fix posterior)
- `.claude/spec/*/spec.md` (migração — Wave 7, scope = todos os arquivos)

**OUT:**
- `apps/dashboard/src/pages/SpecDetail.tsx` e `components/specs/SpecDetailDashboard.tsx` — intocados conforme decisão de design.
- `apps/dashboard/src/pages/Knowledge.tsx`, `Economia.tsx`, `Prd.tsx`, `Commands.tsx`, `Settings.tsx`, `Preferences.tsx`, `Home.tsx`, `ProjectDetail.tsx` — fora de escopo (não consomem `SpecStatus` exceto Workspace/Knowledge, e nesses só lê o enum).
- Refactor do modelo de `events` no SQLite — preservado.
- Nova rota `/health` no dashboard — descartada como non-goal.

## Riscos eliminados por design

- **Risco: auto-close cego marca spec quebrada como completed.**
  Mitigação por design: o hygiene hook só emite `pipeline.status: completed` se `close-gate` passar (build/lint/test/QA verde). Se gate falha, emite `hygiene.skipped` e surface alerta — nunca fecha cego.
- **Risco: migração corrompe specs ativas em meia-execução.**
  Mitigação por design: `migrate-spec-headers` é (a) dry-run obrigatório por default; (b) escrita atômica via tempfile+rename; (c) idempotente — rodar 2x é seguro; (d) audit-log em `.claude/.harness/migration-2026-05-21.log.json` com antes/depois por arquivo; (e) reversível via `git checkout`.
- **Risco: parser tolerante mantém formato legado vivo indefinidamente.**
  Mitigação por design: Wave 7 executa a migração + um teste de invariante (AC-P-1/P-2) garante que nenhuma spec usa header legado após a wave.
- **Risco: árvore expansível causa N+1 queries no SQLite por linha.**
  Mitigação por design: `spec_children_tree` retorna `{waves, acs, subspecs}` em **um round-trip**; lazy-load ao expandir; React Query cacheia por spec_name.
- **Risco: Stage::Close + Outcome::Active aparece em runtime.**
  Mitigação por design: invariante testado em CI (AC-P-x); construtor `SpecState::new` rejeita combinação ilegal.

## Follow-ups não-bloqueantes

Cabem em sub-spec (tactical-fix) se surgirem durante REVIEW/QA:

- Polimento visual do `StageBullet` em motion library (animação do arco preenchendo) — feature de craft, não bloqueia funcionalidade.
- Migração das cópias instaladas em `.claude/commands/mustard/*/SKILL.md` se Wave 4 mexer só nos templates — virou tactical-fix `tf-skill-mirror` em sessão anterior; padrão repetido se necessário.
- Remoção do componente `PageHeader.tsx` (já sem call-sites após task de hoje) — deferido; pode entrar em cleanup posterior.

## Dependências externas

Nenhuma. Todo o trabalho é interno ao monorepo.

## Não inclui (justificado)

- **Coordinate phase** (existe no `pipeline.rs::Phase` enum) — não há case de uso ativo; mantida no enum mas não exposta na UI nem na nova `Stage`.
- **Roadmap multi-spec parent** — fora deste escopo; cabe spec própria quando precisarmos disso.
- **Filtro por autor / assignee** — Mustard é single-user, sem ganho.
- **Bulk operations na lista** (selecionar N specs e mover de status) — fora; cabe spec própria se demanda surgir.
