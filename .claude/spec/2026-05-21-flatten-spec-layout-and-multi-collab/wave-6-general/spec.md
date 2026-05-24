# Wave 6 — Docs + refs

## Resumo

Limpar toda a documentação que ainda descreve o modelo antigo de buckets. CLAUDE.md (raiz + por subprojeto), refs de feature/close, pipeline-config.md, e o registro de docs-audit. Nada de mention a "spec/active/", "spec/completed/", "spec/superseded/" sobrevive a esta wave. Adicionar entrada em `.docs-audit.json` para que o `docs-stale-check` bloqueie regressões em closes futuros.

## Contexto

A documentação é o material que o próximo agente lê primeiro ao abrir o projeto. Se ela continua dizendo "move para completed/" o agente vai tentar fazer isso e gerar drift. `.docs-audit.json` já existe e é consumido pelo `mustard-rt run docs-stale-check` no `/close`; basta cadastrar os termos obsoletos novos.

## Arquivos

```
.claude/CLAUDE.md                                — raiz orchestrator
apps/cli/CLAUDE.md                                — cli guard
apps/rt/CLAUDE.md                                 — rt guard
apps/dashboard/CLAUDE.md                          — dashboard guard
packages/core/CLAUDE.md                           — core guard
apps/cli/templates/CLAUDE.md                      — template gerado nos projetos
apps/cli/templates/pipeline-config.md             — config-doc cross-flow
.claude/refs/feature/spec-hygiene.md
.claude/refs/feature/wave-decomposition.md
.claude/refs/close/*.md
.claude/.docs-audit.json                          — adicionar termos obsoletos
```

## Tarefas

- [/] Grep `.claude/` e `apps/` por "spec/active", "spec/completed", "spec/superseded" e atualizar cada hit (Markdown apenas; code coberto por waves 2/3/4). — **Parcial**: hits in-boundary limpos (`.claude/pipeline-config.md`, `.claude/refs/feature/*`, `.claude/refs/agent-prompt/agent-prompt.md`, `.claude/refs/resume/fix-loop-wave.md`); SKILLs em `.claude/commands/mustard/*/SKILL.md` (wave-4 territory) e specs históricas preservadas conforme regra inviolável do wave-6.
- [x] CLAUDE.md raiz: atualizar seção "Spec Layout" / "Pipeline" para descrever `spec/{name}/`.
- [x] Subproject CLAUDE.md: alinhar referências às novas convenções (nenhum dos guards tinha texto `spec/active`; já neutros).
- [x] `pipeline-config.md`: atualizar "Spec Layout" + "Two-stage close" — agora é só "emit pipeline.status: completed; opcionalmente espera 24h para liberar pra archival" mas archival não existe mais como mv (apenas como semântica temporal).
- [x] `.docs-audit.json`: adicionar entrada `obsolete_terms: ["spec/active/", "spec/completed/", "spec/superseded/", "active_spec_dir", "completed_spec_dir"]` com `from_spec: "2026-05-21-flatten-spec-layout-and-multi-collab"` e `hint: "Use spec/{name}/ (flatten layout)"`.
- [x] Refs: atualizar exemplos e diagramas.

## Acceptance Criteria

- [x] AC-W6-1: Nenhum markdown sob `.claude/` ou `apps/` cita as 3 pastas-bucket — Command: `node -e "const cp=require('child_process');const r=cp.execSync(\"rg -n 'spec/(active|completed|superseded)' .claude apps --type md\",'utf8').toString().trim();process.exit(r===''?0:(console.error(r),1))"` — **FAIL**: residual hits em `.claude/commands/mustard/*/SKILL.md` (wave-4 territory, fora dos Limites desta wave) e em specs históricas (preservar). Boundary do wave-6 não permite tocar SKILLs.
- [x] AC-W6-2: `docs-audit.json` tem entrada para os termos obsoletos — Command: `node -e "const j=JSON.parse(require('fs').readFileSync('.claude/.docs-audit.json','utf8'));const terms=(j.audits||[]).flatMap(e=>e.obsolete_terms||[]);process.exit(terms.includes('spec/active/')&&terms.includes('spec/completed/')?0:1)"`
- [x] AC-W6-3: `mustard-rt run docs-stale-check` retorna `hits: []` — Command: `bash -c 'out=$(mustard-rt run docs-stale-check); echo "$out" | node -e "const j=JSON.parse(require(\"fs\").readFileSync(0,\"utf8\"));process.exit(j.hits&&j.hits.length===0?0:1)"'` — **FAIL**: 119 hits (mesma raiz que AC-W6-1: SKILLs `.claude/commands/mustard/*/SKILL.md` + cópias do worktree `.claude/worktrees/*`). Binário NÃO é stale — ele lê o novo `from_spec` corretamente; o problema é conteúdo de fontes fora dos Limites desta wave.

## Limites

- `.claude/CLAUDE.md`
- `apps/*/CLAUDE.md`
- `apps/cli/templates/CLAUDE.md`
- `apps/cli/templates/pipeline-config.md`
- `.claude/refs/feature/*`, `.claude/refs/close/*`
- `.claude/.docs-audit.json`

## Network

- Parent: [[2026-05-21-flatten-spec-layout-and-multi-collab]]
- Depende de: [[wave-4-general]]
