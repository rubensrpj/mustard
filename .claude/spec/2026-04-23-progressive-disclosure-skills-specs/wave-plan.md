# Wave Plan: progressive-disclosure-skills-specs

### Status: completed | Phase: CLOSE | Scope: full | Decomposed: yes (3 waves)
### Checkpoint: 2026-04-23
### Reason: deep-layers (3 layers: hooks, scripts, commands; 13+ files; 2 new hook entities) + user requested risk isolation for feature/SKILL.md

## Summary

Aplicar progressive disclosure (padrão oficial Anthropic — body ≤200 ideal, ≤500 max) em todos os SKILL.md/CLAUDE.md do Mustard, sem perder conteúdo. Adicionar gates (`spec-size-gate`, `skill-size-gate`) e estender `skill-validate.js`. Refactor em 2 etapas de risco progressivo: low-risk (CLAUDE.md + git + scan) antes do high-risk (feature/SKILL.md, coração do pipeline). Modo default `warn` — nunca bloqueia.

## Waves

### Wave 1 — Size-Gate Infrastructure (5 files, low risk)
Depends on: none
- `templates/hooks/spec-size-gate.js` (NEW)
- `templates/hooks/skill-size-gate.js` (NEW)
- `templates/hooks/__tests__/size-gates.test.js` (NEW)
- `templates/scripts/skill-validate.js` (EXTEND)
- `templates/settings.json` (WIRE)

Full spec: `wave-1-size-gate-infra/spec.md`

### Wave 2a — Refactor Low-Risk Files (~10 files, baixo risco)
Depends on: wave 1
- `templates/CLAUDE.md` (206 → ≤200, Enforcement tables para `pipeline-config.md`)
- `templates/commands/mustard/git/SKILL.md` (588 → ≤200) + 3 references/
- `templates/commands/mustard/scan/SKILL.md` (450 → ≤200) + 2 references/
- `.claude/commands/mustard/{git,scan}/**` (MIRROR)

Full spec: `wave-2a-refactor-low-risk/spec.md`

### Wave 2b — Refactor feature/SKILL.md (6 files, alto risco, approval próprio)
Depends on: wave 2a (layout já provado por git+scan)
- `templates/commands/mustard/feature/SKILL.md` (458 → ≤200) + 3 references/
- NOVA seção `## Spec Layout` documentando padrão para specs futuras
- `.claude/commands/mustard/feature/**` (MIRROR)

Full spec: `wave-2b-refactor-feature-skill/spec.md`

## Rationale

- `scope-decompose.js` indicou `decompose: true, reason: "deep-layers"`.
- Wave 1 é foundational + backwards-compatible (modo `warn`).
- Wave 2a usa git/scan como "prova do conceito" do layout `SKILL.md + references/`.
- Wave 2b isola o refactor mais crítico (`feature/SKILL.md` = pipeline core) com approval próprio + content preservation grep mais agressivo (AC-3 com 11 tokens-chave).

## Risks (após 3-wave split)

- **Alto → Médio:** feature/SKILL.md agora tem spec isolada, AC-3 com grep de 11 tokens-chave, e approval próprio antes do EXECUTE. Se algo estranhar, reject isolado sem afetar git/scan.
- **Baixo:** `.claude/` mirror desatualizar — mitigado por mirror task explícito em ambas 2a e 2b.
- **Baixo:** `skill-validate.js` unstaged changes (factual mode, +403 linhas) — ESTENDER, não sobrescrever.

## Execution Order

1. User approves wave plan.
2. EXECUTE wave 1 (inline ou via /resume).
3. Verify wave 1 CLOSED (hooks + gates ativos em `warn`).
4. EXECUTE wave 2a.
5. Verify wave 2a CLOSED (git + scan already ≤200, mirror ok, AC pass).
6. User re-reviews novo feature/SKILL.md plano antes de wave 2b — opcional mas recomendado.
7. EXECUTE wave 2b.
8. CLOSE epic.
