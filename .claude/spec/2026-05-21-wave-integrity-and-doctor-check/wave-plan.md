# Integridade do scaffolder de waves + check no doctor

### Status: draft
### Phase: PLAN
### Scope: full (wave plan)
### Checkpoint: 2026-05-21T00:00:00Z
### Lang: pt
### Total waves: 4

## PRD

## Contexto

O scaffolder `mustard-rt run wave-scaffold` recebe um `plan.json` declarativo e itera todas as entradas de `plan.waves` para criar `wave-plan.md` + um diretório `wave-N-{role}/spec.md` por wave + `review/spec.md` + `qa/spec.md`. A iteração em `wave_scaffold.rs:306` está correta — quando o `plan.json` chega completo, todas as waves nascem como deveriam (a spec da flatten-spec rodando hoje é prova disso, com 6 wave-files materializados). O elo fraco mora antes: o `plan.json` é montado pelo orquestrador LLM dentro do SKILL `/feature`, baseado num texto descritivo da seção "Wave Decomposition" (linhas 138-156). Na prática isso falha de duas formas observadas: o LLM declara `total_waves: N` mas só preenche a primeira posição do array `waves`, e o scaffolder consome o array tal qual veio — gera `wave-plan.md` referenciando N waves, mas zero ou apenas um `wave-N-role/spec.md` no disco. Em silêncio. Sem stderr, sem exit não-zero. O resultado fica visível só quando outro colaborador (ou o próprio dev) abre o dashboard e vê uma spec multi-wave com diretórios faltando — exatamente o tipo de drift entre disco e canon que a flatten-spec-layout está tentando eliminar do outro lado. Esta spec ataca o lado do scaffolder: validação ruidosa no consumo do `plan.json`, montagem programática do JSON pelo Rust em vez de pelo LLM, e um check pós-fato no `doctor` que pega specs já criadas com discrepância.

## Usuários/Stakeholders

Qualquer pessoa que abre uma spec Full com decomposição de waves via `/mustard:feature` — diretamente afetada quando o scaffolder cria menos pastas do que o `wave-plan.md` declara, porque o pipeline depois trava em `wave-2-{role}/spec.md not found`. Pedido nasceu na análise da spec da flatten-spec-layout, onde o usuário perguntou explicitamente "por que quando uma spec tem mais de uma wave é criado apenas o arquivo da wave 1".

## Métrica de sucesso

Toda invocação de `wave-scaffold` com `plan.json` malformado falha visivelmente (`eprintln` + JSON com campo `error`) em vez de gerar artefatos órfãos. SKILL `/feature` deixa de pedir ao orquestrador para montar `plan.json` na cabeça — chama `mustard-rt run plan-from-spec` que sempre produz array completo. `mustard-rt run doctor` reporta WARN para qualquer spec ativa cujo `wave-plan.md` referencie waves que não existem como diretório. Ao abrir o dashboard, o desenvolvedor vê um badge sutil no footer da Sidebar que muda de cor quando o doctor detectaria WARN/FAIL — sem precisar lembrar de rodar `/maint doctor` manualmente.

## Não-Objetivos

- Hook automático que rode `wave-integrity` em `SessionStart` ou `close_gate`. Doctor continua reativo (feedback `analysis_pattern`: subtrair > adicionar). O badge da Wave 4 só re-roda quando o user clica refresh ou remontagem do app — sem polling.
- Página dedicada `Doctor.tsx` no dashboard. Wave 4 entrega só o badge na Sidebar com tooltip; página completa fica como follow-up se a UX se mostrar útil.
- Mudar o formato de `wave-plan.md` ou da tabela de waves. O parser do check apenas lê wikilinks existentes.
- Suporte ao layout legado `spec/active/`. Esta spec assume que vai rodar **depois** da flatten-spec-layout, então só conhece o layout flat `.claude/spec/{name}/`.
- Migrar specs já corrompidas no repo. O check só reporta; o usuário roda `wave-scaffold` de novo para preencher o que falta.
- Suporte a `plan-from-spec` parseando markdown de tabela existente (`--from-table file.md`). Primeira versão aceita só flags declarativas; tabela fica como follow-up.

## Critérios de Aceitação

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [ ] AC-1: Build full do workspace passa — Command: `cargo build --workspace`
- [ ] AC-2: Testes do rt passam, incluindo os novos de wave_scaffold + plan_from_spec + doctor::wave_integrity — Command: `cargo test -p mustard-rt`
- [ ] AC-3: `wave-scaffold` com `waves: []` retorna JSON contendo campo `error` e NÃO cria `wave-plan.md` — Command: `node -e "const cp=require('child_process');const fs=require('fs');const os=require('os');const path=require('path');const dir=fs.mkdtempSync(path.join(os.tmpdir(),'mst-'));const spec=path.join(dir,'spec');fs.mkdirSync(spec);const plan=path.join(dir,'plan.json');fs.writeFileSync(plan,JSON.stringify({waves:[],total_waves:0,lang:'pt'}));const out=cp.execSync('mustard-rt run wave-scaffold --spec-dir '+spec+' --plan '+plan).toString();const j=JSON.parse(out);if(!j.error)throw new Error('expected error field, got '+out);if(fs.existsSync(path.join(spec,'wave-plan.md')))throw new Error('wave-plan.md should not be created when waves is empty');"`
- [ ] AC-4: `wave-scaffold` com `total_waves: 3` e `waves: [{n:1,...}]` (mismatch) emite WARN em stderr e segue criando o que veio — Command: `node -e "const cp=require('child_process');const fs=require('fs');const os=require('os');const path=require('path');const dir=fs.mkdtempSync(path.join(os.tmpdir(),'mst-'));const spec=path.join(dir,'spec');fs.mkdirSync(spec);const plan=path.join(dir,'plan.json');fs.writeFileSync(plan,JSON.stringify({waves:[{n:1,role:'general',summary:'s',depends_on:[]}],total_waves:3,lang:'pt'}));const r=cp.spawnSync('mustard-rt',['run','wave-scaffold','--spec-dir',spec,'--plan',plan]);const stderr=r.stderr.toString();if(!/mismatch|total_waves/i.test(stderr))throw new Error('expected mismatch warning in stderr, got: '+stderr);"`
- [ ] AC-5: `mustard-rt run plan-from-spec --waves 2 --roles general,frontend --lang pt` emite JSON válido com `waves.length === 2` e `total_waves === 2` — Command: `node -e "const cp=require('child_process');const out=cp.execSync('mustard-rt run plan-from-spec --waves 2 --roles general,frontend --lang pt').toString();const j=JSON.parse(out);if(j.waves.length!==2||j.total_waves!==2)throw new Error('bad shape: '+out);if(j.waves[0].role!=='general'||j.waves[1].role!=='frontend')throw new Error('roles wrong: '+out);"`
- [ ] AC-6: SKILL `/feature` referencia `plan-from-spec` na seção Wave Decomposition — Command: `node -e "const fs=require('fs');const t=fs.readFileSync('apps/cli/templates/commands/mustard/feature/SKILL.md','utf8');if(!/plan-from-spec/.test(t))throw new Error('plan-from-spec missing from SKILL');"`
- [ ] AC-7: `mustard-rt run doctor` em projeto com `wave-plan.md` referenciando wave inexistente reporta WARN no check `wave-integrity` — Command: `bash -c 'd=$(mktemp -d); cd "$d"; mkdir -p .claude/spec/test-spec/wave-1-general; printf "# t\n\n| Wave | Spec | Role |\n|------|------|------|\n| 1 | [[wave-1-general]] | general |\n| 2 | [[wave-2-frontend]] | frontend |\n" > .claude/spec/test-spec/wave-plan.md; printf "# wave1\n" > .claude/spec/test-spec/wave-1-general/spec.md; printf "{}" > .claude/settings.json; out=$(mustard-rt run doctor 2>&1); echo "$out" | grep -qE "WARN\s+wave-integrity" && echo "$out" | grep -q "wave-2-frontend"'`
- [ ] AC-8: `mustard-rt run doctor --json` emite JSON parseável com array `checks` — Command: `node -e "const cp=require('child_process');const j=JSON.parse(cp.execSync('mustard-rt run doctor --json').toString());if(!Array.isArray(j.checks))throw new Error('checks not array: '+JSON.stringify(j));if(!j.checks.every(c=>c.name&&c.status))throw new Error('check missing fields: '+JSON.stringify(j))"`
- [ ] AC-9: Dashboard builda incluindo `DoctorBadge` e o componente é exportado — Command: `bash -c 'pnpm --filter mustard-dashboard build && grep -q "DoctorBadge" apps/dashboard/src/components/layout/Sidebar.tsx'`

## Plano

## Informações da Entidade

`Plan` (`apps/rt/src/run/wave_scaffold.rs:49`) — struct existente, ganha validação no `run()`. Sem mudança de schema.

`PlanFromSpecOptions` — nova struct em `apps/rt/src/run/plan_from_spec.rs` com flags declarativas → emite o mesmo shape de `Plan` em JSON.

`CheckResult` (`apps/rt/src/run/doctor.rs:51`) — struct existente, ganha um novo nome `wave-integrity` na lista de checks renderizados.

`RunCmd` (`apps/rt/src/run/mod.rs`) — enum existente, ganha variante `PlanFromSpec` e fica registrada em `KNOWN_RUN_SUBCOMMANDS` do doctor.

## Arquivos

Distribuídos por wave (cada wave-N tem `## Arquivos` próprio com a lista exata). Resumo cross-wave:

```
apps/rt/src/run/wave_scaffold.rs                       — wave 1: hard gate empty/mismatch + 2 testes
apps/rt/src/run/plan_from_spec.rs                      — wave 2: new (parse flags → emit JSON)
apps/rt/src/run/mod.rs                                 — wave 2: register PlanFromSpec
apps/rt/src/run/doctor.rs                              — wave 2: add "plan-from-spec" em KNOWN_RUN_SUBCOMMANDS; wave 3: rename collect_active_spec_names → collect_spec_names (flat) + nova check_wave_integrity; wave 4: flag --json
apps/cli/templates/commands/mustard/feature/SKILL.md   — wave 2: substituir "orquestrador monta plan.json" por chamada a plan-from-spec
apps/cli/templates/commands/mustard/maint/SKILL.md     — wave 3: documentar wave-integrity em # doctor section
apps/dashboard/src-tauri/src/lib.rs                    — wave 4: registrar novo Tauri command doctor_status
apps/dashboard/src-tauri/src/doctor.rs                 — wave 4: new (invoke mustard-rt run doctor --json + parse)
apps/dashboard/src/lib/doctor.ts                       — wave 4: new (hook useDoctorStatus + types)
apps/dashboard/src/components/DoctorBadge.tsx          — wave 4: new
apps/dashboard/src/components/layout/Sidebar.tsx       — wave 4: render DoctorBadge no footer
```

## Tarefas

Wave-by-wave; detalhes vivem em cada `wave-N-{role}/spec.md`. Resumo da árvore de dependências:

```
wave-1 (library: hard gate) ─┬─► wave-2 (general: plan-from-spec + SKILL)
                             └─► wave-3 (general: doctor wave-integrity) ─► wave-4 (frontend: dashboard badge)
```

Waves 2 e 3 são paralelas entre si — ambas só dependem de wave-1. Wave 4 só depende de wave-3 (precisa do check `wave-integrity` existir + flag `--json` no doctor para o badge consumir).

## Limites

- `apps/rt/src/run/wave_scaffold.rs`
- `apps/rt/src/run/plan_from_spec.rs` (new)
- `apps/rt/src/run/mod.rs`
- `apps/rt/src/run/doctor.rs`
- `apps/cli/templates/commands/mustard/feature/SKILL.md` (apenas seção Wave Decomposition)
- `apps/cli/templates/commands/mustard/maint/SKILL.md` (apenas tabela do doctor)
- `apps/dashboard/src-tauri/src/lib.rs` (apenas register do novo command)
- `apps/dashboard/src-tauri/src/doctor.rs` (new)
- `apps/dashboard/src/lib/doctor.ts` (new)
- `apps/dashboard/src/components/DoctorBadge.tsx` (new)
- `apps/dashboard/src/components/layout/Sidebar.tsx` (apenas inserção do badge no footer)

Out-of-boundary explicit: `apps/rt/src/run/exec_rewave_check.rs` (rota paralela de decomposição), `apps/rt/src/hooks/close_gate.rs` (não vira gate automático), demais páginas/componentes do dashboard (Topbar, SplitDetail, páginas de specs etc.), `.claude/spec/2026-05-21-flatten-spec-layout-and-multi-collab/` (spec rodando — intocada; assume layout flat após Wave 5 da flatten).

## Cobertura de Críticas

| Crítica do usuário | Bucket | Onde |
|---|---|---|
| "quando uma spec tem mais de uma wave é criado apenas o arquivo da wave 1" | Coberto | Wave 1 (gate ruidoso) + Wave 2 (causa raiz: LLM montando JSON) |
| "toda spec tem que ter ao menos a wave 1" | Coberto | Wave 1 (`plan.waves.is_empty()` vira erro reportável) |
| "saber o motivo do bug" | Coberto | Contexto desta spec |
| "o doctor poderia refinar esses ajustes quando necessário" | Coberto | Wave 3 (novo check `wave-integrity`) |
| "onde esse doctor é acionado" | Documentado | Contexto + Não-Objetivos (continua reativo via `/maint doctor`) |
| "não mexer na spec que está rodando" (flatten-spec) | Coberto | Limits — flatten-spec fora dos boundaries |
| "não teremos mais a pasta active" (layout flat pós flatten-spec) | Coberto | Esta spec mora em `spec/{name}/` direto + Wave 3 torna doctor flat-aware (`collect_active_spec_names` → `collect_spec_names`) + AC-7 testa layout flat |
| "dashboard poderia sugerir quando fosse necessário rodar o doctor" | Coberto | Wave 4 (flag `--json` no doctor + Tauri command + badge na Sidebar com cor por severidade + tooltip com hint dos comandos de fix) |
