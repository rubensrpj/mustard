# Plano de Waves

## Tabela de Waves

| Wave | Spec | Papel | Depende de | Resumo |
|------|------|-------|------------|--------|
| 1 | [[wave-1-impl]] | impl | — | Core: seed aspnet no registro + normalizacao de checkbox na materializacao de tasks |
| 2 | [[wave-2-impl]] | impl | — | RT: validacao de AC no analyze-validation + dispatch-plan de spec unica + comando hardcode-gate |
| 3 | [[wave-3-impl]] | impl | [[wave-2-impl]] | RT: comandos compostos plan-materialize, wave-advance e close-pipeline (composicao direta, sem fachada) |
| 4 | [[wave-4-impl]] | impl | [[wave-3-impl]] | Prosa: SKILL do /feature e refs do /spec passam a usar os comandos compostos + wave-dependency (local + templates) |

## Critérios de Aceitação
- AC-6 — Registro com 4a stack aspnet (dotnet) e parse verde: `cargo test -p mustard-core stacks_registry_parses`
- AC-5 — wave-scaffold normaliza prefixo de checkbox: `cargo test -p mustard-rt checkbox_normalize`
- AC-3 — analyze-validation valida AC com o parser do qa-run (WARN unparseable-ac): `cargo test -p mustard-rt ac_format_validation`
- AC-2 — dispatch-plan de spec unica emite plano de 1 item: `cargo test -p mustard-rt dispatch_single_spec`
- AC-4 — hardcode-gate detecta literal introduzido e passa limpo no tree atual: `cargo test -p mustard-rt hardcode_gate`
- AC-1 — Composicoes existem (enum+dispatch), compoem sem duplicacao, JSON deterministico, testes felizes+degradados: `cargo test -p mustard-rt composite`
- AC-7 — Prosa cita os compostos + wave-dependency (local + templates): `rg -l "wave-advance" .claude/commands/mustard apps/cli/templates/commands/mustard .claude/refs apps/cli/templates/refs`
- AC-8 — Suite completa do rt verde: `cargo test -p mustard-rt`
