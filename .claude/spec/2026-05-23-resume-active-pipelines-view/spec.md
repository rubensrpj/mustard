# Tactical Fix: `/mustard:resume` lê 117 paths porque filtra por `Status:` morto

## Contexto

`/mustard:resume` consumiu ~117 paths + 6 Greps redundantes numa única invocação. Diagnóstico em 4 raízes (todas decorrem de drift entre a unificação de lifecycle de spec e a skill que lê o cabeçalho):

1. **Filtro morto** — `SKILL.md` linha 72 do `/mustard:resume` ainda manda filtrar por `### Status: (draft|approved|implementing|closed-followup)`. O parent ([[2026-05-21-spec-lifecycle-unification]]) trocou para `### Stage:` + `### Outcome:`. Resultado: Grep volta 0 linhas; orquestrador faz discovery manual (Greps de `Status:` repetidos, depois `### `, depois `### (Stage|Outcome):`).
2. **Glob global como primeiro passo** — Step 1 da skill faz `Glob .claude/spec/*/spec.md` (100 hits) + `Glob .claude/spec/*/wave-plan.md` (17 hits) **antes** de qualquer filtro semântico. A skill confia no filtro de Status para colapsar; quando o filtro virou no-op (raiz #1), os 117 paths entraram no contexto inteiros.
3. **Sem view "active-pipelines" no `event-projections`** — todos os views existentes (`pipeline-state`, `agent-visibility`, `session-summary`, `epic-summary`) exigem `--spec`. Não há query "me devolva specs com `lastEventAt` recente e último `pipeline.stage` ≠ `Close`". O orquestrador tentou `--view active-pipelines` (inventando), errou, gastou `--help` para descobrir os views válidos.
4. **`docs-stale-check` não pegou** — o `obsolete_terms` declarado pelo parent (`.claude/.docs-audit.json`) provavelmente não inclui `### Status:` como termo a banir, ou não escaneia `apps/cli/templates/commands/mustard/resume/SKILL.md`. Memória [[project_docs_audit_process]] descreve o sensor; este caso é exatamente o que ele existe para evitar.

A correção é cirúrgica: atualizar o filtro da skill (uma string) e adicionar UM view no `event-projections`. ≤100 LOC totais.

## Critérios de Aceitação

- [x] AC-1: filtro do /mustard:resume reconhece lifecycle novo (Stage/Outcome) — Command: `node -e "const fs=require('fs');const s=fs.readFileSync('apps/cli/templates/commands/mustard/resume/SKILL.md','utf8');if(/Filter by \`Status:\` header/.test(s))process.exit(1);if(!/Stage:/.test(s)||!/Outcome:/.test(s))process.exit(2);console.log('ok')"`
- [x] AC-2: view active-pipelines existe no binário — Command: `node -e "const {execSync}=require('child_process');const h=execSync('mustard-rt run event-projections --help',{encoding:'utf8'});if(!/active-pipelines/.test(h))process.exit(1);const j=JSON.parse(execSync('mustard-rt run event-projections --view active-pipelines',{encoding:'utf8'}));if(!Array.isArray(j.pipelines))process.exit(2);console.log('ok')"`
- [x] AC-3: cada item do view traz spec+lastEventAt+stage truthy — Command: `node -e "const {execSync}=require('child_process');const j=JSON.parse(execSync('mustard-rt run event-projections --view active-pipelines',{encoding:'utf8'}));if(j.pipelines.length===0){console.log('ok-empty');process.exit(0)}const p=j.pipelines[0];if(!p.spec||!p.lastEventAt||!p.stage)process.exit(1);console.log('ok')"`
- [x] AC-4: SKILL.md referencia active-pipelines antes do Glob — Command: `node -e "const s=require('fs').readFileSync('apps/cli/templates/commands/mustard/resume/SKILL.md','utf8');const i=s.indexOf('active-pipelines');const g=s.indexOf('Glob');if(i===-1)process.exit(1);if(g===-1||i>g)process.exit(2);console.log('ok')"`
- [x] AC-5: docs-audit declara Status: como termo obsoleto — Command: `node -e "const cfg=JSON.parse(require('fs').readFileSync('.claude/.docs-audit.json','utf8'));if(!/Status:/.test(JSON.stringify(cfg)))process.exit(1);console.log('ok')"`

## Arquivos

- `apps/cli/templates/commands/mustard/resume/SKILL.md` — Step 1: troca filtro `Status:` por `Stage: ≠ Close` + `Outcome: ≠ Completed`; reordena Step 1 para chamar `event-projections --view active-pipelines` **antes** do Glob (Glob vira fallback quando o view está vazio ou indisponível)
- `apps/rt/src/run/event_projections.rs` (ou equivalente) — adicionar branch para `--view active-pipelines`; query: `SELECT spec, MAX(at) AS lastEventAt, last(stage) FROM pipeline_events GROUP BY spec HAVING last(stage) != 'Close' AND lastEventAt > now() - interval N days`
- `apps/rt/src/main.rs` — `--help` enumera o novo view
- `.claude/.docs-audit.json` — adicionar `"### Status:"` ao `obsolete_terms` do parent `2026-05-21-spec-lifecycle-unification`
- (espelho) `.claude/commands/mustard/resume/SKILL.md` — cópia instalada (memória [[feedback_mustard_self_scripts_stale.md]]: cópia stale; mas QA roda contra templates/)
