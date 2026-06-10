# Tactical Fix: wave-scaffold — contrato n/role no --help + parse error com error+exit 2

## Contexto

Auditoria 2026-06-10 (memória `mustard-sialia-payables-audit`): telemetria do sialia registra ≥6 falhas `missing field n` em 6 dias, duas delas com o `--help` lido 1-2 min antes de autorar o plan — o help (doc-comment clap de WaveScaffold em apps/rt/src/commands/mod.rs:775-800) documenta só os campos opcionais com a frase enganosa "all `#[serde(default)]`" e omite os obrigatórios `n`/`role`; o exemplo válido existe só no rustdoc (invisível) e na ref de prosa que o orquestrador não consome (todas as falhas invocaram wave-scaffold bare, nunca plan-materialize). Defeito adjacente: parse error → `ScaffoldOutcome::Unreadable` imprime stderr claro MAS retorna exit 0 com stdout `{"created_files":[],"skipped":[]}` sem campo `error` — inconsistente com EmptyPlan (error + exit 2) e com plan_materialize.rs (que já emite `error`).

Fix: (1) long_about declara obrigatórios `n` (1-based) + `role` (drivers de `wave-{n}-{role}`), cola o exemplo mínimo do rustdoc, aponta `plan-from-spec` como produtor canônico do skeleton e recomenda `plan-materialize` como entrada preferida; corrige a frase "all serde(default)". (2) braço Unreadable alinha com EmptyPlan: campo `error` no stdout + exit 2 + hint acionável em `missing field`. Atenção: snapshots insta/gates de byte-estabilidade do rt.

## Critérios de Aceitação

- **AC-1** — Testes: long_about contém `n` e `role` e o exemplo; plan sem `n` → stdout com campo `error` + hint + exit 2.
  Command: `cargo test -p mustard-rt wave_scaffold`
- **AC-2** — Workspace verde.
  Command: `cargo test --workspace`

## Arquivos

- apps/rt/src/commands/mod.rs — doc-comment clap da variante WaveScaffold
- apps/rt/src/commands/wave/wave_scaffold.rs — braço Unreadable de run() (~443-450)
- testes/snapshots em apps/rt