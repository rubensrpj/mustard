# Tactical Fix: qa-run parseia o formato de AC do drafter + alinha doc de migrate_to_meta

## Contexto

Tactical fix derivado de [[2026-06-02-spec-scaffold-lifecycle-gate]].

Ao rodar a fase QA do parent, o `qa-run` retornou `overall: skip` com `WARN: Acceptance Criteria section found but no parseable AC items` — não conseguiu extrair nenhum critério. A causa-raiz está em `apps/rt/src/commands/review/qa_run.rs::parse_ac_line` (~105), que casa **apenas** o formato histórico `- [ ] AC-N: descrição — Command: \`cmd\`` (tudo numa linha). O formato canônico que o drafter de spec emite hoje é diferente em **três** pontos:

1. **Sem checkbox** — a seção `## Critérios de Aceitação` não é um checklist; cada item é `- **AC-N** — …`, sem `[ ]`. O parser exige `[ ]` em `strip_prefix('[')?`.
2. **Separador travessão** — o drafter separa id e descrição com `—` (em-dash); o parser só aceita `.`/`:`.
3. **`Command:` em linha separada/indentada** — o drafter põe o comando na linha seguinte (`  Command: \`cmd\``); o parser só procura o marcador `command:` na mesma linha do AC.

Resultado: specs geradas pelo tool não têm seu QA registrado deterministicamente — o `qa.result` exigido pelo close-gate nunca é emitido, forçando `--allow-no-qa` (como aconteceu no parent). Conserto é no **tool** (o parser), nunca na spec ([[feedback-mustard-fix-tool-not-spec]]).

Loose-end adjacente da Frente 2 do parent: o doc-comment de `migrate_to_meta.rs` (~24-25) ainda afirma que cria um sidecar `meta.json` ao lado de cada `*.md` (incluindo `qa/spec.md`/`review/spec.md`), mas o código agora pula essas pastas via `is_phase_dir`. Alinhar a prosa ao comportamento.

Fora de escopo (decidido): guard de "cargo test casou 0 testes não passa vacuamente" — exige heurística sobre a saída do cargo (frágil, risco de falso-negativo) e beira decisão de design; melhor tratado à parte. E a tensão `FullScopeNoWaves` × Full-não-decomposto (`contract.rs:280`) é decisão de design, não tactical-fix.

## Critérios de Aceitação

- [ ] AC-1: qa-run extrai o item quando nao ha checkbox, o separador e travessao e o comando esta na linha indentada seguinte (formato do drafter) — Command: `cargo test -p mustard-rt -- qa_run`
- [ ] AC-2: o formato historico (checkbox `[ ]` + separador `:` + comando na mesma linha) continua sendo extraido sem regressao — Command: `cargo test -p mustard-rt -- qa_run`
- [ ] AC-3: o crate rt compila, testa e linta verde — Command: `cargo test -p mustard-rt && cargo clippy -p mustard-rt`

## Arquivos

- `apps/rt/src/commands/review/qa_run.rs` — `parse_ac_items`/`parse_ac_line` (~94-207): aceitar AC sem `[ ]`, separador `—`/`-` além de `.`/`:`, e `Command:` numa linha indentada seguinte (lookahead em `parse_ac_items`); manter back-compat total com o formato histórico; novos testes cobrindo ambos os formatos.
- `apps/rt/src/commands/migrate/migrate_to_meta.rs` — alinhar o doc-comment (~24-25) ao comportamento atual (ignora `qa/`/`review/`).

<!-- wikilinks-footer-start -->
- [2026-06-02-spec-scaffold-lifecycle-gate](?) ⚠ unresolved
- [feedback-mustard-fix-tool-not-spec](?) ⚠ unresolved
<!-- wikilinks-footer-end -->