# wave-3-general

## Resumo

Catch-all retroativo via doctor. Mesmo com wave-1 (gate ruidoso) + wave-2 (montagem programática), specs já criadas no repo podem ter discrepância entre `wave-plan.md` e diretórios reais. Esta wave adiciona um check `wave-integrity` em `doctor.rs` que percorre cada spec sob `.claude/spec/{name}/` (layout flat, pós flatten-spec), parsea os wikilinks `[[wave-N-role]]` da tabela em `wave-plan.md`, lista os diretórios reais sob o spec, e reporta WARN para cada mismatch — com hint do comando exato pra arrumar. Aproveita a mesma passada pra tornar `doctor.rs::collect_active_spec_names` flat-aware (renomeia para `collect_spec_names`, lê `.claude/spec/` direto): gap que a flatten-spec-layout não listou nos `## Arquivos` dela. Roda automaticamente em `mustard-rt run doctor` sem novo hook nem trigger automático em pipeline (feedback `analysis_pattern`: doctor continua reativo).

## Network

- Parent: [[2026-05-21-wave-integrity-and-doctor-check]]
- Depende de: [[wave-1-library]]

## Arquivos

```
apps/rt/src/run/doctor.rs                              — modify: rename collect_active_spec_names → collect_spec_names (flat) + nova fn check_wave_integrity + chamada em run()
apps/cli/templates/commands/mustard/maint/SKILL.md     — modify: documentar wave-integrity na tabela de categorias
```

## Tarefas

- [ ] Renomear `collect_active_spec_names` (`doctor.rs:643`) para `collect_spec_names`. Atualizar o body para ler `.claude/spec/` direto (sem subbucket `active/`), filtrando entradas que (a) são diretório e (b) contêm pelo menos um arquivo `spec.md` ou `wave-plan.md`. Atualizar o caller em `check_state_health` (linha 580).
- [ ] Adicionar `fn check_wave_integrity(claude_dir: &Path) -> CheckResult` em `apps/rt/src/run/doctor.rs`, próximo a `check_state_health`. Para cada subdiretório em `.claude/spec/{name}/`: se contém `wave-plan.md`, parsear o arquivo procurando linhas de tabela com padrão `\[\[wave-(\d+)-([a-z][a-z0-9-]*)\]\]`. Extrair conjunto `declared = {wave-1-general, wave-2-frontend, ...}`. Listar diretórios sob o spec-dir com prefixo `wave-` e nome `wave-N-{role}`. Conjunto `actual`. WARN per item em `declared \ actual` (falta diretório) e `actual \ declared` (diretório órfão).
- [ ] Para cada warning, formato: `"{spec-name}: declared [[{wave}]] but no directory {wave}/ — run mustard-rt run wave-scaffold --spec-dir .claude/spec/{spec} --plan .claude/spec/{spec}/plan.json"` (ou variante "orphan directory" sem hint de fix).
- [ ] Adicionar `results.push(check_wave_integrity(&claude_dir));` em `run()` (linha 741+), entre `check_state_health` e `lsp_check`.
- [ ] Adicionar `"wave-integrity"` à lista de check names esperados em qualquer asserção/doc — neste caso atualizar a tabela "Report categories" em `apps/cli/templates/commands/mustard/maint/SKILL.md`.
- [ ] Adicionar `#[test] fn wave_integrity_warns_on_missing_directory()`: tempdir com `wave-plan.md` referenciando 2 waves mas apenas 1 diretório, roda `check_wave_integrity`, valida status WARN e detail contendo o wave faltando.
- [ ] Adicionar `#[test] fn wave_integrity_warns_on_orphan_directory()`: tempdir com `wave-plan.md` referenciando 1 wave + 2 diretórios no disco, valida WARN com mensagem "orphan".
- [ ] Adicionar `#[test] fn wave_integrity_ok_when_consistent()`: tempdir alinhado, valida status OK.
- [ ] Adicionar `#[test] fn wave_integrity_skips_specs_without_wave_plan()`: spec single-file sem `wave-plan.md`, valida que não gera warning (não é multi-wave).
- [ ] Adicionar `#[test] fn collect_spec_names_reads_flat_layout()`: tempdir com `.claude/spec/foo/spec.md` e `.claude/spec/bar/wave-plan.md`, valida que `collect_spec_names` retorna `["bar","foo"]` direto de `spec/` (sem precisar de subbucket).
- [ ] `cargo build -p mustard-rt && cargo test -p mustard-rt -- doctor`

## Acceptance Criteria

- [ ] AC-1: `cargo test -p mustard-rt -- doctor::tests::wave_integrity` passa (4 testes) — Command: `cargo test -p mustard-rt -- doctor::tests::wave_integrity`
- [ ] AC-2: `mustard-rt run doctor` lista a linha `wave-integrity` no output — Command: `bash -c 'mustard-rt run doctor 2>&1 | grep -qE "(OK|WARN)\\s+wave-integrity"'`
- [ ] AC-3: SKILL `/mustard:maint` documenta o novo check na tabela `Report categories` — Command: `node -e "const t=require('fs').readFileSync('apps/cli/templates/commands/mustard/maint/SKILL.md','utf8');if(!t.includes('wave-integrity'))throw new Error('missing wave-integrity doc')"`

## Limites

- `apps/rt/src/run/doctor.rs` (apenas nova fn + chamada em `run()` + testes)
- `apps/cli/templates/commands/mustard/maint/SKILL.md` (apenas tabela de Report categories)

Out-of-boundary: hooks (não vira gate automático), `wave_scaffold.rs` (wave-1), `plan_from_spec.rs` (wave-2), `apps/cli/src/commands/init.rs` (não muda).
