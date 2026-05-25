# W1 — Refatoração dos 3 pilares: spec creation + injeção entre agentes + montagem de skills

## Contexto

Gargalo arquitetural. Hoje `/feature` SKILL.md tem 354 linhas misturando mecânica, decisão tabelada, template literal e prosa. Skills foundation têm frontmatter inconsistente. `{recommended_skills}` no prompt do Task é escolhido por prosa do LLM. W1 entrega 3 mecanismos Rust que destravam W6/W7/W8 e reduzem escopo deles.

## Tarefas

- [ ] **T1.1** — Contrato de spec em `packages/core/src/spec/contract.rs` (novo). Define shape exato de `spec.md` + `meta.json` + (Full) `wave-plan.md` byte-stable. Inclui: enum `Stage`/`Outcome`/`Phase`/`Scope` + serde, layout obrigatório (PRD divider → Contexto/Usuários/Métrica/Não-Objetivos/AC → Plano divider → Arquivos/Tarefas/Limites), AC com Command runnable, Lang BCP-47.
- [ ] **T1.2** — `mustard-rt run spec-draft --intent "..." --scope light|full --lang pt-BR|en-US [--signals layers,files,...]`. Gera `spec.md` + `meta.json` + (se Full) `wave-plan.md` + `wave-N-{role}/spec.md` conforme T1.1. Substitui ~80 linhas de template em `feature/SKILL.md`.
- [ ] **T1.3** — Schema de frontmatter de skill (`packages/core/src/skill/frontmatter.rs` novo): `name`, `description`, `tags: [add|fix|refactor|review|...]`, `appliesTo: [{cluster-label}, ...]` (vazio = qualquer), `scope: [code-editing|review|plan|analyze]`, `entities: [...]` (opt), `metadata: {generated_by: scan|foundation, cluster?: {label}}`.
- [ ] **T1.4** — `mustard-rt run skill-resolve --intent "..." --subproject {sub} --phase {ANALYZE|EXECUTE|REVIEW}`. Rust faz matching agnóstico: parse leve do intent (verbo + nouns), cross com `entity-registry.json`, lista skills disponíveis (`templates/skills/` + `{sub}/.claude/skills/`), score por (tag-match + applies-match + entity-match + scope-match), output JSON top-K. Zero IA.
- [ ] **T1.5** — Refatorar `agent-prompt-render`: `{recommended_skills}` consumir `skill-resolve`; padronizar `{guards_summary}` (extração estruturada de `CLAUDE.md` em vez de regex frágil); cache por wave para não recomputar; contrato byte-stable de placeholders. **Filtra também princípios técnicos da `memory/` da spec ativa pelo mesmo critério (wirelink ↔ cluster da tarefa)** — princípios irrelevantes não entram no prompt.
- [ ] **T1.6** — Migrar 13 skills foundation em `templates/skills/` para o schema T1.3. Mantém content; apenas frontmatter padronizado. Cada uma com `tags`/`appliesTo`/`scope`/`entities` declarados.
- [ ] **T1.7** — Validators Rust: `mustard-rt run spec-validate --spec X` (valida output do `spec-draft` contra contrato T1.1); `mustard-rt run skills validate --strict-frontmatter` (estende existente para validar T1.3 obrigatório). Build clippy verde.
- [ ] **T1.8** — Auditar refs/* que duplicam conteúdo dos commands: marcar deriváveis para virar subcomando (W5) ou ficar como prosa metodológica. Lista no final desta wave (entry para W6).

- [ ] **T1.9** — `spec-draft` (T1.2) cria também `memory/_index.md` (template stub) na pasta da spec gerada. Subcomando complementar `mustard-rt run spec-memory create --spec X --name Y --kind {principle|process|reference} --origin-wave WN` gera arquivo `memory/{name}.md` com frontmatter padronizado + wirelink automático para a spec + para a wave de origem + seções `## Origem`/`## Aplica-se a`/`## Status`/`## Relacionado`. Durante PLAN/EXECUTE, princípios novos descobertos viram arquivos via este subcomando. Em `apps/rt/src/run/spec_memory.rs` (novo).

## Critérios de Aceitação

- [ ] **AC-W1.1** — `mustard-rt run spec-draft --help` lista todas as flags. Command: `rtk mustard-rt run spec-draft --help`
- [ ] **AC-W1.2** — Spec gerada por `spec-draft` passa em `spec-validate`. Command: `rtk mustard-rt run spec-draft --intent "test" --scope light --lang pt-BR --output /tmp/test && rtk mustard-rt run spec-validate --spec /tmp/test`
- [ ] **AC-W1.3** — `mustard-rt run skill-resolve --help` existe. Command: `rtk mustard-rt run skill-resolve --help`
- [ ] **AC-W1.4** — 13 skills foundation têm frontmatter padronizado. Command: `rtk node -e "const fs=require('fs'),p=require('path');const root='apps/cli/templates/skills';for(const d of fs.readdirSync(root)){const f=p.join(root,d,'SKILL.md');if(!fs.existsSync(f))continue;const t=fs.readFileSync(f,'utf8');for(const k of ['tags:','appliesTo:','scope:']){if(!t.includes(k)){console.error('missing in',d,':',k);process.exit(1)}}}"`
- [ ] **AC-W1.5** — `agent-prompt-render` invoca `skill-resolve`. Command: `rtk node -e "const t=require('fs').readFileSync('apps/rt/src/run/agent_prompt_render.rs','utf8');if(!/skill_resolve|skill-resolve/.test(t))process.exit(1)"`
- [ ] **AC-W1.6** — `mustard-rt run skills validate --strict-frontmatter --json` retorna `ok: true` para skills foundation. Command: `rtk mustard-rt run skills validate --strict-frontmatter --json | rtk node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{const j=JSON.parse(s);if(!j.ok)process.exit(1)})"`

## Limites

`packages/core/src/spec/contract.rs` (novo), `packages/core/src/skill/frontmatter.rs` (novo), `apps/rt/src/run/spec_draft.rs` (novo), `apps/rt/src/run/skill_resolve.rs` (novo), `apps/rt/src/run/spec_validate.rs` (novo), `apps/rt/src/run/skills.rs` (estender), `apps/rt/src/run/agent_prompt_render.rs` (refator), `apps/rt/src/run/mod.rs` (registrar), `apps/cli/templates/skills/*/SKILL.md` (frontmatter migrado).

OUT: tudo fora. NÃO mexer em commands/mustard/*/SKILL.md aqui (W6 corta). NÃO mexer em recipes/scan/graph (W3).

## Role

mixed (core schema + rt subcomandos + cli templates skill foundation)
