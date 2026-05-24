# Feature: b3-hooks-to-rust

> Spec de backlog (Parte B, item B3). **ÉPICO** — decompõe no ANALYZE em waves por família de hook (provavelmente specs-filhas). Depende de B2. Revisada 2026-05-18: **não é porte 1:1** — os 37 hooks viram ~15 módulos atrás de um dispatcher.

## Contexto

Hoje os 37 hooks do Mustard são arquivos `.js` copiados para o `.claude/` de cada projeto e executados via `node`/`bun`. Isso cria uma classe de bug: se o runtime não está no PATH, os hooks falham em silêncio. E hooks rodam em todo tool-use — cada `Write/Edit` dispara 15 processos `bun` separados (8 PreToolUse + 7 PostToolUse), cada um pagando ~40-80 ms de cold-start de interpretador no caminho crítico. Portar para um binário Rust único (`mustard-rt`) elimina a dependência de runtime e derruba o cold-start para ~1 ms. Mas portar 1:1 — um módulo por hook — preservaria a fragmentação: 37 conceitos que ninguém raciocina inteiros. A migração é a janela certa para consolidar: os 37 hooks são, na verdade, ~15 concerns reais sobre 9 `_lib` compartilhados. Portar já consolidado custa o mesmo esforço e entrega um sistema raciocinável.

## Resumo

Construir o crate binário `packages/rt` (`mustard-rt`): um **dispatcher** que recebe o evento do harness, lê o stdin JSON e roda os **~15 módulos de enforcement** aplicáveis (cada um implementando `Check` ou `Observer` de `mustard-core`), consolidando tudo num único `Outcome` — uma saída stdout, um exit code. Os 3 hooks off-by-default (`duplication-check`, `convention-check`, `user-prompt-hint`) são deletados, não portados. O `settings.json` migra de forma incremental: de 47 entradas para ~8 (uma por evento). O dispatcher é também o ponto único de log estruturado que o dashboard consome.

## Entidades

N/A — infraestrutura de enforcement.

## Component Contract

N/A.

## Arquitetura

`mustard-rt` é um binário com dois rostos: `mustard-rt on <evento>` (hooks) e `mustard-rt <script>` (scripts, B4). O dispatcher:

1. Lê `HookInput` do stdin — qualquer erro → `Allow` (fail-open central, não replicado por hook).
2. Consulta o `Registry` (indexado por evento+tool) → só os módulos aplicáveis rodam.
3. Para cada módulo: `Observer` roda fire-and-forget; `Check` produz um `Verdict`, dobrado num `Outcome` segundo o `mode` (off/warn/strict) da config.
4. Escreve um stdout JSON e um exit code.

SOLID: **S** um módulo por concern; **O** adicionar check = registrar módulo, dispatcher imutável; **L** todo check uniforme via `Verdict`; **I** `Check` vs `Observer` separados; **D** módulos dependem das traits de `core`.

As ~15 famílias (a confirmar no ANALYZE):

| Módulo | Hooks consolidados |
|---|---|
| `bash_guard` | bash-safety, bash-native-redirect, rtk-rewrite, review-gate, pr-detect |
| `budget` | context-budget, output-budget, caps de tool-use/main-context |
| `size_gate` | spec-size-gate, skill-size-gate, skill-validate-gate |
| `path_guard` | boundary-gate, file-guard |
| `close_gate` | close-gate (sozinho — sensor real, 645 LOC) |
| `post_edit` | auto-format, checklist-auto-mark, guard-verify, pipeline-phase |
| `tracker` | tool-use-counter, main-context-counter, subagent-tracker, metrics-tracker, skill-usage-tracker (Observer) |
| `model_routing` | model-routing-gate |
| `enforce_registry` | enforce-registry |
| `skills_audit` | recommended-skills-audit |
| `session_start` | harness-init, session-memory, spec-hygiene |
| `knowledge` | session-knowledge, session-knowledge-inc, memory-auto-extract |
| `session_cleanup` | session-cleanup |
| `pre_compact` | pre-compact |
| `prompt_gate` | followup-cancel-gate |

## Arquivos

- `packages/rt/Cargo.toml`, `packages/rt/src/main.rs` — parse de subcomando
- `packages/rt/src/dispatch.rs` — `run_event()`: fail-open central, fold de `Verdict`
- `packages/rt/src/registry.rs` — `Registry` indexado por (evento, tool)
- `packages/rt/src/hooks/*.rs` — ~15 módulos de enforcement (um por concern)
- `packages/cli/templates/settings.json` — migrar entradas para `mustard-rt`
- `packages/cli/templates/hooks/*.js` — removidos conforme portados; os 3 off deletados de cara
- `Cargo.toml` raiz — registrar `packages/rt`

## Limites

- `packages/rt/`, `packages/cli/templates/settings.json`, `packages/cli/templates/hooks/`
- **Fora dos limites:** scripts (B4), CLI (B5). A lógica de **decisão** de cada gate é preservada — muda a linguagem e o agrupamento, não o veredito.

## Tarefas

> Estrutura provisória — o ANALYZE define as waves reais por família e provavelmente specs-filhas.

### Impl Agent (Wave 0) — poda pré-porte

- [x] Deletar `duplication-check.js`, `convention-check.js`, `user-prompt-hint.js` e os 3 `MUSTARD_*_MODE` correspondentes do `settings.json`. Código off não é portado.

### Impl Agent (Wave 1) — dispatcher + registry + família `bash_guard`

- [x] Esqueleto de `mustard-rt`: parse de subcomando, `HookInput::from_stdin`, fail-open global, `Outcome` único.
- [x] `Registry` + dispatch por evento.
- [x] Módulo `bash_guard` (porta 3 dos 5: `bash-safety`, `bash-native-redirect`, `rtk-rewrite`); migração incremental do `settings.json`; 28 testes de paridade. `review-gate`/`pr-detect` seguem JS — ver Concerns.

### Impl Agent (Wave 2) — `bash_guard`-cont (fecha a família Bash 5/5)

- [x] Portar `review-gate` (gate PreToolUse(Bash) em `git commit`: segredo staged / build quebrado) para o módulo `bash_guard` como mais um gate do `Check`. Veredito computado com o modo próprio `MUSTARD_COMMIT_GATE_MODE` (default `warn`).
- [x] Portar `pr-detect` (observer PostToolUse(Bash) DORA) — `bash_guard` ganha um `Observer` e a entrada de registry `(PostToolUse, Bash)`; emite `pr.opened`/`pr.merged` via `core::io::EventSink`.
- [x] Migrar `settings.json`: PreToolUse(Bash) e PostToolUse(Bash) passam a `mustard-rt check bash_guard`; deletar `review-gate.js` e `pr-detect.js`.
- [x] Testes de paridade Rust contra os casos JS de `review-gate`/`pr-detect`. Review APPROVED (0 CRITICAL); `cargo test -p mustard-rt` 38 ✓.

### Impl Agent (Wave 3) — famílias de Task/Subagent

- [x] Módulos `budget`, `model_routing`, `tracker`, `skills_audit`. 9 hooks JS portados; `settings.json` migrado; 9 `.js` deletados; testes de paridade Rust (107 ✓) + suíte JS verde (fix-loop 1 limpou blocos órfãos em `harness-wave*.test.js`). Review APPROVED (0 CRITICAL).

### Impl Agent (Wave 4) — famílias de Write/Edit

- [x] Módulos `size_gate`, `path_guard`, `post_edit`, `close_gate`, `enforce_registry`. 11 hooks JS portados; `settings.json` migrado; 11 `.js` deletados; testes de paridade Rust (196 ✓) + suíte JS verde (232 ✓, fix-loop limpou blocos órfãos em `hooks.test.js`/`harness-dual-emission`/`harness-wave10` e deletou `size-gates.test.js`/`skill-validate-gate.test.js`/`harness-wave9.test.js`).

### Impl Agent (Wave 5) — famílias de sessão + colapso final

- [x] Módulos `session_start`, `knowledge`, `session_cleanup`, `pre_compact`, `prompt_gate`. 9 hooks JS portados; testes de paridade Rust (`cargo test -p mustard-rt` 244 ✓ — inclui fix-loop da review, ver Concern abaixo).
- [x] Colapsar `settings.json`: 8 entradas `mustard-rt on <evento>` (uma por evento de ciclo de vida). O colapso restaura `path_guard`/boundary-gate em `PreToolUse(Write|Edit)` — `on PreToolUse` roda TODOS os módulos registrados; verificado pelo teste de registry `write_edit_family_applies_on_pre_tool_use`.
- [x] Limpeza: 12 `.js` deletados de `templates/hooks/` (9 da Wave 5 + `bash-native-redirect`/`rtk-rewrite` mortos + `bash-safety`); 3 `_lib/*.js` órfãos deletados (`knowledge-extract`/`gate-message`/`size-gate`); `_lib/{harness-event,hook-env,runtime-shim,event-store,metrics-emit}.js` MANTIDOS — ainda consumidos por scripts B4 (Concern). Suíte JS verde (187 ✓ em `templates/hooks/__tests__/`); `integration.test.js` deletado (100% órfão), blocos órfãos removidos de `hooks.test.js`/`harness-wave3.test.js`/`harness-dual-emission.test.js`.

## Dependências

- B2 (`mustard-core`) — o contrato (`Check`/`Observer`/`Verdict`) e a infra. Esperar a Wave 1 de B2 (contrato congelado).
- B1 (monorepo) — concluído.

## Preocupações

- **Volume real:** 37 hooks → ~15 módulos. Épico; o ANALYZE produz specs-filhas por família.
- **Paridade:** os testes JS (`hooks/__tests__/`) são o oráculo. Consolidar N hooks num módulo não pode mudar nenhum veredito — fixtures golden rodam contra o JS antigo e o módulo Rust.
- **`settings.json` misto:** durante a transição, `node` e `mustard-rt` coexistem. O dispatcher aceita `mustard-rt check <id>` (um check) e `mustard-rt on <evento>` (evento inteiro) para permitir migração entrada-a-entrada antes do colapso.
- **Log estruturado:** o dispatcher emite um evento por execução (via `core::io::EventSink`) — é o que o dashboard mapeia. Não criar pipe de log novo; reusar `events.jsonl`.
- **Loop de dev mais lento:** editar hook passa a exigir `cargo build`. Aceito — conjunto estável.

## Concerns

- **(Wave 0 → B4)** Ao deletar `duplication-check`/`convention-check`, ficou órfão o trecho de `event-projections.js:647-650` (`buildSlopeReport`) que conta eventos `duplication.warn`/`convention.warn` — nenhum hook emite mais esses eventos. Não bloqueia B3; remover/ajustar quando B4 portar `event-projections`.
- **(Wave 1 → Wave 2)** ~~`bash_guard` portou 3 dos 5 hooks de Bash.~~ **Resolvido:** Wave 2 (`bash_guard`-cont) porta `review-gate` e `pr-detect`, fechando a família Bash 5/5. Wave inserida em 2026-05-19 via `/resume`; waves seguintes renumeradas 3/4/5.
- **(Wave 2 → Wave 5)** Review da Wave 2 (APPROVED) levantou 2 WARNINGs:
  1. **Profile gate perdido. RESOLVIDO por documentação (Wave 5).** `shouldRun()`/`MUSTARD_HOOK_PROFILE` do `_lib/hook-env.js` não foi portado para o dispatcher. **Decisão consciente:** o profile `minimal` só permitia `bash-safety`/`file-guard` — ambos hoje são `Check`s de veredito puro dentro de `bash_guard`/`path_guard`. Dar consciência de profile ao dispatcher exigiria reintroduzir a tabela `PROFILES` e um gate por-módulo; o ganho é nulo para os módulos `Check` (rodar um gate a mais que se auto-pularia não muda veredito sob `minimal` se o input é benigno) e os módulos `Observer` portados são todos fail-open sem efeito de veredito. O profile gate é portanto **removido de propósito** — `MUSTARD_HOOK_PROFILE` deixa de ter efeito; `MUSTARD_DISABLED_HOOKS` não foi portado tampouco (mesmo raciocínio). Se um kill-switch por-módulo for necessário no futuro, o ponto natural é `registry::mode_for` (já existe e o dispatcher já honra `Mode::Off`).
  2. **`run_build` — timeout não mata o filho.** Permanece (Wave 2). O colapso da Wave 5 deu à entrada `PreToolUse` timeout 310s (≥ o caminho `close_gate` mais lento); `BUILD_TIMEOUT` interno de `bash_guard::run_build` é 5min. O caminho de vazamento interno segue raramente alcançável e o redesenho de `run_build` continua não-cirúrgico — **deferido**, sem mudança de veredito.
- **(Wave 2, NOTE)** Eventos `commit-gate.check` logam `session_id: "unknown"` — o `Ctx` do lado `Check` não carrega `session_id` (o `Observer` do `pr-detect` usa `input.session_id` corretamente). Telemetria, não load-bearing; fix de 1 linha quando `Ctx` ganhar o campo.
- **(Wave 2, NOTE → Wave 5) RESOLVIDO (Wave 5).** Varredura de docs feita: `templates/pipeline-config.md`, `commands/mustard/status` e `stats`, `adapters/cursor/README.md`, `templates/CLAUDE.md` e o `CLAUDE.md` raiz tiveram os nomes `.js` stale trocados pelos módulos `mustard-rt` (só nomes, sem mudança de comportamento). `scripts/metrics.js` **não foi alterado**: as strings `'review-gate'`/`'rtk-rewrite'`/etc. são chaves de categoria de eventos de métrica históricos, não referências a hooks — é B4 e a tabela é não-load-bearing. Specs completas e `.claude/plans/` ficaram intocadas (arquivos históricos).
- **(Wave 3 → Wave 5)** `subagent-tracker` portou só a emissão `agent.start`/`agent.stop` (verdict-free). O explorer-dedup (`deny` de 60s) e a medição de wave-slice ficaram de fora — dependem de `session_id`/`wave` no `Ctx`, que o contrato de `mustard-core` ainda não carrega. Portar quando o `Ctx` ganhar esses campos.
- **(Wave 3 → Wave 4)** `metrics-tracker`/`subagent-tracker` tagueiam eventos com `phase`/`spec` lidos do pipeline-state — sem acesso no `Ctx` atual; emitidos como `null` (igual ao fallback JS). Resolver quando B4 expuser o pipeline-state ao runtime.
- **(Wave 3, WARNING → Wave 1-2 cleanup) RESOLVIDO (Wave 5).** `bash-native-redirect.js` e `rtk-rewrite.js` deletados de `templates/hooks/` no colapso da Wave 5.
- **(Wave 3, WARNING → Wave 5) RESOLVIDO (Wave 5).** `budget::observe` foi eliminado: `output-budget` agora flui pelo `Check` — em `PostToolUse(Task)` o `BudgetGuard::evaluate` emite a métrica e retorna `Verdict::Inject` com o advisory; o dispatcher dobra o `Inject` no único `Outcome` e o `emit_outcome` faz a única escrita de stdout. `BudgetGuard` não implementa mais `Observer`; a entrada de registry `budget` passou a `observer: None`. Uma invocação emite exatamente um objeto JSON.
- **(Wave 3, NOTE) RESOLVIDO (Wave 5).** `now_iso8601` (8 cópias) e `format_gate_message` (6 cópias) extraídos para `crate::util` **dentro de `mustard-rt`** (`packages/rt/src/util.rs`) — `mustard-core` (B2) intocado. Os 9 módulos de hooks fazem `use crate::util::{now_iso8601, format_gate_message}`. `is_word_byte` restou em uma única cópia (`post_edit`) — não é mais duplicação.

- **(Wave 4 → Wave 5/B4)** `boundary-gate` e `pipeline-phase` emitiam eventos (`boundary.expansion`, `pipeline.phase`) tagueados com `session_id`/`wave` resolvidos do pipeline-state via `_lib/harness-event.js`. O `Ctx` do `mustard-core` não carrega nenhum dos dois: o porte emite `session_id` = `input.session_id` (quando ausente → `"unknown"`) e `wave` = `0`/`null` — exatamente o fallback JS. Telemetria, não load-bearing; fix quando o `Ctx` ganhar `session_id`/`wave` (mesma dependência das Concerns de Wave 2/3).

- **(Wave 4, NOTE) RESOLVIDO (Wave 5).** O profile gate (`shouldRun`/`MUSTARD_HOOK_PROFILE`) de `boundary-gate.js`/`file-guard.js` foi removido de propósito — ver a resolução da Concern de Wave 2 acima (item 1): o profile gate não foi portado para o dispatcher; `MUSTARD_HOOK_PROFILE`/`MUSTARD_DISABLED_HOOKS` deixam de ter efeito. Kill-switch por-módulo, se necessário, vai por `registry::mode_for` (`Mode::Off`).

- **(Wave 4, NOTE)** Encoding de wire normalizado: `enforce-registry.js` emitia `permissionDecision: "block"` e `guard-verify.js` emitia o protocolo PostToolUse `decision: "block"/"approve"`. O contrato `mustard-core` tem um único `Verdict::Deny` que o `emit_outcome` codifica como `"deny"`. O **veredito** (bloquear) é preservado 1:1 — só a string do wire normaliza; o harness trata `block`/`deny` de forma idêntica.

- **(Wave 4, WARNING → Wave 5) RESOLVIDO (Wave 5).** Timeouts revisitados no colapso: a entrada consolidada `PostToolUse` recebeu 30s (auto-format antigo tinha 15s, a entrada `post_edit` da janela de transição tinha 20s). Sob `on PostToolUse` rodam `bash_guard`/`budget`/`knowledge`/`tracker`/`post_edit` numa só invocação; 30s cobre o spawn síncrono do formatter (`npx prettier`/`dotnet format`) com folga. Fail-open preservado (cada side-effect engole erros). O spawn do formatter continua síncrono — redesenho assíncrono não é cirúrgico e fica fora de escopo.

- **(Wave 4, NOTE)** `close-gate.js` distinguia `envError` (spawn falho / timeout → fail-open, nunca bloqueia) de falha real (exit ≠ 0 → deny). O `bash_guard::run_build` tem shape diferente (e a Concern de vazamento de timeout da Wave 2); `close_gate` porta seu próprio `run_command` em vez de reusar — a distinção env-error/falha-real fica exata. `run_command` do `close_gate` tem o mesmo padrão de timeout que `run_build`, mas como o timeout do harness em `settings.json` (310s) é menor que o `COMMAND_TIMEOUT` de 5min, o caminho de timeout interno raramente é alcançado.

- **(Wave 4 review, NOTE → Wave 5) RESOLVIDO (Wave 5).** O colapso restaurou `boundary-gate` em Write/Edit: a entrada `PreToolUse` agora é uma só (`matcher: ".*"` → `mustard-rt on PreToolUse`), e `run_event(PreToolUse, …)` roda **todos** os módulos registrados pelo `Registry` para o evento+tool. `path_guard` está registrado em `(PreToolUse, Write)` e `(PreToolUse, Edit)`, então roda lá. Verificado pelo teste de registry `write_edit_family_applies_on_pre_tool_use` e pelo novo `wave5_session_families_apply_to_their_events`. Sob `MUSTARD_BOUNDARY_MODE=strict` o `deny` volta a ser entregue.

- **(Wave 4 review, WARNING)** `close_gate::find_last_qa_result` aceita um evento `qa.result` sem campo `spec` como satisfazendo o gate de QA para *qualquer* spec — um `qa.result` de outra execução pode dar falso-positivo. Paridade preservada: o `findLastQAResult` do JS tinha exatamente o mesmo comportamento frouxo. Apertar quando o `Ctx` ganhar identidade de spec.

- **(Wave 4 review, NOTE)** `path_guard::is_other_h2` não fecha a seção Files/Boundaries num heading `## ` seguido de espaço extra (`##  Título`) — o regex JS `/^##\s/` fecharia. Caso de borda (H2 com espaço duplo), afeta só seção advisory, sem impacto no caminho de `deny`.

- **(Wave 5, NOTE → B4)** `templates/hooks/_lib/{harness-event,hook-env,runtime-shim,event-store,metrics-emit}.js` **não foram deletados** apesar de nenhum hook `.js` os consumir mais — eles ainda são `require()`-d por scripts B4 (`epic-fold.js`, `spec-link.js`, `review-result.js`, `qa-run.js`, `memory.js`, `scripts/_lib/event-store.js`) e pelo comando `commands/mustard/review/SKILL.md`. `harness-event.js` puxa `hook-env.js` que puxa `runtime-shim.js`, e `harness-event.js` puxa `event-store.js` — a árvore inteira é mantida. Quando B4 portar esses scripts para `mustard-rt`, esses 5 `_lib` ficam órfãos e devem ser deletados então. Só os 3 verdadeiramente órfãos foram removidos na Wave 5 (`knowledge-extract.js`, `gate-message.js`, `size-gate.js`).

- **(Wave 5, NOTE → B-adapters)** O adapter experimental do Cursor (`templates/adapters/cursor/`) roteia para `.claude/hooks/{name}.js` — arquivos que o porte b3 deletou. O `adapter.js` está quebrado para os hooks de enforcement; o `README.md` do adapter foi atualizado (varredura de docs) com um NOTE e a coluna `mustard-rt module`, mas o `adapter.js` em si **não foi alterado** — `adapters/` está fora dos Limites de b3. Consertar (rotear via `mustard-rt on <evento>`) é uma tarefa de adapters, não de b3.

- **(Wave 5 review, WARNING → RESOLVIDO via fix-loop)** A review da Wave 5 pegou uma divergência de paridade em `pre_compact::has_active_pipeline`: um diretório `.pipeline-states/` presente mas vazio (zero `.json`) levava o porte Rust a tirar snapshot, enquanto `pre-compact.js` saía em silêncio (`activeStates.length === 0 → process.exit(0)`). Corrigido no fix-loop 1/2: `has_active_pipeline` retorna `false` incondicionalmente quando o dir existe sem estado `active`/`implementing` — dir vazio, dir só-não-JSON e JSON-sem-nenhum-ativo colapsam todos no caminho silencioso, igual ao JS. Teste de paridade `empty_states_dir_is_silent_allow` adicionado; suíte Rust 243 → 244 ✓. Advisory-only (`Inject`/`Allow`, nunca `Deny`) — não era classe-veredito, mas paridade é o oráculo do épico e foi consertada antes do CLOSE.

- **(Wave 5, NOTE)** `mustard-rt on SessionStart` não faz spawn do coletor OTEL que o antigo `harness-init.js` iniciava — o spawn dependia do script B4 `scripts/otel-collector.js` (fora dos Limites). As env `OTEL_*` no `settings.json` seguem dirigindo a telemetria própria do harness. `session_cleanup` ainda remove um `.otel-collector.pid` stale (para não orfanar um coletor iniciado por uma instalação JS legada), mas não envia sinal — não há API de sinal portável sem dependência. Portar o spawn quando B4 portar `otel-collector.js`.

## Critérios de Aceitação

- [x] AC-1: `mustard-rt` compila — Command: `bash -c 'cargo build -p mustard-rt'`
- [x] AC-2: Testes de paridade passam — Command: `bash -c 'cargo test -p mustard-rt'`
- [x] AC-3: Os 3 hooks off foram deletados — Command: `node -e "const fs=require('fs');['duplication-check','convention-check','user-prompt-hint'].forEach(h=>{if(fs.existsSync('packages/cli/templates/hooks/'+h+'.js'))process.exit(1)})"`
- [x] AC-4: O `settings.json` referencia `mustard-rt` — Command: `node -e "const fs=require('fs');if(!fs.readFileSync('packages/cli/templates/settings.json','utf8').includes('mustard-rt'))process.exit(1)"`

## Não-Objetivos

- Não portar scripts nem CLI (B4/B5).
- Não alterar o **veredito** de nenhum gate — consolidar agrupa, não muda decisão.
- Não fazer big-bang — migração incremental, família a família.
