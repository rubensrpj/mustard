# Feature: b3-hooks-to-rust

### Status: implementing | Phase: EXECUTE | Scope: full
### Checkpoint: 2026-05-19T07:59:39Z
### Lang: pt

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

- [ ] Módulos `session_start`, `knowledge`, `session_cleanup`, `pre_compact`, `prompt_gate`.
- [ ] Colapsar `settings.json`: de entradas por hook para ~8 entradas `mustard-rt on <evento>`.
- [ ] Remover os `_lib/*.js` quando o último consumidor JS sair.

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
- **(Wave 2 → Wave 5)** Review da Wave 2 (APPROVED) levantou 2 WARNINGs, ambos resolvidos naturalmente no colapso de `settings.json` da Wave 5:
  1. **Profile gate perdido.** Ao consolidar, `review-gate`/`pr-detect` perderam o `shouldRun()` do `_lib/hook-env.js` — no profile `minimal` (que só permitia `bash-safety`/`file-guard`) eles agora rodam onde antes se auto-pulavam. Veredito não muda; é mudança de comportamento sob `minimal`. A Wave 5 deve dar ao dispatcher consciência de profile ou documentar a remoção.
  2. **`run_build` — timeout não mata o filho.** Em `bash_guard.rs`, no timeout do build o worker thread ainda está preso em `child.wait()`, então o `recv_timeout(0)` falha e `child.kill()` nunca roda → processo órfão (vazamento limitado: sai quando o build termina). Fail-open preservado (dispatcher não pendura). Conserto correto é redesenho de `run_build` (kill no timeout + drenagem concorrente dos pipes p/ evitar deadlock de buffer >64KB) — não cirúrgico. Caminho raramente alcançável: o timeout do harness em `settings.json` é 5s vs. `BUILD_TIMEOUT` 5min. Ao revisitar timeouts na Wave 5, redesenhar `run_build` ou subir o timeout da entrada PreToolUse(Bash).
- **(Wave 2, NOTE)** Eventos `commit-gate.check` logam `session_id: "unknown"` — o `Ctx` do lado `Check` não carrega `session_id` (o `Observer` do `pr-detect` usa `input.session_id` corretamente). Telemetria, não load-bearing; fix de 1 linha quando `Ctx` ganhar o campo.
- **(Wave 2, NOTE → Wave 5)** Docs/refs ainda citam `review-gate.js`/`pr-detect.js` por nome (`pipeline-config.md`, `commands/mustard/status` e `stats`, `adapters/cursor/README.md`, `scripts/metrics.js`). Não load-bearing (`metrics.js` usa `'review-gate'` só como chave de categoria). Varredura de docs cabe na limpeza da Wave 5.
- **(Wave 3 → Wave 5)** `subagent-tracker` portou só a emissão `agent.start`/`agent.stop` (verdict-free). O explorer-dedup (`deny` de 60s) e a medição de wave-slice ficaram de fora — dependem de `session_id`/`wave` no `Ctx`, que o contrato de `mustard-core` ainda não carrega. Portar quando o `Ctx` ganhar esses campos.
- **(Wave 3 → Wave 4)** `metrics-tracker`/`subagent-tracker` tagueiam eventos com `phase`/`spec` lidos do pipeline-state — sem acesso no `Ctx` atual; emitidos como `null` (igual ao fallback JS). Resolver quando B4 expuser o pipeline-state ao runtime.
- **(Wave 3, WARNING → Wave 1-2 cleanup)** `bash-native-redirect.js` e `rtk-rewrite.js` continuam em `templates/hooks/` mas já não são referenciados pelo `settings.json` (portados em `bash_guard` nas Waves 1-2): arquivos mortos. Deletar.
- **(Wave 3, WARNING → Wave 5)** `budget::observe` (`output-budget` advisory) escreve `hookSpecificOutput` direto no stdout via `println!`, contornando o `emit_outcome` do `main.rs` (dono único do protocolo stdout). Paridade de veredito preservada (advisory-only), mas sob o binário consolidado dois objetos JSON podem sair numa invocação. Rotear o aviso pelo `Outcome` no colapso da Wave 5.
- **(Wave 3, NOTE)** `now_iso8601` está duplicada verbatim em 5 módulos de `mustard-rt` (`bash_guard`, `budget`, `model_routing`, `tracker`, `skills_audit`). Candidato a helper em `mustard-core` — não load-bearing. **Wave 4 piorou:** mais 3 cópias (`path_guard`, `post_edit`, `close_gate`) — total 8. Idem `format_gate_message` (6 cópias) e `is_word_byte`. O helper em `mustard-core` ficou mais urgente, mas segue não load-bearing.

- **(Wave 4 → Wave 5/B4)** `boundary-gate` e `pipeline-phase` emitiam eventos (`boundary.expansion`, `pipeline.phase`) tagueados com `session_id`/`wave` resolvidos do pipeline-state via `_lib/harness-event.js`. O `Ctx` do `mustard-core` não carrega nenhum dos dois: o porte emite `session_id` = `input.session_id` (quando ausente → `"unknown"`) e `wave` = `0`/`null` — exatamente o fallback JS. Telemetria, não load-bearing; fix quando o `Ctx` ganhar `session_id`/`wave` (mesma dependência das Concerns de Wave 2/3).

- **(Wave 4, NOTE)** `boundary-gate.js` chamava `shouldRun('boundary-gate')` de `_lib/hook-env.js` (profile gate) — perdido na consolidação para `path_guard`, igual ao caso `review-gate`/`pr-detect` da Wave 2. Sob o profile `minimal` o gate agora roda onde antes se auto-pulava; veredito não muda. Mesma resolução: a Wave 5 dá ao dispatcher consciência de profile ou documenta a remoção. `file-guard.js` também usava `shouldRun`, idem.

- **(Wave 4, NOTE)** Encoding de wire normalizado: `enforce-registry.js` emitia `permissionDecision: "block"` e `guard-verify.js` emitia o protocolo PostToolUse `decision: "block"/"approve"`. O contrato `mustard-core` tem um único `Verdict::Deny` que o `emit_outcome` codifica como `"deny"`. O **veredito** (bloquear) é preservado 1:1 — só a string do wire normaliza; o harness trata `block`/`deny` de forma idêntica.

- **(Wave 4, WARNING → Wave 5)** `post_edit::observe` (auto-format) faz spawn de `npx prettier` / `dotnet format` de forma síncrona como side-effect do `Observer`. O `settings.json` dá 20s de timeout à entrada `post_edit` (auto-format antigo tinha 15s). Sob o binário consolidado, auto-format + checklist-auto-mark + pipeline-phase + guard-verify rodam numa só invocação — se o formatter pendurar, todo o `post_edit` espera. Fail-open preservado (cada side-effect engole erros). Revisitar timeouts no colapso da Wave 5.

- **(Wave 4, NOTE)** `close-gate.js` distinguia `envError` (spawn falho / timeout → fail-open, nunca bloqueia) de falha real (exit ≠ 0 → deny). O `bash_guard::run_build` tem shape diferente (e a Concern de vazamento de timeout da Wave 2); `close_gate` porta seu próprio `run_command` em vez de reusar — a distinção env-error/falha-real fica exata. `run_command` do `close_gate` tem o mesmo padrão de timeout que `run_build`, mas como o timeout do harness em `settings.json` (310s) é menor que o `COMMAND_TIMEOUT` de 5min, o caminho de timeout interno raramente é alcançado.

- **(Wave 4 review, NOTE → Wave 5)** **`boundary-gate` deixou de rodar em Write/Edit durante a janela de transição.** A entrada `PreToolUse(Write|Edit)` do `boundary-gate.js` foi deletada do `settings.json`, mas como `mustard-rt check <id>` roda só **um** módulo, e os blocos `PreToolUse(Write|Edit)` migrados apontam para `size_gate`/`close_gate`, o `path_guard` (que carrega o concern do boundary-gate) só é alcançado em `PreToolUse(Read)` (via `file-guard`). O `registry.rs` já registra `path_guard` em Write/Edit — mas só o colapso da Wave 5 (`mustard-rt on <evento>`) o ativará lá. Veredito preservado no modo default (`warn`); sob `MUSTARD_BOUNDARY_MODE=strict` um `deny` real fica perdido até a Wave 5. **A Wave 5 deve garantir que o colapso restaure o boundary-gate em Write/Edit.**

- **(Wave 4 review, WARNING)** `close_gate::find_last_qa_result` aceita um evento `qa.result` sem campo `spec` como satisfazendo o gate de QA para *qualquer* spec — um `qa.result` de outra execução pode dar falso-positivo. Paridade preservada: o `findLastQAResult` do JS tinha exatamente o mesmo comportamento frouxo. Apertar quando o `Ctx` ganhar identidade de spec.

- **(Wave 4 review, NOTE)** `path_guard::is_other_h2` não fecha a seção Files/Boundaries num heading `## ` seguido de espaço extra (`##  Título`) — o regex JS `/^##\s/` fecharia. Caso de borda (H2 com espaço duplo), afeta só seção advisory, sem impacto no caminho de `deny`.

## Critérios de Aceitação

- [ ] AC-1: `mustard-rt` compila — Command: `bash -c 'cargo build -p mustard-rt'`
- [ ] AC-2: Testes de paridade passam — Command: `bash -c 'cargo test -p mustard-rt'`
- [ ] AC-3: Os 3 hooks off foram deletados — Command: `node -e "const fs=require('fs');['duplication-check','convention-check','user-prompt-hint'].forEach(h=>{if(fs.existsSync('packages/cli/templates/hooks/'+h+'.js'))process.exit(1)})"`
- [ ] AC-4: O `settings.json` referencia `mustard-rt` — Command: `node -e "const fs=require('fs');if(!fs.readFileSync('packages/cli/templates/settings.json','utf8').includes('mustard-rt'))process.exit(1)"`

## Não-Objetivos

- Não portar scripts nem CLI (B4/B5).
- Não alterar o **veredito** de nenhum gate — consolidar agrupa, não muda decisão.
- Não fazer big-bang — migração incremental, família a família.
