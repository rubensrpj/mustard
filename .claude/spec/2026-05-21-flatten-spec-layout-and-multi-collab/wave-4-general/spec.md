# Wave 4 — CLI + SKILLs: limpar bucket references

## Resumo

Os SKILLs de pipeline (`/close`, `/resume`, `/feature`, `/bugfix`, `/tactical-fix`, `/qa`, `/approve`) e os comandos `init`/`update` da CLI ainda escrevem ou citam `spec/active/`, `spec/completed/`, `spec/superseded/`. Wave 4 trata todos esses textos: substitui paths para `spec/{name}/` e ajusta a redação dos passos (não mais "mover para completed/").

## Contexto

Os SKILLs são consumidos diretamente pelo Claude Code (templates copiados para `.claude/commands/mustard/` em cada projeto). Manter o texto antigo significa que cada novo projeto continua vendo "mover para completed/" como instrução — mesmo que o binário não faça mais isso. `init.rs`/`update.rs` da CLI criam a árvore `.claude/spec/{active,completed,superseded}/` em projetos novos; precisa virar só `.claude/spec/`.

## Arquivos

```
apps/cli/templates/commands/mustard/close/SKILL.md         — passos 4, 5, 6, 7b
apps/cli/templates/commands/mustard/resume/SKILL.md        — paths em refs
apps/cli/templates/commands/mustard/feature/SKILL.md       — Spec Hygiene, paths em refs
apps/cli/templates/commands/mustard/bugfix/SKILL.md        — paths
apps/cli/templates/commands/mustard/tactical-fix/SKILL.md  — paths
apps/cli/templates/commands/mustard/qa/SKILL.md            — paths
apps/cli/templates/commands/mustard/approve/SKILL.md       — paths
apps/cli/templates/pipeline-config.md                       — referências cross-flow
apps/cli/src/commands/init.rs                               — criar só spec/
apps/cli/src/commands/update.rs                             — não recriar buckets
apps/cli/templates/refs/feature/spec-hygiene.md             — busca em spec/{name}/
apps/cli/templates/refs/feature/wave-decomposition.md       — paths
```

## Tarefas

- [x] `close/SKILL.md`: passo 1 vira "Locate spec in `.claude/spec/{name}/`". Remover passo 5 ("move to completed/") — substituir por "the spec.md header is updated via `emit-pipeline` (Wave 2)". Ajustar passos 7a/7b para `--spec-dir .claude/spec/{spec-name}`.
- [x] `resume/SKILL.md`: substituir `.claude/spec/active/{slug}` por `.claude/spec/{slug}`. Ajustar fix-loop e qualquer assert de path.
- [x] `feature/SKILL.md`: passos de Spec Hygiene não auditam buckets — varrem `.claude/spec/` direto (filtrando por status do SQLite). Reescrever Step 1 do PLAN para criar em `.claude/spec/{date}-{name}/spec.md`.
- [x] `bugfix/SKILL.md`, `tactical-fix/SKILL.md`, `qa/SKILL.md`, `approve/SKILL.md`: substituições de path.
- [x] `pipeline-config.md`: revisar a seção "Spec Layout" e qualquer parte que descreva o `active → completed` flow.
- [x] `init.rs` / `update.rs`: criar apenas `.claude/spec/`. Remover os mkdir para `active`/`completed`/`superseded`.
- [x] Refs (`spec-hygiene.md`, `wave-decomposition.md`): atualizar quaisquer paths.

## Acceptance Criteria

- [x] AC-W4-1: Nenhuma referência a `spec/active/`, `spec/completed/` ou `spec/superseded/` em SKILLs ou pipeline-config — Command: `node -e "const cp=require('child_process');const r=cp.execSync(\"rg -n 'spec/(active|completed|superseded)/' apps/cli/templates --glob '!*.json'\",'utf8').toString().trim();process.exit(r===''?0:(console.error(r),1))"`
- [x] AC-W4-2: `cargo test -p mustard-cli` passa — Command: `cargo test -p mustard-cli`
- [x] AC-W4-3: `mustard-cli init` em diretório limpo cria só `.claude/spec/` (sem subbuckets) — Command: `bash -c 'TEST=/tmp/mustard-init-$$;mkdir -p "$TEST";cd "$TEST" && cargo run -q -p mustard-cli -- init --yes && [ -d ".claude/spec" ] && [ ! -d ".claude/spec/active" ] && [ ! -d ".claude/spec/completed" ]'`

## Limites

- `apps/cli/templates/commands/mustard/*/SKILL.md`
- `apps/cli/templates/pipeline-config.md`
- `apps/cli/templates/refs/feature/*`
- `apps/cli/src/commands/init.rs`
- `apps/cli/src/commands/update.rs`

## Network

- Parent: [[2026-05-21-flatten-spec-layout-and-multi-collab]]
- Depende de: [[wave-2-general]], [[wave-3-general]]
- Bloqueia: [[wave-5-general]], [[wave-6-general]]
