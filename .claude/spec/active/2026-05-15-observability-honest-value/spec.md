# Feature: Observability Honesta — Vocabulário de Eventos + Dashboard que Prova Valor

### Status: implementing | Phase: EXECUTE | Scope: full | Waves: 7 (0-6)
### Checkpoint: 2026-05-15T12:00:00Z
### Lang: pt
### Repos: C:/Atiz/mustard (raiz) + C:/Atiz/mustard-dashboard (UI)

## Contexto

Auditoria do Mustard Dashboard contra os dados reais do projeto `C:/Atiz/Competi/projetos/sialia`
revelou que o dashboard **não consegue provar que vale a pena usar o Mustard**. A causa raiz
não é principalmente a UI — é o que o Mustard **emite**.

Fatos medidos (sialia `.claude/.harness/events.jsonl`, 299 linhas, sessão de hoje 07:14→11:06,
arquivo fresco):

| Evento emitido | Qtd na sessão |
|---|---|
| `tool.use` | 276 |
| `finding` | 10 |
| `mustard.subtraction.applied` | 8 |
| `pipeline.phase` | 2 |
| `skill.invoked` / `session.start` / `commit-gate.check` | 1 cada |

Fases marcadas nos `tool.use`: **EXECUTE 228 · REVIEW 37 · PLAN 11 · ANALYZE 0 · QA 0 · CLOSE 0**.

Inventário dos emissores (grep em `templates/hooks` + `templates/scripts`):
`tool.use`, `pipeline.phase`, `session.start`, `skill.invoked`, `qa.result`, `agent.start`,
`agent.stop`, `mustard.subtraction.applied`, `finding`, `duplication.warn`, `convention.warn`,
`commit-gate.check`, `boundary.expansion`, `close-gate.check`. **Ninguém emite `retry.attempt`.**

Cinco verdades técnicas confirmadas:

1. **ANALYZE nunca emite.** `pipeline-phase.js` (linhas 47-48, 81) só emite `pipeline.phase`
   quando um arquivo `.claude/.pipeline-states/{spec}.json` é escrito *e a fase muda*. Mas
   `CLAUDE.md` define ANALYZE como "Grep/Glob direct preferred" — roda no parent **antes** de
   existir state file. Resultado: zero telemetria de ANALYZE. O dashboard mostra `0` fielmente.

2. **Quatro vocabulários de fase divergentes.** `CLAUDE.md`: ANALYZE/PLAN/EXECUTE/QA/CLOSE.
   `pipeline-phase.js` (comentário linha 130-132): ANALYZE/PLAN/EXECUTE/CLOSE/COORDINATE.
   `metrics-tracker.js` marca de fato: EXECUTE/REVIEW/PLAN. Cards do dashboard:
   ANALYZE/PLAN/EXECUTE/QA/CLOSE. **REVIEW tem 37 eventos reais e nenhum card** — é invisível.

3. **`retry.attempt` não existe como evento.** `Quality.tsx` conta `event_type==="retry.attempt"`
   → coluna RETRIES sempre `0`. O dado de retry existe (knowledge.json tem `high-hook-retry-*`
   com "Pipeline triggered N hook-level retries") mas nunca vira evento consumível.

4. **`knowledge.json` poluído.** `session-knowledge.js` grava entradas `high-hook-retry-*` e
   `heavy-pipeline-*` com `type: convention` / `type: pattern`. São **telemetria de atrito**,
   não conhecimento. O campo `occurrences: 187` é só quantas vezes o extrator re-leu o mesmo
   `.metrics` — número sem significado. A página Knowledge promete "Padrões e lições" e mostra
   contadores de retry rotulados como "CONVENÇÃO".

5. **O dashboard mistura acumulador vitalício com dado de sessão sem rotular.** "Hooks 323.4K"
   e "routing 141K" são totais "desde a instalação"; numa sessão de 2h não se movem
   visivelmente. Não é bug — é apresentação. Falta o delta de sessão.

Bugs de UI confirmados no código de `mustard-dashboard`:

- **Click na spec não carrega** — `Telemetry.tsx:177` `selectPipeline` existe e abre
  `SpecSidePanel`, mas o conteúdo de detalhe falha (specName não bate o arquivo OU o Tauri
  command retorna vazio).
- **Badges de coletor contraditórios** — `Telemetry.tsx:220` usa `is_fresh || livePipelines>0`
  ("live"); `PromptEconomy.tsx:54-62` usa `otel_healthy + last_metric_ts + janela 5min`
  ("Coletor parado"). Dois sinais independentes, ambos corretos, contradizem-se na tela.
- **Telemetria e Prompt Economy duplicam** — as duas leem o mesmo hook `usePromptEconomy`;
  a página Prompt Economy é o drill-down de um card da Telemetria. Números divergem
  (901KB vs 1.7MB) por cache/timing entre duas queries da mesma fonte.
- **Layout inconsistente** — Telemetry/Quality/PromptEconomy usam `w-full`; Knowledge não tem
  constraint de largura explícito; Settings é coluna estreita com metade da tela vazia.
- **Settings expõe nomes de env var** — `MUSTARD_QA_GATE_MODE`, `OTEL_EXPORTER_OTLP_PROTOCOL`
  etc. como título de campo, em vez de nome descritivo para humano.

Esta spec ataca a **raiz primeiro** (vocabulário de eventos no Mustard), senão o dashboard
fica polindo painéis que esperam dados que nunca chegam.

## Requisitos do usuário

Rastreabilidade do feedback original — cada item levantado pelo usuário, verbatim, mapeado
para a wave que o resolve. Nenhum item pode ser fechado sem cobrir todos os R.

| # | Feedback original | Wave |
|---|---|---|
| R1 | "Em execução — ao clicar na spec não carrega nada" | 5 |
| R2 | "Analyze — nunca é disparado mesmo durante resume/feature" | 2 |
| R3 | "RTK · comandos — funciona, mas precisamos auditar" | 0 |
| R4 | "Hooks · interceptação — está funcionando, mas os números não mudam" | 0, 6 |
| R5 | "Roteamento de modelo — os números mudam muito pouco" | 0, 6 |
| R6 | "Prompt Economy — USD muda mas os outros estão travados desde o início" | 0 |
| R7 | "Não consigo entender as informações juntas; preciso provar que compensa usar o Mustard" | 6 (Home/ROI) |
| R8 | "Imagem 11 e 12 não seria uma coisa só?" (Telemetria + Prompt Economy) | 6 |
| R9 | "Knowledge — o que é isso, não tem informação relevante, informações vagas, como analiso?" | 0, 4, 6 |
| R10 | "Qualidade — colunas sem legenda, sempre zeradas" | 0, 6 |
| R11 | "Settings — nomes de parâmetro; deveria ser descritivo para humano" | 6 |
| R12 | "Telas não seguem o mesmo formato responsivo; colunas sem ocupar a largura" | 6 |
| R13 | "Cada texto precisa de versão didática; abreviações com legenda; nomes descritivos" | 6 |
| R14 | "Usar frontend-design para padronizar todo o projeto" | 6 |

## Boundaries

Repo `C:/Atiz/mustard`:
- `templates/hooks/pipeline-phase.js` (vocabulário canônico + emitir ANALYZE)
- `templates/hooks/metrics-tracker.js` (alinhar tag de fase ao vocabulário canônico)
- `templates/hooks/session-knowledge.js` (parar de rotular atrito como convention/pattern)
- `templates/hooks/subagent-tracker.js` (verificar wiring de agent.start; sem mudança se OK)
- `templates/hooks/_lib/harness-event.js` (helper de fase canônica, se necessário)
- `templates/commands/mustard/feature/SKILL.md` (emitir marcador de ANALYZE no início)
- `templates/commands/mustard/bugfix/SKILL.md` (idem)
- `templates/commands/mustard/resume/SKILL.md` (idem, ao retomar em ANALYZE)
- `templates/scripts/emit-retry.js` (NEW — emissor de retry.attempt)
- `templates/refs/canonical-phases.md` (NEW — fonte única do vocabulário de fases)
- `templates/CLAUDE.md` + `.claude/CLAUDE.md` (alinhar tabela de fases ao canônico)
- `templates/pipeline-config.md` (alinhar fases)

Repo `C:/Atiz/mustard-dashboard`:
- `src-tauri/src/telemetry.rs` (badge único de freshness; sessão-delta; ROI; spec detail)
- `src-tauri/src/lib.rs` (registrar commands novos)
- `src/lib/dashboard.ts` + `src/api/promptEconomy.ts` (shapes)
- `src/components/SpecSidePanel.tsx` + `src/components/LivePipelineCard.tsx` (bug do click)
- `src/components/layout/AppShell.tsx` (page shell responsivo unificado)
- `src/pages/Telemetry.tsx` (absorve Economy; cards de fase = vocabulário canônico)
- `src/pages/PromptEconomy.tsx` (DELETE — fundida em Telemetria, ou vira aba)
- `src/pages/Knowledge.tsx` (recategorizar; atrito sai de "convenção")
- `src/pages/Quality.tsx` (legenda nas colunas; empty-state explicativo)
- `src/pages/Settings.tsx` (títulos descritivos; grupo "Avançado" colapsável)
- `src/pages/Home.tsx` (placar de ROI com-vs-sem Mustard)
- `src/data/env-catalog.ts` (nome humano + descrição por env var)

Artefato da spec (Wave 0, read-only):
- `.claude/spec/active/2026-05-15-observability-honest-value/provenance-map.md` (NEW)

Fora do escopo: model routing (intocável — `feedback_no_routing_downgrade`); esquema do
`entity-registry.json`; novas dependências npm/cargo; cálculo de pricing local.

## Summary

Sete waves (0-6), raiz antes de UI:

0. **Auditoria de proveniência de cada métrica.** Read-only. Para *todo* card exibido hoje
   no dashboard, mapeia fonte → transformação → valor exibido → classificação
   (REAL / ACUMULADO / INFERIDO / STALE / AUSENTE). Garante que "validar os cards" significa
   validar **cada métrica**, não só consertar os bugs já vistos nos screenshots. O
   `provenance-map.md` resultante orienta o que as Waves 5 e 6 corrigem.
1. **Vocabulário canônico de fases.** Uma fonte única (`canonical-phases.md`):
   ANALYZE → PLAN → EXECUTE → REVIEW → QA → CLOSE (+ COORDINATE para roadmaps). `pipeline-phase.js`,
   `metrics-tracker.js`, `CLAUDE.md`, `pipeline-config.md` e os cards do dashboard passam a
   usar o mesmo conjunto. **REVIEW vira fase reconhecida** (já emite 37 eventos).
2. **ANALYZE deixa de ser silenciosa.** Os SKILLs feature/bugfix/resume emitem
   `pipeline.phase {to: ANALYZE}` no início, antes de qualquer Grep — via `bun emit` igual já
   se faz com `emit-subtraction.js`.
3. **`retry.attempt` vira evento.** Novo `emit-retry.js`; o ponto que hoje computa
   `high-hook-retry` passa a emitir um `retry.attempt` por retry. Dashboard ganha dado real.
4. **Knowledge despoluída.** `session-knowledge.js` para de gravar `high-hook-retry-*` /
   `heavy-pipeline-*` como `convention`/`pattern`; esses viram `type: friction` (ou vão para
   `.claude/.metrics/`, não para `knowledge.json`). Knowledge fica só com padrão/decisão reais.
5. **Dashboard: bugs.** Click na spec carrega o detalhe; badge de coletor único e consistente
   em todas as telas (uma função `collectorHealth` no Tauri).
6. **Dashboard: IA + design.** Telemetria absorve Prompt Economy (fonte única, sem divergência);
   Home vira placar de ROI (com-vs-sem Mustard); cada número acumulado ganha delta de sessão;
   page shell responsivo unificado; Settings com nomes humanos; Knowledge recategorizada;
   Qualidade com legenda e empty-state. Passe final via skill `frontend-design`.

## Files (~27)

| Arquivo | Operação | Wave |
|---|---|---|
| `.claude/spec/.../provenance-map.md` | Create (artefato de auditoria) | 0 |
| `templates/refs/canonical-phases.md` | Create | 1 |
| `templates/hooks/pipeline-phase.js` | Edit (vocabulário + comentário VALID_PHASES) | 1 |
| `templates/hooks/metrics-tracker.js` | Edit (tag de fase canônica) | 1 |
| `templates/CLAUDE.md` | Edit (tabela de fases) | 1 |
| `.claude/CLAUDE.md` | Edit (tabela de fases) | 1 |
| `templates/pipeline-config.md` | Edit (fases) | 1 |
| `templates/commands/mustard/feature/SKILL.md` | Edit (emitir ANALYZE no início) | 2 |
| `templates/commands/mustard/bugfix/SKILL.md` | Edit (emitir ANALYZE) | 2 |
| `templates/commands/mustard/resume/SKILL.md` | Edit (emitir ANALYZE no resume) | 2 |
| `templates/scripts/emit-retry.js` | Create | 3 |
| `templates/hooks/session-knowledge.js` | Edit (retry/heavy → type friction) | 4 |
| `mustard-dashboard/src-tauri/src/telemetry.rs` | Edit (collectorHealth + spec detail + ROI) | 5 |
| `mustard-dashboard/src-tauri/src/lib.rs` | Edit (registrar commands) | 5 |
| `mustard-dashboard/src/components/SpecSidePanel.tsx` | Edit (carregar detalhe) | 5 |
| `mustard-dashboard/src/components/LivePipelineCard.tsx` | Edit (passar specName correto) | 5 |
| `mustard-dashboard/src/api/promptEconomy.ts` | Edit (shape unificado) | 5 |
| `mustard-dashboard/src/lib/dashboard.ts` | Edit (badge único + session delta) | 5 |
| `mustard-dashboard/src/components/layout/AppShell.tsx` | Edit (page shell responsivo) | 6 |
| `mustard-dashboard/src/pages/Telemetry.tsx` | Edit (absorve Economy; cards canônicos) | 6 |
| `mustard-dashboard/src/pages/PromptEconomy.tsx` | Delete (fundida) | 6 |
| `mustard-dashboard/src/pages/Home.tsx` | Edit (placar de ROI) | 6 |
| `mustard-dashboard/src/pages/Knowledge.tsx` | Edit (recategorizar) | 6 |
| `mustard-dashboard/src/pages/Quality.tsx` | Edit (legenda + empty-state) | 6 |
| `mustard-dashboard/src/pages/Settings.tsx` | Edit (nomes humanos + Avançado) | 6 |
| `mustard-dashboard/src/data/env-catalog.ts` | Edit (label humano por env) | 6 |
| `mustard-dashboard/src/components/Sidebar*.tsx` | Edit (remover item Prompt Economy) | 6 |

## Tasks

### Wave 0 — Auditoria de proveniência de cada métrica (Status: DONE)

Read-only. Produz `provenance-map.md` no diretório da spec — uma tabela com uma linha por
card/métrica: `Página | Card | Métrica | Fonte (arquivo/Tauri command) | Transformação |
Valor exibido | Classificação | Ação`. Classificação ∈ {REAL, ACUMULADO, INFERIDO, STALE,
AUSENTE}.

- [x] Inventariar TODOS os cards de TODAS as páginas: Home, Atividade, Telemetria, Qualidade,
      Prompt Economy, Knowledge, Comandos, PRD, Settings
- [x] Telemetria — auditar: EM EXECUÇÃO (linha de pipeline); ATIVIDADE POR FASE (eventos hoje,
      /5min, /1h, sparkline, por fase); RTK (tokens salvos, taxa, nº comandos); Hooks (TOTAL +
      breakdown por hook); Roteamento (intervenção %, bloqueados, por tipo de agente, por
      categoria de ação); card Prompt Economy
- [x] Telemetria "Como o projeto está indo" — auditar: Histórico do projeto, AC últimos QAs,
      Onde o esforço acontece
- [x] Telemetria — auditar: AGENTES DESPACHADOS; FERRAMENTAS uso acumulado (Read/Bash/Agent/
      Edit/Write)
- [x] Prompt Economy — auditar: Cache da API (USD total + por modelo), Bytes omitidos
      (diff-vs-full, wave-slice, review-diff-first, analyze-diff-skip), Eventos Claude Code
      (sessions, active time)
- [x] Knowledge — auditar: confidence %, occurrences, tool breakdown, contagem de tipo
- [x] Qualidade — auditar: métricas do topo + colunas por spec (FASE, WAVES, AC, RETRIES)
- [x] Para cada métrica: rastrear até a fonte real e confirmar se valor exibido == valor da
      fonte. Sinalizar todo INFERIDO (número calculado/estimado sem dado de origem)
- [x] RTK especificamente (R3): confirmar que "92M / 79.4%" vem de `rtk gain` (binário),
      é GLOBAL e não do projeto, e exigir rótulo "global RTK" honesto
- [x] Hooks/Roteamento (R4/R5): confirmar se "não mudam" é acumulador vitalício correto —
      classificar ACUMULADO e marcar "precisa de delta de sessão"
- [x] Prompt Economy subtractions (R6): confirmar por que `diff-vs-full`/`wave-slice` parecem
      travados — classificar (poucos eventos reais por sessão vs. emissor parado)
- [x] Knowledge (R9): registrar no mapa por que a página é incompreensível hoje (atrito
      rotulado como convenção, `occurrences` sem sentido)
- [x] O `provenance-map.md` é a entrada obrigatória das Waves 5 e 6

### Wave 1 — Vocabulário canônico de fases (Status: DONE)

- [x] Criar `templates/refs/canonical-phases.md` — fonte única: ANALYZE, PLAN, EXECUTE, REVIEW,
      QA, CLOSE, COORDINATE; descrição de cada uma + qual evento marca entrada na fase
- [x] `pipeline-phase.js` — atualizar comentário `VALID_PHASES` (linha 130-132) para o canônico;
      aceitar REVIEW e QA na lista
- [x] `metrics-tracker.js` — confirmar/ajustar a derivação de `payload.phase` para emitir só
      valores do canônico (hoje emite REVIEW, que é válido — manter; garantir QA/CLOSE também)
- [x] Alinhar `templates/CLAUDE.md`, `.claude/CLAUDE.md` e `pipeline-config.md`: a tabela de
      fases passa a citar o canônico e linkar `canonical-phases.md`

### Wave 2 — ANALYZE deixa de ser silenciosa (Status: DONE)

- [x] `feature/SKILL.md` — no início da fase ANALYZE, emitir
      `pipeline.phase {from: null, to: ANALYZE}` via `bun .claude/scripts/emit-*` (mesmo padrão
      cross-shell de `emit-subtraction.js`); reusar `emit-subtraction.js` generalizado OU
      pequeno `emit-phase.js` se preferir separação
- [x] `bugfix/SKILL.md` — idem ao entrar em ANALYZE
- [x] `resume/SKILL.md` — ao retomar pipeline cuja fase corrente é ANALYZE, emitir o marcador
- [x] Garantir idempotência: não emitir ANALYZE duas vezes para a mesma spec/sessão

### Wave 3 — retry.attempt vira evento (Status: DONE)

- [x] Criar `templates/scripts/emit-retry.js` — flags `--spec --wave --reason --tool`; emite
      `retry.attempt` no harness (cross-shell, sem `bun -e` inline)
- [x] Localizar o ponto que hoje computa "hook-level retries" (a origem do `high-hook-retry-*`
      em `session-knowledge`/`.metrics`) e fazer emitir um `retry.attempt` por ocorrência
- [x] Confirmar empiricamente que `agent.start`/`agent.stop` estão wired no `settings.json`
      template (subagent-tracker já os emite via `emitEvent`) — se o gap do sialia for install
      antigo, registrar em Concerns; senão, corrigir o wiring

### Wave 4 — Knowledge despoluída (Status: DONE)

- [x] `session-knowledge.js` — parar de gravar `high-hook-retry-*` e `heavy-pipeline-*` com
      `type: convention`/`pattern`. Opção A: `type: 'friction'` no mesmo `knowledge.json`.
      Opção B: gravar em `.claude/.metrics/friction.json`, fora do `knowledge.json`. Decidir
      no PLAN — preferência por B (Knowledge fica 100% padrão/decisão real)
- [x] Remover/ignorar o campo `occurrences` para entradas de atrito (número sem sentido)
- [x] `knowledge.json` consumido pelo dashboard fica só com padrão/convenção/decisão reais

### Wave 5 — Dashboard: bugs (Status: DONE)

- [x] `SpecSidePanel.tsx` + `LivePipelineCard.tsx` — corrigir o click: garantir que `specName`
      passado bate o diretório real em `.claude/spec/active/`; tratar fetch vazio com
      mensagem em vez de painel mudo
- [x] `telemetry.rs` — função única `collector_health()` retornando um enum
      (`live` / `stale` / `off`) com a mesma regra para todas as páginas
- [x] `dashboard.ts` + `promptEconomy.ts` — todas as telas consomem `collector_health()`;
      remover as duas lógicas de badge divergentes
- [x] Validar: Telemetria e (futura) seção Economy mostram o MESMO badge ao mesmo tempo

### Wave 6 — Dashboard: IA + design (Status: DONE)

- [x] `AppShell.tsx` — page shell único: largura máxima consistente, padding e grid
      responsivo; todas as páginas herdam (fim das colunas estreitas órfãs)
- [x] `Telemetry.tsx` — absorver Prompt Economy como seção/aba da mesma página, fonte de
      dados única (sem divergência 901KB vs 1.7MB); cards de fase = vocabulário canônico
      (inclui REVIEW)
- [x] Deletar `PromptEconomy.tsx` e remover o item do Sidebar
- [x] `telemetry.rs` — expor delta de sessão para cada acumulador vitalício; UI mostra
      "323.4K total · +N nesta sessão"
- [x] `Home.tsx` — placar de ROI: custo/ tokens com Mustard vs. estimativa sem (base: RTK
      tokens salvos + bytes omitidos por subtraction + USD OTEL). Resposta direta a
      "compensa usar o Mustard?"
- [x] `Knowledge.tsx` — separar "Padrões & decisões" (knowledge real) de "Atrito" (retry/
      heavy-pipeline); rótulo "CONVENÇÃO" só para convenção real; empty-state honesto;
      cabeçalho explica para que serve a página e como interpretá-la (R9)
- [x] `Quality.tsx` — tooltip/legenda em WAVES/AC/RETRIES; cada coluna vazia explica por que
      está vazia e como preencher
- [x] `Settings.tsx` + `env-catalog.ts` — título descritivo para humano; nome da env var só
      como subtítulo monoespaçado; knobs OTEL de baixo nível num grupo "Avançado" colapsável
- [x] Passe final com skill `frontend-design`: textos didáticos/explicativos em todas as
      telas; abreviações sempre com legenda; consistência visual (dark-first, Linear+Notion
      conforme `feedback_design_aesthetic`)

## Acceptance Criteria

Critérios binários, cross-shell (node -e / bash -c — `feedback_ac_cross_shell_windows`).

- [ ] AC-0a: `provenance-map.md` existe e cobre as 9 páginas — Command:
      `node -e "const c=require('fs').readFileSync('C:/Atiz/mustard/.claude/spec/active/2026-05-15-observability-honest-value/provenance-map.md','utf8');process.exit(['Home','Atividade','Telemetria','Qualidade','Prompt Economy','Knowledge','Comandos','PRD','Settings'].every(p=>c.includes(p))?0:1)"`
- [ ] AC-0b: toda linha de card no mapa tem classificação válida — Command:
      `node -e "const c=require('fs').readFileSync('C:/Atiz/mustard/.claude/spec/active/2026-05-15-observability-honest-value/provenance-map.md','utf8');const rows=c.split('\n').filter(l=>l.startsWith('|')&&!/^\|\s*-+/.test(l)&&!/Página/.test(l));const ok=rows.length>0&&rows.every(l=>/(REAL|ACUMULADO|INFERIDO|STALE|AUSENTE)/.test(l));process.exit(ok?0:1)"`
- [ ] AC-1: `canonical-phases.md` existe e lista as 7 fases — Command:
      `node -e "const fs=require('fs');const c=fs.readFileSync('C:/Atiz/mustard/templates/refs/canonical-phases.md','utf8');process.exit(['ANALYZE','PLAN','EXECUTE','REVIEW','QA','CLOSE','COORDINATE'].every(p=>c.includes(p))?0:1)"`
- [ ] AC-2: `pipeline-phase.js` reconhece REVIEW e QA no comentário VALID_PHASES — Command:
      `node -e "const c=require('fs').readFileSync('C:/Atiz/mustard/templates/hooks/pipeline-phase.js','utf8');process.exit(c.includes('REVIEW')&&c.includes('QA')?0:1)"`
- [ ] AC-3: SKILL feature emite ANALYZE — Command: rodar `/mustard:feature` num repo de teste
      e confirmar `grep '"to":"ANALYZE"' .claude/.harness/events.jsonl` retorna ≥1 linha
- [ ] AC-4: `emit-retry.js` emite `retry.attempt` — Command:
      `bash -c 'cd $(mktemp -d) && mkdir -p .claude && cp -r /c/Atiz/mustard/templates/hooks /c/Atiz/mustard/templates/scripts .claude/ && CLAUDE_PROJECT_DIR=$PWD bun .claude/scripts/emit-retry.js --spec t --wave 1 --reason test && grep -q "retry.attempt" .claude/.harness/events.jsonl'`
- [ ] AC-5: `session-knowledge.js` não grava atrito como convention/pattern — Command:
      `node -e "const c=require('fs').readFileSync('C:/Atiz/mustard/templates/hooks/session-knowledge.js','utf8');process.exit(/high-hook-retry|heavy-pipeline/.test(c)&&!/friction/.test(c)?1:0)"`
- [ ] AC-6: dashboard buildou sem erro — Command:
      `bash -c 'cd /c/Atiz/mustard-dashboard && pnpm build'` (exit 0)
- [ ] AC-7: `PromptEconomy.tsx` foi removida e Telemetria absorveu Economy — Command:
      `node -e "const fs=require('fs');const gone=!fs.existsSync('C:/Atiz/mustard-dashboard/src/pages/PromptEconomy.tsx');const t=fs.readFileSync('C:/Atiz/mustard-dashboard/src/pages/Telemetry.tsx','utf8');process.exit(gone&&/[Ee]conom/.test(t)?0:1)"`
- [ ] AC-8: badge de coletor é fonte única — Command:
      `node -e "const c=require('fs').readFileSync('C:/Atiz/mustard-dashboard/src-tauri/src/telemetry.rs','utf8');process.exit(/fn collector_health/.test(c)?0:1)"`
- [ ] AC-9: Settings usa título humano, não nome de env var, como label primário — Command:
      validar em `env-catalog.ts` que cada entrada tem campo `label`/`title` distinto da chave
      da env var (revisão manual + check de shape)
- [ ] AC-10: Home tem placar de ROI com-vs-sem Mustard — Command:
      revisão visual: Home renderiza custo/tokens com Mustard vs. estimativa sem, derivado de
      RTK + subtractions + OTEL
- [ ] AC-11: Quality tem legenda nas colunas — Command:
      `node -e "const c=require('fs').readFileSync('C:/Atiz/mustard-dashboard/src/pages/Quality.tsx','utf8');process.exit(/[Tt]ooltip|title=/.test(c)?0:1)"`
- [ ] AC-12: Knowledge separa conhecimento real de atrito — Command: revisão visual: entradas
      `high-hook-retry-*` não aparecem mais sob o rótulo "Convenção"
- [ ] AC-13: cargo test do dashboard passa — Command:
      `bash -c 'cd /c/Atiz/mustard-dashboard/src-tauri && cargo test'` (exit 0)
- [ ] AC-14: testes de hook do Mustard passam — Command:
      `bash -c 'cd /c/Atiz/mustard && bun test templates/hooks/__tests__/hooks.test.js'` (exit 0)
- [ ] AC-15: re-auditoria pós-Wave-6 — nenhum card exibe valor INFERIDO nem número
      divergente da fonte sem rótulo honesto. Command: revisão visual card a card contra o
      `provenance-map.md` atualizado; cada ACUMULADO mostra delta de sessão, cada STALE/
      AUSENTE mostra estado honesto, RTK rotulado como "global"
- [ ] AC-16: todo requisito R1-R14 da seção "Requisitos do usuário" tem wave entregue —
      Command: revisão da tabela de rastreabilidade; nenhum R sem cobertura

## Dependencies

- Wave 0 → todas (read-only, roda primeiro; o `provenance-map.md` orienta o que cada wave corrige)
- Wave 1 → Wave 2 (emitir ANALYZE precisa do vocabulário definido)
- Wave 1 → Wave 6 (cards de fase do dashboard dependem do canônico)
- Wave 3 independente de Wave 2 (pode rodar em paralelo)
- Wave 4 independente (saneamento de knowledge)
- Wave 5 (bugs de UI) independente das waves 1-4 — pode adiantar
- Wave 6 → depende de Wave 1 (cards), Wave 3 (retry no Quality), Wave 4 (Knowledge limpa)
- Sem dependência externa nova (sem npm/cargo install)

## Decisões

- **REVIEW vira fase reconhecida**, não sub-passo escondido de EXECUTE. Motivo: já emite 37
  eventos reais; escondê-la é que é o bug. Alternativa rejeitada: dobrar REVIEW dentro de
  EXECUTE — perderia granularidade que o dado já tem.
- **ANALYZE emitida pelo SKILL, não por hook.** Motivo: ANALYZE roda no parent antes de
  existir state file, então `pipeline-phase.js` (PostToolUse em Write/Edit) nunca a alcança.
  O SKILL é o único ponto que sabe que ANALYZE começou.
- **Atrito sai do `knowledge.json` (Opção B preferida).** Motivo: Knowledge é "padrões e
  lições"; contador de retry é telemetria. Misturar os dois foi o que tornou a página inútil.
- **Prompt Economy é fundida em Telemetria, não mantida como página irmã.** Motivo: é
  literalmente o drill-down de um card da Telemetria e lê a mesma fonte; duas páginas só
  geram divergência de número e confusão.
- **Home vira placar de ROI.** Motivo: o objetivo declarado do usuário é "provar que
  compensa usar o Mustard"; nenhuma tela hoje responde isso. Mecanismo (hooks, eventos) não
  é argumento de venda — comparação com-vs-sem é.

## Concerns

- **`agent.start` no sialia = 0** apesar de `subagent-tracker.js` emitir via `emitEvent`.
  Provável install antigo do Mustard no sialia OU nenhum Task despachado naquela sessão. Wave
  3 deve confirmar empiricamente antes de assumir que é bug de emissor.
- **`qa.result` no sialia = 0** porque nenhuma spec ativa chegou à fase QA — não é emissor
  faltando (`qa-run.js` emite). A página Quality precisa de empty-state honesto, não de
  emissor novo. Coberto na Wave 6.
- **`retry.attempt` retroativo**: specs antigas não terão o evento; o dashboard Quality só
  mostrará retries a partir de pipelines rodadas após a Wave 3. Aceitável — empty-state cobre.
- **Cross-repo**: a spec toca dois repos. EXECUTE precisa respeitar os `## Boundaries` de cada
  um; `boundary-gate` pode avisar em edições no `mustard-dashboard` se ele tiver spec própria.
- **`mustard.db` 16 min atrás de `events.jsonl`** no sialia — investigar na Wave 5 se o
  dashboard mistura fontes (JSONL vs SQLite) com tempos diferentes; pode exigir alinhar a
  leitura para uma fonte só.
- **`pass_at_1` está mal rotulado (achado da Wave 0).** `quality_metrics_from_db` em `db.rs`
  calcula `pass_at_1` como `specs completed / total specs` — não é pass@1 de QA. O rótulo
  "Acerto de 1ª tentativa" mente nas páginas Telemetria e Qualidade. Pior: `RoleQuality.pass_at_1`
  está hardcoded `0.0` no Rust. Wave 6 deve renomear o rótulo para o que o número realmente é
  (taxa de conclusão de specs) OU computar pass@1 real, e remover o hardcode.

## Non-Goals

- Não muda model routing nem custo por modelo (`feedback_no_routing_downgrade`).
- Não adiciona hook bloqueante novo.
- Não toca `entity-registry.json` nem `sync-registry.js`.
- Não introduz dependência npm/cargo.
- Não implementa cálculo de pricing local — USD vem do OTEL nativo do Claude Code.
- Não reescreve o coletor OTEL (entregue em `2026-05-15-honest-prompt-economy`); só consome.
