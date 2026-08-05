# Re-review — packages/core (após o ciclo de correção)

## VERDICT: APPROVED — 0 blocking findings

Guards do core mantidos: `domain/spec/contract.rs` e `domain/meta.rs` continuam livres de IO; o único caminho de escrita é `write_meta` -> `io::fs::write_atomic`; nenhum `unwrap`/`expect` fora de `#[cfg(test)]`; a mudança em `Meta` é puramente aditiva. Nenhum molde do core cobre `domain/spec/**` — verificado, não presumido.

## Acceptance Criteria — cada comando rodado, saída real

Todos os oito verdes, com controle verde ao lado onde declarado. `cargo test --workspace` → 0 failed.

## Prova ponta a ponta contra o binário compilado

- Prosa real de revisor: o coletor sobre cópias dos próprios `review/findings-*.md` desta spec rendeu **8 registros distintos**, um por cabeçalho com severidade; marcadores de resultado de AC corretamente não cunhados.
- **O CRÍTICO da rodada anterior está corrigido:** achado da rodada 1 roteado `--to dropped`, `findings.md` sobrescrito com a rodada 2, `emit-phase --to CLOSE` → **exit 1**, nomeando `F-findings-1531cd9f` e a linha exata de `mark-finding`; a decisão da rodada 1 sobreviveu no registro. Antes: exit 0 com rota herdada.
- **O MAIOR da rodada anterior está corrigido:** fonte que para de reportar mantém o registro roteado (`retained`) e descarta só os indecisos (`stale`).
- **AC-8 no caminho real:** `close-orchestrate` emitiu linha `"name": "close-gates", "ok": false` carregando a recusa verbatim, `overall: "fail"`, `"chained": false`. Controle positivo: depois do `mark-finding --to queued`, a mesma linha voltou `ok: true`.
- **Nenhum dano ao sidecar:** rodar o coletor sobre o `meta.json` real acrescentou só a chave `findings`; todas as outras linhas byte-idênticas.

## MAJOR — o portão descarta o `ok` do próprio coletor

`apps/rt/src/commands/pipeline/close_gates.rs:572` — `open_findings` descarta `FindingCollectReport::ok`. Reproduzido ao vivo: um diretório de spec com `review/findings.md` e **sem** `meta.json` recusa o CLOSE, enquanto a remediação impressa e o seu fallback falham os dois (`error: spec "demo" carries no readable meta.json…`, `"error": "meta-not-found"`). Só `MUSTARD_FINDINGS_GATE_MODE=warn` escapa. Alcance baixo, mas é a terceira revisão seguida que o aponta.

## MINOR — os três testes do change request não são nomeados por AC

`.claude/spec/every-finding-must-have-declared/spec.md:53` — as três sub-requisições do change request de 21:26 são cobertas só pela suíte do AC-7; nenhum AC nomeia seus testes, contra a convenção que os outros sete critérios seguem.

## MINOR — AC-8 sem linha de Control

`.claude/spec/every-finding-must-have-declared/spec.md:59` — AC-8 não declara `Control:`, diferente de AC-1..AC-6.
