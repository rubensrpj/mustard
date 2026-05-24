# Wave plan — spec-lifecycle-unification

## Visão geral

Sete waves sequenciais, cada uma com escopo e role bem definidos. Waves dependem da anterior compilar e passar nos próprios ACs antes de iniciar.

```
W1 (core) ─► W2 (rt) ─► W3 (dashboard) ─► W4 (skills) ─► W5 (hygiene) ─► W6 (observability) ─► W7 (migration)
```

| # | Slug | Role | Modelo recomendado | Resumo |
|---|---|---|---|---|
| 1 | `wave-1-core` | core | opus | `Stage`/`Outcome`/`Flags` no `mustard-core` + parser tolerante + invariantes |
| 2 | `wave-2-rt` | rt | opus | Comando `spec_children_tree` + projection AC detalhado + emit-pipeline aceita novos kinds |
| 3 | `wave-3-dashboard` | dashboard | opus | `SpecRow`, `StageBullet`, agrupamento por Stage, árvore expansível com lazy-load |
| 4 | `wave-4-skills` | cli (templates) | sonnet | 18 SKILLs emitem `pipeline.stage`/`pipeline.outcome`/`pipeline.flag.*` e escrevem header novo |
| 5 | `wave-5-hygiene` | rt | opus | Hook `spec_hygiene` + eventos `hygiene.*` + auto-close com close-gate |
| 6 | `wave-6-observability` | dashboard | sonnet | Card "Saúde" no Workspace + badges inline em Specs + filtro `Suspeitas` |
| 7 | `wave-7-migration` | rt | opus | `migrate-spec-headers` + execução criteriosa + testes de invariante AC-P-1..8 |

## Dependências entre waves

- **W2** depende de W1 (consome `Stage`, `Outcome`, `Flags` do core).
- **W3** depende de W2 (consome `spec_children_tree`) e de W1 (renderiza Stage/Outcome).
- **W4** depende de W1 (emite novos kinds usando os enums).
- **W5** depende de W1 e W4 (hook usa o state model e os event kinds emitidos pelas skills).
- **W6** depende de W1, W4, W5 (renderiza eventos `hygiene.*`).
- **W7** depende de W1 (parser novo valida headers reescritos).

W3 e W4 podem ser executadas em paralelo se houver capacidade — sem dependência cruzada — mas a convenção do Mustard executa waves em sequência. Mantemos sequencial para simplificar gates.

## Roles e agentes alvo

- `core` → `core-impl` agent (acesso a `packages/core/`)
- `rt` → `rt-impl` agent (acesso a `apps/rt/`)
- `dashboard` → `dashboard-impl` agent (acesso a `apps/dashboard/`)
- `cli (templates)` → `cli-impl` agent (acesso a `apps/cli/templates/`)

## Gates entre waves

Cada wave só passa para a próxima quando:

1. Build do crate/app afetado passa.
2. Lint passa (`cargo clippy --workspace` ou `pnpm lint` conforme área).
3. Testes do crate/app afetado passam.
4. ACs declarados no `wave-N-*/spec.md` retornam pass (rodados via `mustard-rt run qa-run --spec ...`).

Falha em qualquer gate → não inicia próxima wave; sub-spec tactical-fix se necessário.

## Estratégia de transição (importante)

Para evitar deploy "big bang", o parser do W1 é **tolerante** ao formato legado durante W2-W6. Isso significa:

- Specs existentes (`### Status: approved + ### Phase: EXECUTE`) continuam **legíveis** sem migração.
- Novas specs criadas a partir de W4 já nascem com o formato novo (`### Stage:` / `### Outcome:` / `### Flags:`).
- W7 só **reescreve** os headers em batch ao final — quando todo o pipeline e dashboard já estão prontos para o formato novo.

Resultado: durante W1-W6, dashboard e CLI funcionam em **modo bilíngue**. Wave 7 elimina o legado.

## Migração — wave 7 em detalhe

A wave 7 tem três sub-objetivos:

1. **Ferramenta de migração** (`mustard-rt run migrate-spec-headers`)
   - Dry-run default; `--apply` exige flag explícita.
   - Escrita atômica (tempfile + rename); idempotente.
   - Audit log em `.claude/.harness/migration-{date}.log.json` (antes/depois por arquivo).
   - Reversibilidade via `git`.

2. **Execução em batch** (rodar a ferramenta)
   - Dry-run primeiro, log inspecionado manualmente.
   - `--apply` em commit dedicado para diff revisável.

3. **Testes de invariante**
   - Unit: `SpecState::new(Stage::Close, Outcome::Active, _)` retorna `Err`.
   - Unit: `Outcome::Abandoned` exige zero eventos.
   - Integração: percorre todas as specs em `.claude/spec/`, parseia, valida invariantes.

## Mapping legado → novo (tabela de migração)

Aplicado por W7 quando `### Status:` e/ou `### Phase:` são encontrados no header:

| Header legado | Stage | Outcome | Flags |
|---|---|---|---|
| `Status: draft` | Plan | Active | — |
| `Status: approved` | (deriva de `Phase` se presente; senão `Plan`) | Active | — |
| `Status: planning` | Plan | Active | — |
| `Status: in_progress` ou `implementing` | Execute | Active | — |
| `Status: reviewing` | QaReview | Active | — |
| `Status: qa` | QaReview | Active | — |
| `Status: closed-followup` | Close | Active | `followup_open` |
| `Status: completed` ou `closed` | Close | Completed | — |
| `Status: cancelled` ou `superseded` | (last known `Phase`, senão `Close`) | Cancelled | — |
| `Status: abandoned` ou `orphan` | Close | Abandoned | — |
| `Status: blocked` ou `paused` | (last known `Phase`, senão `Plan`) | Active | `blocked` |
| `Status: wave-failed` | Execute | Active | `wave_failed` |
| `Phase: ANALYZE/PLAN/EXECUTE/QA/CLOSE` | mesmo nome | (deriva de Status acima) | — |
| `Phase: REVIEW` | QaReview | (deriva) | — |

Regras de conflito:
- Se ambos `Status` e `Phase` apontam Stages diferentes, **Status terminal vence** (Cancelled, Completed, Abandoned são definitivos).
- Se Status é qualificador (blocked/wave-failed), **Phase decide Stage** e Status vira flag.
- Se nenhum header existir → erro de migração (spec mal-formada — log e skip).
- Se header já está no formato novo (`### Stage:`) → skip (idempotente).

## Eventos novos emitidos a partir de W4

```
pipeline.stage    { spec, stage: "Execute" }
pipeline.outcome  { spec, outcome: "Completed" }
pipeline.flag.set { spec, flag: "blocked" }
pipeline.flag.clear { spec, flag: "blocked" }
```

Eventos legados (`pipeline.phase`, `pipeline.status`) continuam **aceitos** pelo `emit-pipeline` durante W4-W6 — escritos como aliases dos novos. W7 não remove suporte legado no `emit-pipeline` (deferido).

## Riscos por wave e mitigação

| Wave | Risco | Mitigação |
|---|---|---|
| W1 | Quebrar consumers do `mustard-core` (`rt` e `dashboard` Tauri) | Manter `SpecStatus`/`Phase` antigos como `#[deprecated]` aliases por toda a transição; cargo build do workspace é AC |
| W2 | `spec_children_tree` lento em specs com muitas waves | Cache em `mustard-rt`; benchmark com spec real (`2026-05-21-flatten-spec-layout` que tem 5 waves) |
| W3 | Regressão visual em outras páginas que consomem `SpecCard` | Glob por uso de `SpecCard`; manter o componente até confirmar zero call-sites |
| W4 | SKILLs emitindo evento errado por divergência template/instalado | Aplicar nos templates (W4) + tactical-fix-mirror posterior se preciso ([[2026-05-21-tf-skill-mirror]] como precedente) |
| W5 | Auto-close marca spec quebrada como completed | Gate (build/lint/test) é OBRIGATÓRIO antes de emitir `pipeline.outcome: completed` |
| W6 | Card "Saúde" adiciona ruído ao Workspace | Card sempre colapsável; só expande automaticamente se há sinal (≥1 órfã ou auto-close recente) |
| W7 | Migração corrompe headers ativos | Dry-run obrigatório, audit log, escrita atômica, idempotência, reversível |

## ACs transversais

Definidos no `spec.md` parent (AC-P-1..8). Cada wave tem ACs específicos no seu próprio `spec.md`.
