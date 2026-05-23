# Tactical Fix: `active-pipelines` view inclui specs sem evento via header fallback

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: light
### Checkpoint: 2026-05-23T13:30:00Z
### Lang: pt
### Parent: [[2026-05-21-flatten-spec-layout-and-multi-collab]]

## Contexto

O parent `2026-05-21-flatten-spec-layout-and-multi-collab` definiu a arquitetura multi-colaborador: SQLite local (telemetria privada) vs `spec.md` header (canon cross-dev em git). Mecanismo concreto vive em `packages/core/src/projection/card.rs::project_spec_view_with_header` — quando o event stream local **não tem evento** pra uma spec presente em disco, a função parseia `### Stage:`/`### Outcome:` do header e seeda o `SpecView`, com synthetic-emit gravando um `pipeline.status` no SQLite local pra hidratá-lo.

A tactical-fix anterior [[2026-05-23-resume-active-pipelines-view]] adicionou um novo `--view active-pipelines` ao `mustard-rt run event-projections`, lido pelo `/mustard:resume` como primeira ação. Problema: o `build_active_pipelines` em `apps/rt/src/run/event_projections.rs` consome **só** `events: &[HarnessEvent]` — não chama `project_spec_view_with_header` nem faz Glob de disco. Consequência:

- Colaborador faz `git pull` → recebe `spec.md` nova de outro dev
- Dashboard ainda não foi aberto, nenhuma projeção foi executada
- Colaborador roda `/mustard:resume` → Step 1 chama `event-projections --view active-pipelines`
- View retorna lista **sem aquela spec** (não tem evento local)
- Resume cai no Glob fallback (correto, mas era pra ser o caso raro)
- E pior: o SQLite local **continua sem evento** — a próxima chamada repete a falha

A fix delega ao reader canônico do core: para cada spec presente em disco que falta no event stream, parsear o header e (a) incluir no resultado do view, (b) deixar o synthetic-emit gravar o evento. A partir daí, comportamento esperado em time consolidado: pull → resume → SQLite hidrata → próxima leitura tem evento real.

## Decisão de design

- **Quem decide se a spec é "ativa"** continua sendo o último `pipeline.stage`/`pipeline.phase` (events) OU, na ausência, o par `### Stage:` ≠ `Close` + `### Outcome:` ≠ `Completed` do header. Mesma regra, duas fontes — header fallback é só pra hidratar quando o stream está vazio pra aquela spec.
- **Onde adicionar**: dentro de `build_active_pipelines` em `event_projections.rs`. Não vamos duplicar parsing — chama o reader/projeção do core (que já tem o `view_from_header` privado + synthetic-emit).
- **Glob escopo**: `.claude/spec/*/spec.md` + `.claude/spec/*/wave-plan.md`. Mesma dupla do `/mustard:resume` Step 1.
- **Synthetic emit**: re-aproveitado de `project_spec_view_with_header`. Não precisa criar caminho novo. Side-effect intencional — o user perguntou explicitamente "alimenta o SQL antes de mostrar o resume correto?" e a resposta é sim.
- **Política de inclusão**: spec em disco com header parseável e `Stage: ≠ Close` entra. Sem header parseável OU `Stage: Close` fica de fora. Mesmo critério do event stream — simetria.

## Critérios de Aceitação

- [x] AC-1: build verde — Command: `cargo build -p mustard-rt -p mustard-core`
- [x] AC-2: event_projections.rs delega ao header fallback do core (não duplica parsing) — Command: `bash -c "grep -qE 'mustard_core::projection|view_from_header|project_spec_view_with_header|spec_md_path' apps/rt/src/run/event_projections.rs && echo ok"`
- [x] AC-3: nenhum item do view tem stage Close — Command: `node -e "const {execSync}=require('child_process');const j=JSON.parse(execSync('mustard-rt run event-projections --view active-pipelines',{encoding:'utf8'}));const bad=j.pipelines.filter(p=>p.stage && p.stage.toLowerCase()==='close');if(bad.length>0){console.error('found Close:',bad.map(p=>p.spec));process.exit(1)}console.log('ok')"`
- [x] AC-4: lastEventAt desc preservado em todo o array — Command: `node -e "const {execSync}=require('child_process');const j=JSON.parse(execSync('mustard-rt run event-projections --view active-pipelines',{encoding:'utf8'}));for(let i=1;i<j.pipelines.length;i++){if(j.pipelines[i-1].lastEventAt<j.pipelines[i].lastEventAt){console.error('order break at '+i);process.exit(1)}}console.log('ok')"`
- [x] AC-5: todo item tem campos spec+lastEventAt+stage truthy — Command: `node -e "const {execSync}=require('child_process');const j=JSON.parse(execSync('mustard-rt run event-projections --view active-pipelines',{encoding:'utf8'}));if(j.pipelines.length===0){console.log('ok-empty');process.exit(0)}for(const p of j.pipelines){if(!p.spec||!p.lastEventAt||!p.stage){console.error('bad row',p);process.exit(1)}}console.log('ok n='+j.pipelines.length)"`

## Arquivos

- `apps/rt/src/run/event_projections.rs` — `build_active_pipelines` ganha segundo passo: após processar events, Glob `.claude/spec/*/{spec.md,wave-plan.md}`. Para cada spec presente em disco mas ausente do `per_spec`, chama `mustard_core::projection::card::project_spec_view_with_header` (ou equivalente público) passando `spec_md_path` + sink de eventos. Mescla resultado no `per_spec` map antes do filtro/sort.
- (Opcional, somente se necessário) `packages/core/src/projection/card.rs` — expor `view_from_header` ou um wrapper público se a função interna não estiver acessível ao rt. Sem mudar contrato — só visibilidade.

## Limites

- NÃO mudar o contrato de saída do view (`{ spec, lastEventAt, stage }` continua igual).
- NÃO duplicar parsing de header em rt — sempre delega ao core.
- NÃO derivar de `### Status:` legado nesta sub-spec (varredura anterior já normalizou; legacy parsing já vive no core).
- NÃO Glob em outros lugares além de `.claude/spec/*/` (top-level dirs).

## Cobertura de Críticas

| Crítica/pergunta do usuário | Bucket | Onde |
|---|---|---|
| "alimenta o SQL antes de mostrar o resume correto?" | Coberto | Decisão de design + AC-3 (synthetic-emit re-aproveitado) |
| "view tem que casar com arquitetura multi-collab" | Coberto | Decisão de design + AC-2 |
| Gap surfaced no commit anterior | Coberto | Contexto |
