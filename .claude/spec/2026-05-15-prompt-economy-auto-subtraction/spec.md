# Feature: Medição Automática de Prompt Economy (Subtraction)

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full
### Checkpoint: 2026-05-15T17:45:00Z
### QA: pass (8/8 AC) | Review: APPROVED (mustard + dashboard, 0 CRITICAL)
### Lang: pt
### Repos: C:/Atiz/mustard + C:/Atiz/mustard-dashboard

## Contexto

O Mustard fatia o spec por wave: cada sub-agente despachado na fase EXECUTE recebe só
a seção da sua wave, não o spec inteiro. Essa diferença — o que o agente não precisou
receber — é uma economia real de contexto, e o dashboard tem um card para prová-la.
Hoje esse card fica permanentemente zerado: o evento `mustard.subtraction.applied` só
é gravado quando o orquestrador roda à mão um comando `bun emit-subtraction.js`,
conforme uma instrução marcada "silent, optional" nos SKILLs. Como isso depende da
disciplina de um LLM em executar um passo opcional, na prática o evento quase nunca é
emitido — o `mustard.db` de projetos reais tem zero subtractions. A medição existe e
está correta; o disparo é que é frágil. A correção é mover a emissão para o hook que
já dispara em todo despacho de Task e que já tem spec, fase, wave e o tamanho do
prompt em mãos.

## Summary

`subagent-tracker.js` (PreToolUse(Task)) passa a emitir `mustard.subtraction.applied`
do tipo `wave-slice` automaticamente quando a fase é EXECUTE, usando
`spec-extract.measure()`. As chamadas manuais a `emit-subtraction.js` saem dos SKILLs;
o script é deletado. O dashboard reflete o dado real num card "Contexto enviado vs.
evitado".

## Contrato do evento (Wave 1 -> Wave 2)

`mustard.subtraction.applied`, emitido pelo hook em PreToolUse(Task) quando a fase
corrente é EXECUTE:

```
payload: { type:"wave-slice", bytes_omitted:N, full_bytes:N, slice_bytes:N,
           prompt_bytes:N, wave:N, measured:true }
ctx:     { spec, wave, actor:{ kind:"hook", id:"subagent-tracker" } }
```

- `bytes_omitted` = `full_bytes - slice_bytes` (de `spec-extract.measure()`) — o resto
  do spec que o agente não recebeu. Mantém o nome `bytes_omitted` porque a query do
  dashboard já lê `$.bytes_omitted`.
- `prompt_bytes` = tamanho real do prompt do Task (já capturado como `prefix_bytes`).
- Um evento por despacho de Task em EXECUTE.

## Boundaries

Repo `C:/Atiz/mustard`:
- `templates/hooks/subagent-tracker.js` — emitir wave-slice automaticamente
- `templates/scripts/emit-subtraction.js` — DELETE
- `templates/commands/mustard/feature/SKILL.md` — remover chamada emit-subtraction
- `templates/commands/mustard/bugfix/SKILL.md` — idem
- `templates/commands/mustard/resume/SKILL.md` — remover chamadas + seção explicativa
- `templates/commands/mustard/review/SKILL.md` — idem
- `templates/hooks/__tests__/` — cobertura do novo emit (arquivo de teste existente)

Repo `C:/Atiz/mustard-dashboard`:
- `src-tauri/src/telemetry.rs` — query + struct SubtractionsBlock
- `src/api/promptEconomy.ts` — shape TypeScript
- `src/pages/Telemetry.tsx` — card "Contexto enviado vs. evitado"
- `src/hooks/usePromptEconomy.ts` — só se re-exporta o tipo alterado

Fora de escopo: `diff-vs-full` / `review-diff-first` / `analyze-diff-skip`. Não toca
model routing, `entity-registry.json`, nem adiciona dependência npm/cargo.

## Files (~11)

| Arquivo | Operação | Wave |
|---|---|---|
| `templates/hooks/subagent-tracker.js` | Edit | 1 |
| `templates/scripts/emit-subtraction.js` | Delete | 1 |
| `templates/commands/mustard/feature/SKILL.md` | Edit | 1 |
| `templates/commands/mustard/bugfix/SKILL.md` | Edit | 1 |
| `templates/commands/mustard/resume/SKILL.md` | Edit | 1 |
| `templates/commands/mustard/review/SKILL.md` | Edit | 1 |
| `templates/hooks/__tests__/{subagent-tracker test}` | Edit | 1 |
| `mustard-dashboard/src-tauri/src/telemetry.rs` | Edit | 2 |
| `mustard-dashboard/src/api/promptEconomy.ts` | Edit | 2 |
| `mustard-dashboard/src/pages/Telemetry.tsx` | Edit | 2 |
| `mustard-dashboard/src/hooks/usePromptEconomy.ts` | Edit (cond.) | 2 |

## Tasks

### Hooks Agent (Wave 1)

- [x] `subagent-tracker.js` — em `handlePreToolUse`, após calcular `prefix_bytes`:
      detectar fase EXECUTE (`ps.phase === 'EXECUTE'` ou `ps.phase === 3`) e
      `ps.wave >= 1`; resolver o caminho do spec
      (`.claude/spec/active/{ps.spec}/spec.md`; layout wave-plan: best-effort,
      fail-open se não resolver); chamar `measure()` de `spec-extract.js`; emitir
      `mustard.subtraction.applied` com o payload do contrato. Fail-open total:
      qualquer erro (sem state, sem spec, measure null) = nenhum emit, hook segue.
- [x] Emitir um evento por despacho de Task (sem idempotência por wave — ver Decisões)
- [x] Deletar `templates/scripts/emit-subtraction.js`
- [x] `feature/SKILL.md`, `bugfix/SKILL.md`, `review/SKILL.md` — remover a linha
      `bun .claude/scripts/emit-subtraction.js ...`
- [x] `resume/SKILL.md` — remover as chamadas a `emit-subtraction.js` e substituir a
      seção que explica os tipos de subtração por nota curta: a subtração wave-slice
      agora é automática via `subagent-tracker.js`, sem ação do orquestrador
- [x] `templates/hooks/__tests__/` — adicionar teste: payload PreToolUse(Task) +
      pipeline-state em EXECUTE wave 1 + spec com seção de wave → `events.jsonl`
      recebe `mustard.subtraction.applied`
- [x] build/type-check: `bun test templates/hooks/__tests__/hooks.test.js`

### Dashboard Agent (Wave 2)

- [x] `telemetry.rs` — query: filtrar `type='wave-slice'`, `GROUP BY` wave; somar
      `bytes_omitted` e `prompt_bytes`; manter o delta de sessão (`session_since`);
      remover os 3 tipos mortos do struct `SubtractionsBlock`; expor
      `context_sent_bytes` (Σ prompt_bytes), `context_avoided_bytes` (Σ
      bytes_omitted) e breakdown por wave
- [x] `promptEconomy.ts` — atualizar o shape: remover os 3 tipos mortos; campos
      enviado / evitado / por-wave
- [x] `Telemetry.tsx` — renomear o card "Bytes omitidos pelo Mustard" para "Contexto
      enviado vs. evitado"; mostrar contexto enviado (Σ prompt_bytes) vs. evitado
      (Σ bytes_omitted), breakdown por wave, acumulado + delta de sessão; atualizar
      o empty-state (sai "raros por design / só em /resume"; entra "aparece quando
      uma pipeline roda a fase EXECUTE")
- [x] `usePromptEconomy.ts` — ajustar só se re-exporta o tipo alterado
- [x] build: `pnpm build` + `cargo test`

## Acceptance Criteria

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: o hook emite `mustard.subtraction.applied` num Task em EXECUTE — Command: `node -e "const fs=require('fs'),os=require('os'),path=require('path'),cp=require('child_process');const T=fs.mkdtempSync(path.join(os.tmpdir(),'ac1-'));fs.mkdirSync(path.join(T,'.claude','.pipeline-states'),{recursive:true});fs.mkdirSync(path.join(T,'.claude','spec','active','s'),{recursive:true});fs.writeFileSync(path.join(T,'.claude','.pipeline-states','s.json'),JSON.stringify({specName:'s',phaseName:'EXECUTE',wave:1}));fs.writeFileSync(path.join(T,'.claude','spec','active','s','spec.md'),'## Summary\nomitted prose padding padding padding\n\n### Backend Agent (Wave 1)\nalpha\n\n### Backend Agent (Wave 2)\nbeta\n');const r=cp.spawnSync(process.execPath,['C:/Atiz/mustard/templates/hooks/subagent-tracker.js'],{input:JSON.stringify({hook_event_name:'PreToolUse',tool_name:'Task',cwd:T,tool_input:{description:'d',subagent_type:'general-purpose',prompt:'hello'}}),encoding:'utf8',env:Object.assign({},process.env,{CLAUDE_PROJECT_DIR:T})});const ev=path.join(T,'.claude','.harness','events.jsonl');const ok=fs.existsSync(ev)&&fs.readFileSync(ev,'utf8').includes('mustard.subtraction.applied');fs.rmSync(T,{recursive:true,force:true});process.exit(ok?0:1)"`
- [x] AC-2: o hook NÃO emite quando a fase não é EXECUTE — Command: `node -e "const fs=require('fs'),os=require('os'),path=require('path'),cp=require('child_process');const T=fs.mkdtempSync(path.join(os.tmpdir(),'ac2-'));fs.mkdirSync(path.join(T,'.claude','.pipeline-states'),{recursive:true});fs.mkdirSync(path.join(T,'.claude','spec','active','s'),{recursive:true});fs.writeFileSync(path.join(T,'.claude','.pipeline-states','s.json'),JSON.stringify({specName:'s',phaseName:'PLAN',wave:1}));fs.writeFileSync(path.join(T,'.claude','spec','active','s','spec.md'),'### Backend Agent (Wave 1)\nalpha\n');const r=cp.spawnSync(process.execPath,['C:/Atiz/mustard/templates/hooks/subagent-tracker.js'],{input:JSON.stringify({hook_event_name:'PreToolUse',tool_name:'Task',cwd:T,tool_input:{description:'d',subagent_type:'general-purpose',prompt:'hello'}}),encoding:'utf8',env:Object.assign({},process.env,{CLAUDE_PROJECT_DIR:T})});const ev=path.join(T,'.claude','.harness','events.jsonl');const emitted=fs.existsSync(ev)&&fs.readFileSync(ev,'utf8').includes('mustard.subtraction.applied');fs.rmSync(T,{recursive:true,force:true});process.exit(emitted?1:0)"`
- [x] AC-3: `emit-subtraction.js` foi deletado — Command: `node -e "process.exit(require('fs').existsSync('C:/Atiz/mustard/templates/scripts/emit-subtraction.js')?1:0)"`
- [x] AC-4: nenhum SKILL chama mais emit-subtraction — Command: `node -e "const fs=require('fs');const f=['feature','bugfix','resume','review'].map(n=>'C:/Atiz/mustard/templates/commands/mustard/'+n+'/SKILL.md');process.exit(f.some(p=>fs.readFileSync(p,'utf8').includes('emit-subtraction'))?1:0)"`
- [x] AC-5: testes de hook do Mustard passam — Command: `bash -c 'cd /c/Atiz/mustard && bun test templates/hooks/__tests__/hooks.test.js'`
- [x] AC-6: dashboard buildou sem erro — Command: `bash -c 'cd /c/Atiz/mustard-dashboard && pnpm build'`
- [x] AC-7: cargo test do dashboard passa — Command: `bash -c 'cd /c/Atiz/mustard-dashboard/src-tauri && cargo test'`
- [x] AC-8: o card foi renomeado e o empty-state atualizado — Command: `node -e "const c=require('fs').readFileSync('C:/Atiz/mustard-dashboard/src/pages/Telemetry.tsx','utf8');process.exit(/enviado/i.test(c)&&/evitado/i.test(c)&&!/raros por design/.test(c)?0:1)"`

## Dependencies

- Wave 1 → Wave 2. O dashboard consome o payload novo; o contrato acima permite a
  Wave 2 ser desenhada em paralelo, mas a validação real (cargo test contra dado de
  fixture) confirma-se depois da Wave 1.
- Sem dependência externa nova.

## Decisões

- **Emissão por despacho de Task, sem idempotência por wave.** O contrafactual honesto
  é "orquestrador ingênuo cola o spec inteiro no prompt de cada Task". N agentes numa
  wave = N omissões reais — cada Task é um contexto de API separado que recebeu a
  fatia em vez do spec inteiro. Idempotência por wave subcontaria a economia.
- **`diff-vs-full` fica fora de escopo.** Medir a economia de passar `git diff` em vez
  dos arquivos full exige o pipeline-state gravar o SHA de fronteira de cada wave para
  o hook rodar `git diff` sozinho. Sem essa infra, o hook não tem como medir — e o
  honesto é não emitir. `wave-slice` automático já tira o card do zero permanente.
- **`emit-subtraction.js` deletado, não mantido como lib.** A lógica de medição vive
  em `spec-extract.js` (`measure()`), que o hook já pode `require`. Manter o script
  seria código morto — ninguém mais o chama.

## Concerns

- `analyze-validation.js` marcou `telemetry.rs`, `promptEconomy.ts`, `Telemetry.tsx` e
  `usePromptEconomy.ts` como "missing-file" (WARN). Falso positivo: o validador resolve
  paths contra `C:/Atiz/mustard`, mas esses arquivos vivem no repo irmão
  `C:/Atiz/mustard-dashboard` (existência e linhas confirmadas na fase ANALYZE).

## Non-Goals

- Não emite `diff-vs-full` / `review-diff-first` / `analyze-diff-skip`.
- Não altera model routing nem o `entity-registry.json`.
- Não adiciona dependência npm/cargo em nenhum dos repos.
- Não toca o coletor OTEL nem o card de USD do dashboard.
