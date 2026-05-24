# Wave 2 — Rt: remover bucket-aware code + sync header

## Resumo

Remover do `mustard-rt` toda lógica que depende de `spec/{active,completed,superseded}/`. `complete_spec::archive` perde a parte de mover pasta (vira só emit). `session_cleanup` perde o varredor de mv. Todos os helpers que resolvem caminho passam a buscar `spec/{name}/` flat. `emit-pipeline --kind pipeline.status` ganha um lado de write: sincroniza o cabeçalho `### Status:` do `spec.md` correspondente para a fonte canônica cross-dev.

## Contexto

Hoje o rt conhece três pastas e move arquivos entre elas no fim do ciclo. Isso amarra o lifecycle ao filesystem. Wave 2 corta esse acoplamento: arquivar deixa de mover, e o que sincroniza colaboradores é o header do markdown — versionado em git — atualizado a cada `emit-pipeline --kind pipeline.status`. Hooks de sessão deixam de mexer em pastas.

Depende da Wave 1 porque o `complete_spec` chama `pipeline_state_for_spec`, que ainda passa pelo `project_spec_view` — quando o spec local não tem eventos (caso colaborador), o fold da Wave 1 garante que o status seja resolvido a partir do header.

## Arquivos

```
apps/rt/src/run/complete_spec.rs         — arrancar archive::move_dir + active/completed paths
apps/rt/src/run/spec_extract.rs          — resolve flat
apps/rt/src/run/qa_run.rs                — resolve flat
apps/rt/src/run/wave_tree.rs             — resolve flat
apps/rt/src/run/wikilink_extract.rs      — resolve flat (já tenta multi-bucket; remover loop)
apps/rt/src/run/emit_pipeline.rs         — sincroniza header da spec.md em pipeline.status
apps/rt/src/hooks/session_cleanup.rs     — remover mv de pasta no cleanup
apps/rt/tests/*                          — ajustar testes que dependem de buckets
```

## Tarefas

- [x] `complete_spec.rs::archive`: remover `move_dir(active, completed)`. Mantém apenas o `emit_completed_status` (já idempotente). O `complete-spec --archive` vira um no-op em termos de filesystem; só garante o emit terminal. Remover `archive_metrics_from_state` se não tem mais caller.
- [x] `complete_spec.rs`: as funções `active_spec_dir`/`completed_spec_dir` viram uma única `spec_dir(cwd, name) = cwd/.claude/spec/{name}`.
- [x] `complete_spec.rs::archive_followups` e `archive_followups_legacy_fs`: ajustar para iterar via `distinct_specs()` no SQLite e checar header + timestamp do `pipeline.complete`. Sem dependência de pasta.
- [x] Mesma flatten em `spec_extract.rs`, `qa_run.rs`, `wave_tree.rs`, `wikilink_extract.rs` (substituir loops `for sub in ["active","completed","cancelled","superseded"]` por lookup direto).
- [x] `emit_pipeline.rs`: quando `kind == "pipeline.status"`, depois de gravar o evento, abrir `.claude/spec/{spec}/spec.md`, reescrever o cabeçalho `### Status:` para o valor de `payload.to` (fail-open: warn se arquivo não existe). Isso mantém o canon cross-dev.
- [x] `session_cleanup.rs`: remover o passo que move pastas para `completed/`. Quaisquer outros side-effects (limpar pipeline-states stale, telemetria) ficam.
- [x] Testes: atualizar `complete_spec_emits_qa.rs` e qualquer outro que faça `assert!(completed_dir.exists())`.

## Acceptance Criteria

- [x] AC-W2-1: Testes do rt passam — Command: `cargo test -p mustard-rt --bin mustard-rt`
- [x] AC-W2-2: Não há mais `active_spec_dir` / `completed_spec_dir` no rt — Command: `node -e "const r=require('child_process').execSync('rg -n \"active_spec_dir|completed_spec_dir\" apps/rt/src','utf8').toString().trim();process.exit(r===''?0:(console.error(r),1))"`
- [x] AC-W2-3: Emit `pipeline.status: completed` sincroniza header — Command: `bash -c 'TEST=/tmp/mustard-test-$$;mkdir -p "$TEST/.claude/spec/sample" "$TEST/.claude/.harness";echo -e "# x\n### Status: implementing\n" > "$TEST/.claude/spec/sample/spec.md";cd "$TEST" && mustard-rt run emit-pipeline --kind pipeline.status --spec sample --payload "{\"to\":\"completed\"}" && grep -q "^### Status: completed" .claude/spec/sample/spec.md'`

## Limites

- `apps/rt/src/run/*`
- `apps/rt/src/hooks/session_cleanup.rs`
- `apps/rt/tests/*`

## Network

- Parent: [[2026-05-21-flatten-spec-layout-and-multi-collab]]
- Depende de: [[wave-1-library]]
- Bloqueia: [[wave-4-general]], [[wave-5-general]]
