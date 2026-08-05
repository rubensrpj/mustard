# Re-review — apps/rt (após o ciclo de correção)

## VERDICT: APPROVED — 0 blocking findings

Guards todos mantidos; clippy 0 warnings nos arquivos tocados; as QUATRO registrações presentes para os dois comandos novos. Moldes `rt-cmd-pattern`, `rt-report-pattern` e `rt-outcome-pattern` conferidos um a um.

## Acceptance Criteria — cada comando rodado, saída real

| AC | Comando | Resultado |
|---|---|---|
| 1 | `cargo test -p mustard-core finding_item` | 5 passed |
| 2 | `…finding_collect_reads_both_sources` | 1 passed x2 |
| 3 | `…finding_collect_preserves_declared_route` | 1 passed x2 |
| 4 | `…findings_gate_denies_open_finding` | 1 passed x2 |
| 5 | `…findings_gate_allows_when_every_finding_routed` | 1 passed x2 |
| 6 | `…mark_finding_records_route_and_refuses_without_reason` | 1 passed x2 |
| 8 | `…close_orchestrate_blocks_on_open_finding` | 1 passed x2 |
| 7 | `cargo test --workspace` | exit 0, 4765 passed |

## Prova ponta a ponta contra dados REAIS

Copiou os próprios `review/findings-apps-rt.md` + `findings-packages-core.md` desta unidade para um projeto de rascunho: o coletor produziu **8 registros a partir de 2 arquivos**, cada um com id por impressão digital de conteúdo. Confirma que (1) identidade é a descoberta e (2) N achados = N registros valem sobre as formas que o revisor realmente escreve.

## MAJOR — os três testes do change request não são nomeados por AC nenhum

O change request de 2026-08-04T21:26 exigia "cada uma das três precisa de teste nomeado". Os três testes existem e passam, mas **nenhum AC os nomeia**: estão protegidos só pelo AC-7 (workspace verde). Apagar qualquer um dos três não deixaria nenhum critério vermelho.

Testes órfãos: `finding_collect_mints_a_new_finding_for_a_new_discovery_under_the_same_source`, `finding_collect_splits_one_file_into_one_record_per_finding`, `finding_collect_keeps_the_decision_when_the_source_stops_reporting`.

## MINOR — arquivo de revisão limpo ainda cunha pseudo-achado

Provado ao vivo: um `findings.md` com `## Findings` / `- none` coletou `open:1`, statement `"none"`. Toda revisão aprovada passa a bloquear o CLOSE até alguém descartá-la com motivo. É fail-closed deliberado (documentado em `finding_collect.rs:386`), mas é atrito que o operador encontra em toda unidade.

## MINOR — o portão ignora o `ok` do próprio coletor

`close_gates.rs:572`: um diretório de spec com achados e sem `meta.json` legível bloqueia o CLOSE por achados que o `mark-finding` depois recusa rotear. Vem da rodada anterior, sem destino declarado.

## MINOR — forma dos argumentos diverge do irmão

`mark-finding` declara `--id/--to/--reason` como `Option<String>` com validação à mão, enquanto o vizinho `mark-checklist-item` usa `String` obrigatório.

## MINOR — o build roda duas vezes no /pr

`close-orchestrate` agora roda os comandos do `mustard.json` duas vezes (`verify-pipeline` e depois o sub-portão de build). Documentado e aceito no módulo; vale nota ao operador.
