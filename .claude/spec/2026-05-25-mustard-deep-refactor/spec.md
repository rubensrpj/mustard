# Mustard Deep Refactor — pipeline + scan + skills + memória + economia
### Stage: Plan
### Outcome: Active
### Flags: 
### Checkpoint: 2026-05-25T18:42:00Z

## PRD

## Contexto

Após 6 meses de evolução, o Mustard tem três camadas que precisam ser reescritas juntas para o produto ficar coerente:

**1. Criação de spec.** O comando `/feature` (`apps/cli/templates/commands/mustard/feature/SKILL.md` — 354 linhas) ainda carrega template literal de spec, decisão de scope por prosa, lang resolution com formato curto (`Lang: pt`), referências a headers que não existem mais (`### Stage:`, `### Outcome:` — substituídos por `meta.json`). É verboso e defasado.

**2. Injeção entre agentes.** O `agent-prompt-render` (Rust) renderiza prompts para Task agents, mas dois placeholders críticos são preenchidos pelo LLM via prosa: `{recommended_skills}` (LLM lê texto e escolhe) e `{guards_summary}` (extração regex frágil de CLAUDE.md). Resultado: variabilidade entre runs, e skills geradas pelo scan nunca chegam ao prompt porque o LLM não as conhece.

**3. Montagem de skills.** As 13 skills foundation em `templates/skills/` têm frontmatter inconsistente. Skills geradas pelo scan saem em formato livre. Não há validador Rust nem `skill-resolve` (matching agnóstico). Hoje o orquestrador depende da intuição do LLM.

E há resíduo arquitetural:
- A `2026-05-24-mustard-unification` entregou W0-W4 mas tem W5 em curso + W6-W17 ainda como plano. Decisão 2026-05-25: encerrar a mega-spec e migrar o restante para esta nova spec.
- 136 specs históricas estavam em `.claude/spec/` poluindo o picker. Decisão: todas movidas para `~/.mustard-backups/2026-05-25-specs-archive/`. Apenas esta nova spec é ativa.
- Recipes hardcoded genéricos, refs defasados com cabeçalhos extintos, graph com nós entity/enum poluindo escopo de pipeline — tudo tratado nas waves desta spec.

Princípios consolidados nas memórias [[feedback_mustard_agnostic]], [[feedback_scan_rust_first]], [[feedback_no_hardcoded_stack_patterns]], [[feedback_recipes_from_scan]], [[feedback_graph_pipeline_knowledge]], [[feedback_templates_md_moat]]: Mustard é ferramenta agnóstica; nada hardcoded de stack; estrutural em Rust, IA só para interpretação semântica nomeável; templates `.md` são moat — devem ser enxutos e atuais.

## Usuários/Stakeholders

- **Rubens** (operador único atual) — usa Mustard via CLI + dashboard local; quer pipeline que produza specs/injeção/skills consistentes em qualquer projeto-alvo, sem viés do projeto Mustard em si.
- **O próprio Mustard** (hooks, comandos, dashboard, scan) — passa a consumir 5+ subcomandos Rust novos (`spec-draft`, `skill-resolve`, `scan-structural`, `scan-md-validate`, etc.).
- **Quem mantém o código** — herda 12 commands enxutos (≤67 linhas média vs ~190 hoje), 18 refs reduzidos (~10 sobreviventes), skills com frontmatter padronizado, `mustard.db` redesenhado.

## Métrica de sucesso

- Cobertura do `entity-registry.json` em projeto canário: ≥99% dos arquivos com declaração pública/exportada (hoje ~6% no próprio Mustard).
- Tokens prompt do scan-agent: ≤80 linhas (hoje ~250).
- Tamanho médio dos 12 `commands/mustard/*/SKILL.md`: ≤67 linhas (hoje ~190).
- `mustard-rt run active-specs` retorna apenas `2026-05-25-mustard-deep-refactor`.
- Build verde (`cargo build --workspace && cargo clippy --workspace -- -D warnings`, `pnpm --filter mustard-dashboard build`).
- `mustard.db` em projeto canário ≤1 MB pós-cleanup.

## Não-Objetivos

- Reabsorver o que já foi entregue na `2026-05-24-mustard-unification` (W0-W4: clippy/docs-stale + worktree-gc + scan cold-path subprocess + meta.json + i18n). Citado como fundação, não reimplementado.
- Restaurar `mustard-v1-installer-and-update` ou `dashboard-prd-ai-lapidator` como specs ativas — marcadas como Cancelled nesta sessão (deferred; podem ser reabertas como novas specs futuras).
- Migrar dados de specs antigas para o novo `mustard.db`: fase dev, drop limpo ([[feedback_no_migration_dev_phase]]).
- Reintroduzir SDK Anthropic em Rust — todo LLM call via subprocess `claude` CLI ([[feedback_llm_via_claude_cli]]).
- Hardcodear catálogo de padrões esperados por stack — tudo emerge do filesystem via heurística agnóstica ([[feedback_no_hardcoded_stack_patterns]]).
- Reescrever cold-path `scan/interpret.rs` — entregue em W2 da mega-spec, fica como está.
- Manter componente de grafo interno do dashboard — wikilinks abrem no Obsidian via URI scheme ([[project_dashboard_no_graph_obsidian]]).
- Estimar economia em prosa — todo delta passa por `economy_baselines`/`economy_savings` reais no `telemetry.db` ([[feedback_everything_measurable]]).

## Critérios de Aceitação

ACs autoritativos vivem em cada `wave-N-{role}/spec.md` (13 ondas). `/mustard:qa` agrega no momento da execução. ACs globais agregados:

- [ ] **AC-G1.** `cargo build --workspace && cargo clippy --workspace -- -D warnings` passa após todas as ondas. Command: `rtk cargo build --workspace && rtk cargo clippy --workspace -- -D warnings`
- [ ] **AC-G2.** `pnpm --filter mustard-dashboard build && pnpm --filter mustard-dashboard lint` passa. Command: `rtk pnpm --filter mustard-dashboard build && rtk pnpm --filter mustard-dashboard lint`
- [ ] **AC-G3.** `mustard-rt run active-specs --format json` retorna apenas esta spec. Command: `rtk mustard-rt run active-specs --format json | rtk node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{const j=JSON.parse(s);const names=j.specs.map(x=>x.name);if(names.length!==1||names[0]!=='2026-05-25-mustard-deep-refactor')process.exit(1)})"`
- [ ] **AC-G4.** Subcomandos novos `spec-draft`, `skill-resolve`, `scan-structural`, `scan-md-validate`, `scan-recipes-validate` registrados. Command: `rtk mustard-rt run --help 2>&1 | rtk node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{for(const k of ['spec-draft','skill-resolve','scan-structural','scan-md-validate','scan-recipes-validate']){if(!s.includes(k))process.exit(1)}})"`
- [ ] **AC-G5.** `entity-registry.json` ≥99% cobertura em projeto canário (declarações públicas/exportadas).
- [ ] **AC-G6.** Soma de linhas dos `commands/mustard/*/SKILL.md` ≤ 800 (hoje ~2300). Command: `rtk node -e "const fs=require('fs'),p=require('path');function walk(d,out){for(const e of fs.readdirSync(d)){const f=p.join(d,e);const s=fs.statSync(f);if(s.isDirectory())walk(f,out);else if(e==='SKILL.md')out.push(f)}return out}const total=walk('apps/cli/templates/commands/mustard',[]).reduce((s,f)=>s+fs.readFileSync(f,'utf8').split(String.fromCharCode(10)).length,0);if(total>800)process.exit(1)"`
- [ ] **AC-G7.** Schema `mustard.db` limpo (sem `events`/`events_fts`/`knowledge` legacy/`metrics_projection`). Command: validado em wave dedicada.
- [ ] **AC-G8.** Skill frontmatter padronizado em 100% das skills foundation + geradas pelo scan. Validado por `mustard-rt run skills validate --strict-frontmatter`.

## Plano

Ver `wave-plan.md`. Resumo:

| W | Nome | Role | Depende | Status |
|---|------|------|---------|--------|
| 0 | residual-w5 (encerra T5.2-T5.8 da mega-spec) | mixed | — | 📋 |
| 1 | spec-injection-skills-refactor (3 pilares) | mixed | 0 | 📋 (gargalo) |
| 2 | claude-dir-prune | mixed | 0 | 📋 (T2.1 já feito manualmente) |
| 3 | scan-rust-first-agnostic | mixed | 0 | 📋 |
| 4 | archive-completed-specs | rt | — | 📋 |
| 5 | rt-new-subcommands | rt | 1, 3 | 📋 |
| 6 | templates-cuts | cli | 1 | 📋 (menor após W1) |
| 7 | shared-memory-hardening | rt | 1, 5 | 📋 |
| 8 | context-injection-optimization | rt | 1, 5, 7 | 📋 |
| 9 | stop-notification-triggers | rt | 5 | 📋 |
| 10 | verify-pipeline-multistack + wave-integrity-doctor | mixed | 0, 5 | 📋 |
| 11 | telemetry-perf + economy-wiring | mixed | 5, 8 | 📋 |
| 12 | close-and-archive | mixed | 0-11 | 📋 |
