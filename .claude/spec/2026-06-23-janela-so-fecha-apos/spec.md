# Tactical Fix: janela so fecha apos primeira leitura (flag touched) — Bash do feature nao auto-fecha

## Contexto

Tactical fix derivado de [[consertar-loop-outcome-digest-janela]].

**Defeito (encontrado por leitura de runtime, mascarado pelos ACs unitários da pai).** O observer `feature_outcome_observer.rs` fecha a janela em QUALQUER tool não-Read/Edit/Write (`is_research_tool` = só `Read|Edit|Write`). Mas o comando `mustard-rt run feature` roda VIA o tool **Bash** — então o `PostToolUse(Bash)` do próprio comando que ABRIU a janela apaga o marcador `active-research.json` ANTES de qualquer leitura de anchor. Resultado prático: quase ZERO `feature.outcome` gravados (pior que o bug original). Os Bash de ANALYZE (`glossary-coverage`, `scan`, `digest-adherence-finalize`) fechariam a janela cedo do mesmo jeito. Os AC-3/4/5 da pai passaram porque chamam `correlate_and_emit_at`/`close_window_at` direto com marcadores fabricados — nenhum teste exercita a ordem real Bash→Read.

**Conserto.** Adicionar um flag `touched` (bool) ao marcador, gravado `false` na abertura e setado `true` na PRIMEIRA emissão de outcome (1ª leitura dentro da janela). O `close_window` só remove o marcador num tool não-research SE `touched == true`. Assim o Bash do próprio `feature` (e os Bash de ANALYZE antes da 1ª leitura) NÃO fecham a janela; depois que o orquestrador lê os anchors (`touched=true`), o primeiro tool diferente fecha. `expires_at` (expiração por idade) e o overwrite por nova query ficam como estão.

## Critérios de Aceitação

- **AC-1** — Build verde
  Command: `cargo build`
- **AC-2** — Suite completa verde (sem regressão)
  Command: `cargo test`
- **AC-3** — Um tool não-research com `touched=false` NÃO fecha a janela (simula o Bash do `feature` logo após a abertura: o marcador sobrevive)
  Command: `cargo test window_not_closed_before_first_read`
- **AC-4** — Após uma leitura (`touched` vira `true`), um tool não-research FECHA a janela
  Command: `cargo test window_closes_after_first_read_then_nonresearch`
- **AC-5** — Sequência ponta a ponta no nível do `observe()`: abre → Bash (não fecha) → Read (conta + `touched`) → Bash (fecha) → Read (não conta)
  Command: `cargo test outcome_window_lifecycle_open_read_close`

## Arquivos

- `apps/rt/src/commands/feature.rs` — `active_research_marker` passa a gravar `touched: false` na abertura (campo aditivo no JSON do marcador).
- `apps/rt/src/hooks/observe/feature_outcome_observer.rs` — setar `touched=true` e persistir de volta no marcador na 1ª emissão de outcome; gatear `close_window` em `touched==true` (ler o flag antes de remover). Adicionar os testes dos AC-3/4/5, sendo o AC-5 ao nível de `observe()` (não só dos cores explícitos).

<!-- wikilinks-footer-start -->
- [consertar-loop-outcome-digest-janela](?) ⚠ unresolved
<!-- wikilinks-footer-end -->