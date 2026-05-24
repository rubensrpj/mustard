# W7 — Templates cuts + opt-in skills

### Stage: Plan
### Outcome: Active
### Phase: PLAN
### Scope: full
### Checkpoint: 2026-05-24T19:30:00Z
### Lang: pt-BR
### Parent: 2026-05-24-mustard-unification

## Contexto

Os `SKILL.md` em `apps/cli/templates/commands/mustard/` somam ~2630 linhas. Alvo: ≤1100 (-56%). Cada `SKILL.md` cortado delega para um dos 15 subcomandos novos (W6 entregou). Skills 3rdparty grandes (hallmark com 100+ arquivos, design-craft, grill-me, react-best-practices) viram opt-in via `mustard add skill:nome`, instaladas em cache global `~/.claude/skill-cache/`. Refs duplicados consolidados em fonte única.

## Tarefas

### T7.1 — Cortes nos SKILL.md (12 arquivos)

Cada corte segue padrão: SKILL.md fica com (1) frontmatter, (2) Trigger, (3) Description ≤3 linhas, (4) Action — só rodar binário + branch no JSON, (5) Pointer a refs progressivos, (6) Rules ≤6 linhas.

| Arquivo | Linhas atuais | Alvo | Subcomando W6 |
|---|---|---|---|
| `feature/SKILL.md` | 353 | 90 | `spec-scaffold` + `spec-lang resolve` + refs novos |
| `bugfix/SKILL.md` | 240 | 70 | `spec-scaffold` + `bugfix-cache` |
| `close/SKILL.md` | 220 | 50 | `close-orchestrate` |
| `review/SKILL.md` | 178 | 60 | `review-dispatch` |
| `prd/SKILL.md` | 167 | 30 | `prd-build` (wrapper puro) |
| `skill/SKILL.md` | 161 | 60 | `skill-fetch` + `skill-cache` |
| `tactical-fix/SKILL.md` | 135 | 40 | `tactical-fix-create` |
| `task/SKILL.md` | 131 | 70 | `task-checklist` |
| `qa/SKILL.md` | 115 | 40 | (já delega a `qa-run`; só corte de verbosidade) |
| `knowledge/SKILL.md` | 118 | 50 | (já delega; corte) |
| `maint/SKILL.md` | 104 | 40 | `maint-deps` + `maint-validate` |
| `spec/SKILL.md` | 157 | 120 | (já delega; corte leve) |

Outros 6 SKILL.md (`scan`, `stats`, `status`, `git`, `unhook`, `rehook`) só cleanup ≤10 linhas cada.

### T7.2 — Skills opt-in

- [ ] Mover `apps/cli/templates/skills/{hallmark, design-craft, react-best-practices, grill-me}/` → `apps/cli/templates-extras/skills/`.
- [ ] Mover subdirs grandes do `skill-creator/`: `scripts/`, `agents/`, `assets/`, `eval-viewer/`, `references/` → `templates-extras/skills/skill-creator/`. Manter só `SKILL.md` base no default.
- [ ] Estender `apps/cli/src/commands/add.rs` para aceitar `skill:<nome>` como tipo. `mustard add skill:hallmark` invoca `mustard-rt run skill-fetch --name hallmark` (W6 entregou) + symlink em `.claude/skills/hallmark/`.
- [ ] Atualizar `apps/cli/src/commands/init.rs` para mostrar mensagem final listando extras disponíveis: "Optional skills available: hallmark (landing pages), design-craft (UI), grill-me, react-best-practices. Run `mustard add skill:<nome>` to install."

### T7.3 — Tradução en-US consistente

- [ ] Traduzir trechos pt-BR remanescentes em SKILL.md para en-US: `feature/SKILL.md § Grill Opt-In`, `spec/SKILL.md`, `prd/SKILL.md` inteiro.
- [ ] Banners hardcoded em pt-BR (~50 ocorrências) viram `i18n.key("...")` no Rust (W4 entregou `i18n.rs`).
- [ ] AC: `rg -E "(Está|Você|Aprovar)" templates/commands/mustard/**/*.md | wc -l` cai de ~50 para 0.

### T7.4 — Refs progressivos consolidados

Novos refs (extraídos de SKILL.md):

- `apps/cli/templates/refs/feature/analyze-protocol.md`
- `apps/cli/templates/refs/feature/plan-protocol.md`
- `apps/cli/templates/refs/feature/execute-protocol.md`
- `apps/cli/templates/refs/bugfix/diagnose-protocol.md`
- `apps/cli/templates/refs/bugfix/retry-cache.md`
- `apps/cli/templates/refs/task/action-bridge.md`
- `apps/cli/templates/refs/task/domain-checklists.md`
- `apps/cli/templates/refs/knowledge/capture-at-close.md`

Refs expandidos (já existem; absorvem texto duplicado de SKILL.md):

- `refs/feature/ac-cross-shell.md` — fonte única
- `refs/feature/wave-decomposition.md` — fonte única
- `refs/feature/spec-language.md` — só Header Translation Table; resolução roda em Rust via `spec-lang resolve`
- `refs/scan/scan-format.md` — recebe "Sourcing rule" de scan/SKILL.md

### T7.5 — Remover adapter.js bun

- [ ] Deletar `apps/cli/templates/adapters/cursor/adapter.js`.
- [ ] Atualizar `apps/cli/templates/adapters/cursor/README.md` para apontar para `mustard-rt run adapt-cursor` (W6).

### T7.6 — Cleanup de pinos órfãos

- [ ] Remover referência python órfã em `apps/cli/templates/commands/mustard/skill/SKILL.md:~122` (`python .claude/skills/skill-creator/scripts/run_loop.py`) — agora skill-creator subdir é opt-in.
- [ ] Auditar e remover referências a `node`/`bun` que sobraram em SKILL.md ou refs.

### T7.7 — Auditar recipes

- [ ] `apps/cli/templates/recipes/{add-component,add-endpoint,add-field,add-validation,null-guard}.json`: contar matches em telemetria (eventos `recipe.matched` últimos 90d). Se 0, mover para `templates-extras/recipes/`.

### T7.8 — Settings.json

- [ ] Revisar `apps/cli/templates/settings.json` após W5 (NDJSON), W8 (novos hooks), W10 (Stop/Notification). Adicionar entries novas, remover legacy se houver.

### T7.9 — Pipeline-config.md

- [ ] Revisar `apps/cli/templates/pipeline-config.md`. Consolidar política em refs onde aplicável.

### T7.10 — Economy events

- [ ] Cada corte de SKILL.md gera `pipeline.economy.skill.loaded { skill, size_bytes_before, size_bytes_after }` quando carregado pela primeira vez (instrumentado via hook `PreToolUse(Skill)`).

## Files

- `apps/cli/templates/commands/mustard/{feature,bugfix,close,review,prd,skill,tactical-fix,task,qa,knowledge,maint,spec,scan,stats,status,git,unhook,rehook}/SKILL.md` (18 arquivos)
- `apps/cli/templates/skills/{hallmark,design-craft,react-best-practices,grill-me}/` → `apps/cli/templates-extras/skills/` (mover)
- `apps/cli/templates/skills/skill-creator/{scripts,agents,assets,eval-viewer,references}/` → `apps/cli/templates-extras/skills/skill-creator/`
- `apps/cli/templates/refs/feature/{analyze,plan,execute}-protocol.md` (novos)
- `apps/cli/templates/refs/{bugfix/diagnose-protocol,bugfix/retry-cache,task/action-bridge,task/domain-checklists,knowledge/capture-at-close}.md` (novos)
- `apps/cli/templates/adapters/cursor/adapter.js` (DELETAR)
- `apps/cli/templates/adapters/cursor/README.md` (atualizar)
- `apps/cli/src/commands/add.rs` (tipo "skill")
- `apps/cli/src/commands/init.rs` (mensagem final extras)
- `apps/cli/templates/settings.json` (revisar)
- `apps/cli/templates/pipeline-config.md` (revisar)

## Critérios de Aceitação

- [ ] **AC-7.1.** Soma de linhas dos 18 SKILL.md ≤ 1100. Command: `node -e "const{execSync}=require('child_process');const out=execSync('find apps/cli/templates/commands/mustard -name SKILL.md -exec wc -l {} +',{encoding:'utf8'});const total=out.split('\\n').reverse().find(l=>l.includes('total'));const n=parseInt(total.trim().split(/\\s+/)[0]);if(n>1100){console.error('total',n);process.exit(1)}"`
- [ ] **AC-7.2.** Skills opt-in não estão em `apps/cli/templates/skills/`. Command: `node -e "const fs=require('fs');for(const s of ['hallmark','design-craft','react-best-practices','grill-me']){if(fs.existsSync('apps/cli/templates/skills/'+s))process.exit(1)}"`
- [ ] **AC-7.3.** `mustard add skill:hallmark` em projeto canário cria symlink em `.claude/skills/hallmark/`. Command: manual em projeto canário.
- [ ] **AC-7.4.** `mustard init` mostra mensagem de extras disponíveis. Command: `node -e "const t=require('fs').readFileSync('apps/cli/src/commands/init.rs','utf8');if(!/Optional skills|mustard add skill:/i.test(t))process.exit(1)"`
- [ ] **AC-7.5.** `adapter.js` deletado; `adapt-cursor` referenciado no README. Command: `node -e "const fs=require('fs');if(fs.existsSync('apps/cli/templates/adapters/cursor/adapter.js'))process.exit(1);const r=fs.readFileSync('apps/cli/templates/adapters/cursor/README.md','utf8');if(!/adapt-cursor/.test(r))process.exit(1)"`
- [ ] **AC-7.6.** Banners pt-BR hardcoded em SKILL.md ≤ 3 ocorrências. Command: `node -e "const{execSync}=require('child_process');const out=execSync('rg -E \"(Está|Você|Aprovar)\" apps/cli/templates/commands/mustard --type md',{encoding:'utf8'}).trim();if(out.split('\\n').filter(Boolean).length>3)process.exit(1)"`
- [ ] **AC-7.7.** Referência python órfã removida de skill/SKILL.md. Command: `node -e "const t=require('fs').readFileSync('apps/cli/templates/commands/mustard/skill/SKILL.md','utf8');if(/python .*run_loop\\.py/.test(t))process.exit(1)"`

## Notas

- Bloqueia nada (paralelizável com W8).
- Recipes auditados em T7.7: se um for movido, registra em `templates-extras/recipes/`.
- Cada SKILL.md cortado precisa preservar `description:` no frontmatter (Claude Code roteia por isso).
