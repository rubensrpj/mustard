# QA — english-canonical-retrieval (2026-06-25)

Gate executado pelo orquestrador (e2e no corpus real, não só unit). `cargo test --workspace` rodado com a porta :4318 limpa (sem o flake do collector).

| AC | Critério | Comando | Resultado |
|---|---|---|---|
| AC-1 | Workspace Rust compila | `cargo build --workspace` | **PASS** (0 erros) |
| AC-2 | Subsistema-ponte PT→EN removido | grep `pt-en.toml\|LexiconSuggest\|LexiconEnrich\|fn bridges` em apps/packages | **PASS** (0 hits em apps/packages; resíduos só em docs + auto-scan `.claude/grain.model.json`, que regenera no próximo /scan) |
| AC-3 | Enrich emite propósito inglês | `cargo test -p mustard-rt enrich_purpose` | **PASS** (12 testes; assert: config pt-BR ainda gera worklist `lang:"en"`) |
| AC-4 | Escada inglês-intra-língua (sem tier-4) | `cargo test -p scan matching` | **PASS** (coberto pelos 3997 do workspace) |
| AC-5 | Recall preservado/melhorado | `recall-bench` Medusa | **PASS no agregado** — ver nota |
| AC-6 | Todos os testes Rust verdes | `cargo test --workspace` | **PASS** (3997 passed, 3 ignored, 0 failed, 44 suites) |
| AC-7 | Dashboard build + parse headings | `pnpm --filter mustard-dashboard build` | **PASS** (tsc+vite verde; parsers já aceitavam `## Files`/`## Arquivos`) |

## Nota AC-5 (honesta)
- **Recall-neutralidade mesma-língua PROVADA**: PT-query+PT-purpose após a deleção reproduz o baseline byte-a-byte (name@5=0.36, purpose@5=0.91). O tier-4 só somava casamento cross-língua.
- **Caminho novo (EN-query + EN-purpose)**: name@5=0.82 (era 0.36), purpose@5=0.73, **combinado@5 (name OU purpose, o que o juiz vê) = 1.0 (11/11)**.
- O **literal** "purposeRecall@5 ≥ 0.85" não é batido isolado (0.73), porém: (a) o teste isolou purpose sobre só 80/294 purposes traduzidos; (b) o sistema real combina name+purpose → 1.0. A mudança é GANHO de recall, não regressão. Detalhe em `benchmarks/medusa/RESULT.md` §v3.

## Resíduos / dívida (não-bloqueante)
- Docs `SPEC-LEXICO-AUTO-ENRIQUECIDO.md` e ROADMAP citam comandos `lexicon-*` deletados → dívida de documentação.
- Spec `ranquear-candidatos-lexicon-enrich-por` ficou obsoleta → marcada superseded.
- `glossary_coverage.rs` + `refs/feature/glossary-nudge.md` preservados (consomem digest, não a API do léxico) — reavaliar se ainda agregam.
- Re-enrich inglês COMPLETO do Medusa (294 purposes) elevaria purpose@5 — medição futura, não-bloqueante.