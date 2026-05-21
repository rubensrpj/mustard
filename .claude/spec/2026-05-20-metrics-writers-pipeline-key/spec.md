# metrics-writers-pipeline-key — números honestos para `metrics wave-status`

### Parent: [[2026-05-20-mustard-wave-network-standard]]
### Status: cancelled
### Phase: CLOSE
### Scope: full
### Checkpoint: 2026-05-20T23:59:00Z
### Lang: pt
### Superseded-by: [[2026-05-20-economia-moat-unification]] — wave-4-attribution AC-5/AC-6 absorvem os 2 itens que esta spec não cobria por outro caminho. O fix proposto aqui (patch local em `metrics_wave_status.rs`) foi substituído pelo refactor de domínio em `packages/core/src/economy/` que resolve a causa raiz (writers emitindo dados incompletos + reader procurando no lugar errado) ao consolidar tudo num módulo único.

## PRD

## Contexto

A wave 4 (`metrics-diagnose-fix`) entregou o dashboard totalmente wired para `mustard-rt run metrics wave-status`, mas o subcomando devolve `tokens_saved=0`, `duration_ms=0`, `retries=0`, `status=null` para toda wave. A causa, registrada no `metrics-audit.md` da wave 4 (Audit-1), é um **descasamento writer↔reader que vive em `apps/rt/` — fora do § Limites daquela wave**:

1. **Reader (`apps/rt/src/run/metrics_wave_status.rs:175-223`)** filtra eventos com `WHERE json_extract(payload,'$.pipeline') = ?1` onde `?1` é o nome da wave (`wave-1-rt-infra`, etc.).
2. **Writers de produção** (`token.saved`, `pipeline.status`, `retry.attempt`) **não** populam `payload.pipeline` quando a pipeline ativa é uma child wave — o nome do parent é gravado na coluna top-level `spec`.
3. **`token.saved` não tem nenhum emitter no código** — só consumidores. Os emitters que existem são `rtk.savings`, `prompt.economy.saved`, `hook.savings` e `routing.savings`, consumidos em `packages/core/src/projection/workspace.rs:73-80`.
4. **`memory cross-wave`** (também escrito na wave 1) tem o mesmo sintoma residual: o único writer que seta `payload.pipeline` é `apps/rt/src/run/memory.rs`, mas a query do reader não encontra eventos quando o `<wave-name>` extraído da tabela `wave-plan.md` não bate com o que foi gravado em runtime (validar caso real abaixo).

A consequência prática é grave: o operador não confia em nenhuma KPI da Economia/Quality/Activity, e o feature "Wave network como padrão" entregou a infra de leitura sem nada que valide o ciclo escrita→leitura→render.

Esta sub-spec, irmã rastreável de [[2026-05-20-mustard-wave-network-standard]] (padrão estabelecido por [[2026-05-20-tactical-fix-via-sub-spec]]), conserta os writers e o reader para que `metrics wave-status` devolva números honestos e `memory cross-wave` retorne markdown não-vazio quando há memória prévia.

## Stakeholders

Operadores que usam o dashboard para validar custo/qualidade do pipeline. Indiretamente: toda wave futura que depende de cross-wave memory injection — sem ela, o agente de cada wave parte do zero.

## Métrica de sucesso

- `mustard-rt run metrics wave-status --spec 2026-05-20-mustard-wave-network-standard` devolve `tokens_saved>0` para a wave 1 (que rodou com RTK emitting savings).
- `mustard-rt run metrics wave-status` devolve `retries>0` para a wave 1 (1 fix-loop registrado).
- `mustard-rt run memory cross-wave --spec 2026-05-20-mustard-wave-network-standard --wave 4` devolve markdown não-vazio (memória da wave 1 já está persistida).
- Página Economia do dashboard mostra valores não-zero por wave após `pnpm tauri dev` em modo manual.
- Teste de regressão em `metrics_wave_status::tests` cobre o cenário "evento gravado com `spec=<parent>` e wave inferida via outro mecanismo" para que a divergência não volte.

## Não-Objetivos

- **Não migrar schema** do `mustard.db`. Solução vive em queries + writers, não em DDL.
- **Não emitir `token.saved` como evento canônico novo.** Os 4 kinds existentes (`rtk.savings`/`prompt.economy.saved`/`hook.savings`/`routing.savings`) são a fonte de verdade — o reader passa a fazer UNION sobre eles.
- **Não tocar o dashboard.** Wave 4 já entregou o consumo; esta sub-spec só faz os números aparecerem no lado server.
- **Não criar UI de "configurar atribuição de eventos a waves".** A atribuição é derivada do contexto da pipeline ativa no momento do emit, não algo configurável.
- **Não alterar `apps/rt/src/run/memory.rs`** (writer de `agent.memory`) — ele já popula `payload.pipeline`. Investigar o reader.
- **Não fazer follow-up de specs históricas.** O fix aplica-se aos eventos gravados a partir do merge.

## Critérios de Aceitação

Critérios binários, executáveis. `node -e "..."` cross-shell (memória `feedback_ac_cross_shell_windows`).

- [ ] AC-1: Cargo check passa — Command: `cargo check -p mustard-rt`
- [ ] AC-2: Cargo test passa — Command: `cargo test -p mustard-rt -- metrics_wave_status memory_cross_wave`
- [ ] AC-3: `metrics wave-status` devolve `tokens_saved>0` para wave-1-rt-infra — Command: `bash -c 'out=$(mustard-rt run metrics wave-status --spec 2026-05-20-mustard-wave-network-standard); node -e "const j=JSON.parse(process.argv[1]);const w=j.waves.find(w=>w.name===\"wave-1-rt-infra\");if(!w||(w.tokens_saved||0)<=0)throw new Error(\"tokens_saved still zero: \"+JSON.stringify(w))" "$out"'`
- [ ] AC-4: `metrics wave-status` devolve `retries>=1` para wave-1-rt-infra (1 fix-loop conhecido) — Command: `bash -c 'out=$(mustard-rt run metrics wave-status --spec 2026-05-20-mustard-wave-network-standard); node -e "const j=JSON.parse(process.argv[1]);const w=j.waves.find(w=>w.name===\"wave-1-rt-infra\");if(!w||(w.retries||0)<1)throw new Error(\"retries still 0: \"+JSON.stringify(w))" "$out"'`
- [ ] AC-5: `memory cross-wave` devolve markdown não-vazio para wave>=2 do parent atual — Command: `bash -c 'out=$(mustard-rt run memory cross-wave --spec 2026-05-20-mustard-wave-network-standard --wave 4); [ -n "$out" ] && echo "$out" | grep -q "wave-1-rt-infra"'`
- [ ] AC-6: Reader tem teste de regressão para o caso parent-spec/child-wave — Command: `node -e "const t=require('fs').readFileSync('apps/rt/src/run/metrics_wave_status.rs','utf8');if(!t.includes('parent_spec_child_wave_attribution'))throw new Error('regression test name missing')"`

## Plano

## Arquivos (~4)

```
apps/rt/src/run/metrics_wave_status.rs   (modify — query UNION sobre os 4 kinds reais + fallback de atribuição)
apps/rt/src/run/memory_cross_wave.rs     (modify — debug + fix do parser de wave-plan.md ou da query)
apps/rt/src/hooks/tracker.rs OU writer correlato (modify — popular payload.pipeline quando active wave é child; arquivo exato é descoberta da fase ANALYZE)
apps/rt/src/run/metrics_wave_status.rs (tests anexados ao final do mesmo arquivo)
```

## Tarefas

### General Agent

#### Tarefa 0 — ANALYZE (entrega lista de writers que precisam tocar `payload.pipeline`)

- [ ] Grep `store.append\|append_event` em `apps/rt/src/` e `packages/core/src/` para mapear todos os writers de eventos `token.*`, `pipeline.status`, `retry.attempt`
- [ ] Para cada writer, verificar se o contexto da chamada tem acesso ao nome da wave ativa (via `pipeline-state.json` `currentWave` ou via env/state setado pelo `/mustard:resume`)
- [ ] Confirmar que `agent.memory` realmente seta `payload.pipeline` corretamente (rodar uma escrita real e ler o que foi persistido com `event-projections` ou `db-query` se existir)
- [ ] Listar exatamente: (a) writers que precisam ser modificados; (b) writers que já populam corretamente; (c) caso especial do reader que precisa UNION

#### Tarefa 1 — Reader fix

- [ ] `apps/rt/src/run/metrics_wave_status.rs`: query de `tokens_saved` passa a UNION sobre `('rtk.savings','prompt.economy.saved','hook.savings','routing.savings')` em vez de `'token.saved'`. Soma `json_extract(payload,'$.saved')` quando presente; fallback `json_extract(payload,'$.tokens')` se o emitter usa outra chave.
- [ ] Mesma função: adicionar fallback de atribuição — `WHERE (json_extract(payload,'$.pipeline')=?1 OR (spec=?parent AND json_extract(payload,'$.wave')=?wave_n))`. O parâmetro `?wave_n` é o número 1..N extraído do nome da wave (regex `^wave-(\d+)-`).
- [ ] `retries`: mesma estrutura, conta `retry.attempt`.
- [ ] `duration_ms`: usar `max(ts)-min(ts)` sobre o conjunto unido (reader já faz, só ajustar o WHERE).

#### Tarefa 2 — Writer fix

- [ ] Para cada writer identificado na Tarefa 0(a), adicionar `payload.pipeline` quando há child wave ativa. Source da wave ativa: ler `.pipeline-states/{parent}.json` `currentWave` e mapear para o nome via wave-plan.md (helper já existe em `memory_cross_wave.rs` — extrair pra módulo compartilhado).
- [ ] Se o writer não tem acesso ao parent name (rodando fora de pipeline), `payload.pipeline` permanece ausente — comportamento atual preservado para eventos globais.

#### Tarefa 3 — Memory cross-wave fix

- [ ] Rodar `mustard-rt run memory cross-wave --spec 2026-05-20-mustard-wave-network-standard --wave 4` e capturar exatamente onde retorna vazio. Hipóteses a verificar em ordem:
  1. Parser de `wave-plan.md` não extrai os nomes das waves (tabela usa pipe `|` markdown padrão)
  2. Query filtra `payload.pipeline` mas a memória foi gravada com outra chave
  3. Spec name não bate (`memory agent --json` aceitou `pipeline: 'wave-1-rt-infra'` mas o lookup espera outro)
- [ ] Aplicar fix mínimo que faça o output não-vazio.
- [ ] Adicionar teste regressivo `memory_cross_wave::tests::reads_prior_waves_from_real_writer` que faz o roundtrip writer→reader sem mock.

#### Tarefa 4 — Teste de regressão

- [ ] `apps/rt/src/run/metrics_wave_status.rs`: adicionar `mod tests { fn parent_spec_child_wave_attribution() { ... } }` que insere eventos reais com `spec=<parent>` e nome do reader = `<wave>`, valida que UNION + fallback de atribuição faz `tokens_saved>0`.

#### Tarefa 5 — Validate

- [ ] `cargo check -p mustard-rt`
- [ ] `cargo test -p mustard-rt -- metrics_wave_status memory_cross_wave`
- [ ] Rodar `mustard-rt run metrics wave-status --spec 2026-05-20-mustard-wave-network-standard` e confirmar números não-zero contra waves 1 e 4

## Dependências

- [[2026-05-20-mustard-wave-network-standard]]/[[wave-1-rt-infra]] — usa os subcomandos `metrics wave-status` e `memory cross-wave` que essa wave introduziu.
- [[2026-05-20-mustard-wave-network-standard]]/[[wave-4-metrics-diagnose-fix]] — consome o `metrics-audit.md` produzido por essa wave como contrato do que precisa consertar.

## Network

- Parent: [[2026-05-20-mustard-wave-network-standard]]
- Origem do diagnóstico: [[wave-4-metrics-diagnose-fix]] (`metrics-audit.md` Audit-1)
- Padrão de existência (sub-spec linkada): [[2026-05-20-tactical-fix-via-sub-spec]]
- Consumidores que ficam honestos quando essa spec entregar: Economia.tsx, SpecCard, Workspace alerts, qualquer dashboard de Quality

## Limites

Em escopo: `apps/rt/src/run/metrics_wave_status.rs`, `apps/rt/src/run/memory_cross_wave.rs`, **um** writer file em `apps/rt/src/hooks/` (exato definido na Tarefa 0), opcional helper compartilhado em `apps/rt/src/run/mod.rs` ou novo `apps/rt/src/run/wave_lookup.rs` pequeno.

Fora de escopo:
- Dashboard (wave 4 já entregou o consumer)
- Schema `mustard.db` (queries + writers só)
- RTK / token economy / hooks novos (apenas LER o que já é emitido)
- `mustard-core` (writers vivem em `apps/rt/`, não em core)
- Specs históricas (eventos antes do merge ficam zerados — só novos)
- Light scope flow (essa sub-spec é Full)
