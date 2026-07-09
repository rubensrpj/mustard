# TF: gates com falso-positivo + nudge que colide com a regra de eficiência

## Contexto

Consolidação do `/btw` de campo (sialia, 2026-07-09, run real do prompt sialia-partners): os gates abaixo têm histórico de disparo falso — "um gate que precisa de escape hatch rotineiro não é gate, é atrito" — e um hook do próprio harness contradiz a regra de eficiência do CLAUDE.md. Itens 1-3 já apareceram em memórias anteriores; este TF os consolida com reprodução e critério de fechamento.

## Tarefas

- [ ] T1 `verify-pipeline` poliglota: em repo multi-linguagem o verify "nunca passa" e o fechamento exige `MUSTARD_QA_GATE_MODE=warn` como rota rotineira. Reproduzir no sialia (TS+C#), identificar o(s) check(s) que assumem stack única, corrigir para stack-aware (precedente: `source_lang` no precheck/size-check), e cobrir com teste de fixture poliglota.
- [ ] T2 `resume-bootstrap` reporta `ReviewPending` falso: reproduzir (spec com review concluído que ainda acusa pendência), achar a fonte (evento terminal ausente? leitura de span?), corrigir e testar.
- [ ] T3 `rewave` gera falso "full": reproduzir o caso em que `exec-rewave-check` promove para full sem crescimento real de escopo; ajustar o sinal (respeitar layerCount/arquivos reais) e testar.
- [ ] T4 `bash_command_gate` (Native Tool Redirect) colide com a regra "capture em arquivo, fatie o arquivo": o nudge manda usar Grep nativo mesmo quando o Bash está fatiando um ARQUIVO capturado de `mustard-rt run …` (o padrão que o próprio CLAUDE.md manda usar). Silenciar o nudge quando o alvo do grep/head/tail é um arquivo único explícito (não árvore/glob); manter o nudge para varredura de árvore. Teste dos dois lados.

## Critérios de Aceitação

- [ ] AC1: fixture poliglota em que `verify-pipeline` passa sem `MUSTARD_QA_GATE_MODE=warn`; o caso que antes falhava vira teste de regressão.
- [ ] AC2: cenário do ReviewPending falso reproduzido em teste; pós-fix o resume-bootstrap reporta o estado real.
- [ ] AC3: cenário do falso "full" em teste; pós-fix o rewave só promove com crescimento real (>5 arquivos ou 2ª camada).
- [ ] AC4: `cargo test -p mustard-rt` com casos novos: nudge SILENCIOSO em `grep <termo> <arquivo-capturado>`, ATIVO em `grep -r <termo> src/`.
- [ ] AC5: `cargo build --release` + suíte completa verde; nenhuma saída de `run` perde byte-estabilidade.