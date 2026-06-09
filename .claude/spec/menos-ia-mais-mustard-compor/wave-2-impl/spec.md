# wave-2-impl

## Resumo

RT: validacao de AC no analyze-validation + dispatch-plan de spec unica + comando hardcode-gate

## Rede

- Pai: [[menos-ia-mais-mustard-compor]]

## Tarefas

- [ ] - [ ] AC no analyze-validation (apps/rt/src/commands/spec/analyze_validation.rs): localizar o parser de AC que o qa-run usa (memoria: TF qa-run-parseia-ac usou o parser do drafter — ache a fonte unica; se for privado, exponha pub(crate)/core em vez de duplicar). Regra: secao de AC presente (## Criterios de Aceitacao / ## Acceptance Criteria) + zero itens parseaveis -> issue WARN `unparseable-ac` com dica do formato exato (`**AC-N** — titulo` + linha `Command:`). Secao ausente nao muda comportamento atual. Testes `ac_format_validation_*` (parseavel ok; mal-formatado WARN; sem secao inalterado).
- [ ] - [ ] dispatch-plan de spec unica (apps/rt/src/commands/spec/dispatch_plan.rs): quando o spec NAO e wave-plan (sem wave-plan.md), emitir plano de 1 item em vez de []: role impl, wave null/0, subproject inferido do spec (## Arquivos/## Files — prefixo comum dos paths; fallback `.`), prompt_cmd = agent-prompt-render --spec X --role impl --subproject Y --mode first (sem --wave). Nao mudar NADA no caminho wave-plan. Testes `dispatch_single_spec_*` (TF-like emite 1 item; wave-plan segue identico; spec inexistente degrada como hoje).
- [ ] - [ ] Novo comando `hardcode-gate` (variante no enum RunCmd em apps/rt/src/commands/mod.rs + braco em dispatch() — os DOIS registros): le os literais dos registries embutidos do core (stacks.toml: manifest_deps + path_markers + code_signatures; nomes de stack ficam FORA — `next` colide com identificadores comuns), roda git diff (working tree + staged) restrito a apps/scan/src e packages/core/src arquivos .rs, e reporta linhas ADICIONADAS contendo qualquer literal. Saida JSON deterministica {ok, hits:[{file,line,literal}]}; exit 0 sempre (gate advisory — o orquestrador decide), ok=false quando ha hits. Excecao: linhas dentro de #[cfg(test)] nao precisam ser distinguidas na v1 — documente a limitacao no help.
- [ ] - [ ] Testes `hardcode_gate_*`: (1) repo limpo atual -> ok=true zero hits; (2) fixture de diff sintetico com literal introduzido -> hit reportado com file/line. Use tempdir+git init como os testes de rt fazem (ache um exemplo existente de teste que usa git).
- [ ] - [ ] Rodar os filtros novos + suite do rt; reportar numeros.

## Arquivos

- `apps/rt/src/commands/spec/analyze_validation.rs`
- `apps/rt/src/commands/spec/dispatch_plan.rs`
- `apps/rt/src/commands/mod.rs`
- `apps/rt/src/commands/review/hardcode_gate.rs`
