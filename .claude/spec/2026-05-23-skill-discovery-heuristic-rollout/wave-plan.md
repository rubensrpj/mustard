# Wave Plan — skill-discovery-heuristic-rollout

### Stage: Close
### Outcome: Completed
### Flags:
### Scope: full (wave plan)
### Checkpoint: 2026-05-23T19:30:00Z
### Lang: pt
### Total waves: 3

## Waves

| N | Role | Modelo | Files | Depends on | Rationale |
|---|------|--------|-------|------------|-----------|
| 1 | rt | sonnet | `.claude/pipeline-config.md`, `apps/cli/templates/pipeline-config.md`, `apps/rt/src/run/doctor.rs` (EDIT — adicionar check), `apps/rt/src/run/skill_discovery_lint.rs` (NEW) | none | Funda a regra antes do rollout — heurística doc + lint enforcement |
| 2 | rt | sonnet | `apps/rt/src/run/status.rs` (NEW), `apps/rt/src/run/skills.rs` (EDIT — adicionar `list`), `apps/rt/src/run/memory.rs` (EDIT — adicionar `--grouped --format table` ao `list`), `apps/rt/src/run/knowledge.rs` (NEW — `glossary`), `apps/rt/src/run/review_prefetch.rs` (NEW), `apps/rt/src/run/mod.rs` (EDIT) | wave 1 | Implementa todos os subcomandos novos/estendidos do `mustard-rt` — fonte de verdade para os SKILLs |
| 3 | cli | sonnet | `apps/cli/templates/commands/mustard/status/SKILL.md` (+mirror em `.claude/commands/mustard/status/SKILL.md`), `apps/cli/templates/commands/mustard/skill/SKILL.md` (+mirror), `apps/cli/templates/commands/mustard/knowledge/SKILL.md` (+mirror), `apps/cli/templates/commands/mustard/review/SKILL.md` (+mirror), `apps/cli/templates/commands/mustard/bugfix/SKILL.md` (+mirror), `apps/cli/templates/commands/mustard/qa/SKILL.md` (+mirror) | wave 2 | Atualiza todos os SKILLs para chamar os binários de wave 2; aplica a heurística de wave 1 |

## Acceptance Criteria (cross-wave)

- **AC-G1** — Heurística existe em ambos `pipeline-config.md` (raiz + templates) com mesmo conteúdo:
  ```bash
  rtk node -e "const fs=require('fs'); const a=fs.readFileSync('.claude/pipeline-config.md','utf8'); const b=fs.readFileSync('apps/cli/templates/pipeline-config.md','utf8'); if(!a.includes('Skill Discovery Heuristic')) throw new Error('root miss'); if(!b.includes('Skill Discovery Heuristic')) throw new Error('templates miss');"
  ```

- **AC-G2** — Lint `mustard-rt run doctor --check skill-discovery` existe e retorna JSON:
  ```bash
  rtk mustard-rt run doctor --check skill-discovery --format json
  ```

- **AC-G3** — Após o rollout, lint reporta 0 violações nos SKILLs do repo:
  ```bash
  rtk node -e "const r=JSON.parse(require('child_process').execSync('rtk mustard-rt run doctor --check skill-discovery --format json').toString()); if(r.violations && r.violations.length > 0) throw new Error('viol: '+JSON.stringify(r.violations));"
  ```

- **AC-G4** — Cada novo/estendido subcomando responde a `--help` com exit 0:
  ```bash
  rtk mustard-rt run status --help && rtk mustard-rt run skills list --help && rtk mustard-rt run knowledge glossary --help && rtk mustard-rt run review-prefetch --help
  ```

- **AC-G5** — `mustard-rt run status --harness --format json` retorna JSON válido com chave `hooks`:
  ```bash
  rtk node -e "const r=JSON.parse(require('child_process').execSync('rtk mustard-rt run status --harness --format json').toString()); if(!r.hooks) throw new Error('hooks missing');"
  ```

- **AC-G6** — `mustard-rt run skills list --format json` retorna pelo menos 1 skill:
  ```bash
  rtk node -e "const r=JSON.parse(require('child_process').execSync('rtk mustard-rt run skills list --format json').toString()); if(!Array.isArray(r.skills) || r.skills.length === 0) throw new Error('skills empty');"
  ```

- **AC-G7** — `mustard-rt run knowledge glossary --format json` retorna entidades (ou vazio se registry vazio):
  ```bash
  rtk node -e "const r=JSON.parse(require('child_process').execSync('rtk mustard-rt run knowledge glossary --format json').toString()); if(!Array.isArray(r.entities)) throw new Error('entities not array');"
  ```

- **AC-G8** — SKILLs atualizados não contêm telltale phrases LLM-side:
  ```bash
  rtk node -e "const fs=require('fs'); const paths=['apps/cli/templates/commands/mustard/status/SKILL.md','apps/cli/templates/commands/mustard/skill/SKILL.md','apps/cli/templates/commands/mustard/knowledge/SKILL.md']; const banned=['Glob ' + String.fromCharCode(96) + '.claude/skills',`parse YAML frontmatter of each`,`Iterate ` + String.fromCharCode(96) + `registry.e`]; for(const p of paths){const s=fs.readFileSync(p,'utf8'); for(const b of banned){if(s.includes(b)) throw new Error(p+': '+b);}}"
  ```

## Critical Path

Wave 1 → Wave 2 → Wave 3 sequencial. Dentro de wave 2, os 5 subcomandos podem ser implementados em paralelo (não compartilham state). Dentro de wave 3, os 6 SKILLs podem ser editados em paralelo.
