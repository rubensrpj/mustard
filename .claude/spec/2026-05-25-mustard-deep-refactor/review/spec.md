# Review — Mustard Deep Refactor

Code review consolidado após EXECUTE de todas as 13 waves. Dispatch um review-agent por subprojeto afetado (rt/cli/core/dashboard).

## Categorias

1. **Determinismo** — todo placeholder de prompt agora preenchido por Rust (não prosa do LLM)
2. **Agnosticismo** — zero hardcode de stack em código ou prompt
3. **Cobertura** — entity-registry e cluster_discovery rodaram em todos subprojetos
4. **Wirelinks** — refs `[[id]]` apontam para nós que existem
5. **Templates** — `commands/mustard/*/SKILL.md` total ≤800 linhas
6. **Memória** — `agent_memory` + `memory_feedback` funcionais com FTS5
7. **Validators** — `spec-validate`, `skill-validate --strict-frontmatter`, `scan-md-validate`, `scan-recipes-validate` operacionais

## Comando

```bash
rtk mustard-rt run review-dispatch --spec 2026-05-25-mustard-deep-refactor
```
