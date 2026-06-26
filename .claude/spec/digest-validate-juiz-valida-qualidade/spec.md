---
id: spec.digest-validate-juiz-valida-qualidade
---

# digest-validate juiz valida qualidade de recuperacao conceito central achado e roda sempre apos o digest

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

digest-validate juiz valida qualidade de recuperacao conceito central achado e roda sempre apos o digest.

Âncoras (do scan):
- apps/rt/src/commands/agent/digest_validate.rs (digest, validate, verdict, render)
- apps/scan/src/digest.rs (digest, concept, tier)
- packages/core/src/domain/economy/reader.rs (route, scope)
- apps/cli/templates/skills/skill-creator/scripts/quick_validate.py (validate)
- apps/dashboard/src/pages/Economia.tsx (tier, scope)
- apps/dashboard/src-tauri/src/telemetry.rs (route, scope)
- apps/mcp/src/lib.rs (scope)
- apps/rt/src/commands/agent/concern_judge.rs (render, concept, tier, judge)
- apps/rt/src/commands/knowledge/recall_cli.rs (render, scope, recall)
- apps/rt/src/commands/lexicon_judge.rs (render, judge)
- apps/rt/src/commands/knowledge/recall.rs (scope, recall)
- apps/rt/src/commands/agent/agent_prompt_render.rs (render, tier, recall)

Fatias recorrentes (precedente a espelhar): Opts (×2)

Por que agora.

## Usuários/Stakeholders

Quem se beneficia.

## Métrica de sucesso

Métrica de sucesso.

## Não-Objetivos

O que fica de fora.

## Critérios de Aceitação

- **AC-1** — Pipeline build green
  Command: `cargo build`

## Checklist

- [ ] T1 — primeira tarefa rastreável.
