# Wave 5 — Comando /mustard:spec clear

### Parent: [[2026-05-23-per-spec-event-log-claude-devtools]]
### Stage: Plan
### Outcome: Active
### Flags:
### Lang: pt

## PRD

### Contexto

Como não há janitor automático (Não-Objetivo declarado), specs fechadas mantém pasta `events/` + `blobs/` indefinidamente. Esta wave entrega um comando manual `/mustard:spec clear` (skill que invoca `mustard-rt run spec-clear`) que varre `.claude/spec/` e, para cada spec com `Stage: Close` + `Outcome: Done` cuja pasta `events/` tem mtime mais antiga que `--age-days` (default 15), remove `events/` e `blobs/` preservando `spec.md`, `wave-plan.md` e `wave-N-{role}/spec.md`. O default é `--dry-run` — só imprime o que seria apagado. `--apply` executa. `--all` ignora o filtro de idade. `--name <slug>` opera só numa spec específica. Saída é tabela enxuta `spec | events_age_days | size_kb | action`.

### Acceptance Criteria

- [ ] AC-W5-1: `mustard-rt run spec-clear --help` lista `--dry-run`, `--apply`, `--all`, `--name`, `--age-days` — Command: `rtk mustard-rt run spec-clear --help`
- [ ] AC-W5-2: Sem flags, é dry-run (nenhum arquivo apagado) — Command: `cargo test -p mustard-rt --test spec_clear_dry_run_default`
- [ ] AC-W5-3: `--apply --name <test-spec>` remove `events/` e `blobs/`, preserva `spec.md` — Command: `cargo test -p mustard-rt --test spec_clear_apply_preserves_spec_md`
- [ ] AC-W5-4: Skill `apps/cli/templates/commands/mustard/spec/clear/SKILL.md` existe e invoca o comando — Command: `node -e "const fs=require('fs');const p='apps/cli/templates/commands/mustard/spec/clear/SKILL.md';process.exit(fs.existsSync(p)&&fs.readFileSync(p,'utf8').includes('mustard-rt run spec-clear')?0:1)"`
- [ ] AC-W5-5: `cargo build && cargo clippy --workspace -- -D warnings` passa — Command: `cargo build --workspace && cargo clippy --workspace -- -D warnings`
- [ ] AC-W5-6: Após `--apply` numa spec dummy fechada, `spec.md` + `wave-plan.md` continuam acessíveis e nenhum `events/` resta — Command: `cargo test -p mustard-rt --test spec_clear_apply_preserves_spec_md`

## Plano

### Arquivos

- `apps/rt/src/run/spec_clear.rs` (novo) — implementação clap + algoritmo
- `apps/rt/src/run/mod.rs` (edição) — registrar subcomando `spec-clear`
- `apps/cli/templates/commands/mustard/spec/clear/SKILL.md` (novo) — wrapper /mustard:spec clear
- `apps/rt/tests/spec_clear_dry_run_default.rs` (novo)
- `apps/rt/tests/spec_clear_apply_preserves_spec_md.rs` (novo)

### Tarefas

#### General Agent (Wave 5)

- [ ] Criar `spec_clear.rs` com clap (flags `--dry-run` default, `--apply`, `--all`, `--name <slug>`, `--age-days <N>` default 15)
- [ ] Algoritmo: glob `.claude/spec/*/spec.md` → parse headers `### Stage:` + `### Outcome:` → filtra Close+Done → para cada, lê mtime mais recente em `events/` recursivo → compara com cutoff → emite linha na tabela ou apaga
- [ ] Mutuamente exclusivos: `--dry-run` e `--apply`
- [ ] Saída: tabela `spec | events_age_days | size_kb | action (KEEP|DELETE|SKIPPED-OPEN)`
- [ ] Preserva: `spec.md`, `wave-plan.md`, qualquer `wave-N-{role}/spec.md` (não remove a pasta da wave inteira, só `events/` e `blobs/` de dentro)
- [ ] Criar skill em `apps/cli/templates/commands/mustard/spec/clear/SKILL.md` (didático, exibe a tabela do binário verbatim)
- [ ] Testes: dry-run não toca disco; --apply preserva spec.md e remove apenas events/+blobs/
- [ ] `cargo build && cargo test -p mustard-rt && cargo clippy --workspace -- -D warnings`

### Dependências

Wave 1 (precisa do layout `events/` + `blobs/` real pra varrer).

### Limites

- **Tocar:** `apps/rt/src/run/spec_clear.rs`, `apps/rt/src/run/mod.rs`, `apps/cli/templates/commands/mustard/spec/clear/SKILL.md`, `apps/rt/tests/spec_clear_*.rs`.
- **NÃO tocar:** `apps/dashboard/**`, `packages/core/**`, hooks do `mustard-rt`, schema SQLite.
