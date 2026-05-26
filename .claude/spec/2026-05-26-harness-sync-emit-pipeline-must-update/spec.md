# Tactical Fix: meta-sync em pipeline.wave.complete

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: light
### Lang: pt-BR
### Checkpoint: 2026-05-26T14:42:00.000Z
### Parent: 2026-05-26-claude-paths-single-source

## Contexto

Descoberto durante a CLOSE phase de [[2026-05-26-claude-paths-single-source]]: o dashboard e o picker `/mustard:spec` lêem `meta.json` para mostrar status de spec e progresso das waves. Hoje o harness só sincroniza `meta.json` em transições `pipeline.status` (`draft→approved`, `approved→closed`), e mesmo nesses casos só toca o `meta.json` da spec-pai — nunca os das waves filhas.

Evento `pipeline.wave.complete` é emitido por contrato (`resume-flow.md § Step 12c`) toda vez que uma wave fecha, mas o handler em `emit_pipeline.rs:229` **não escuta** esse kind. Resultado: rodar uma pipeline inteira deixa `wave-N-*/meta.json` em `Plan/Active/PLAN` e a parent meta com `currentWave: 1`, `completedWaves: []`, `phase: PLAN` — sem refletir nenhum progresso.

Esta sub-spec corrige só o sync — não muda contrato de evento, nome de campo, ou modelo.

## Critérios de Aceitação

- [ ] **AC-TF1.** Após `emit-pipeline --kind pipeline.wave.complete --spec X --payload "{\"wave\":N}"`, o arquivo `.claude/spec/X/wave-N-*/meta.json` tem `stage: "Close"`, `outcome: "Completed"`, `phase: "CLOSE"`. Command: `rtk node -e "const{execSync}=require('child_process');const path=require('path');const fs=require('fs');const tmp=fs.mkdtempSync(path.join(require('os').tmpdir(),'mt'));fs.mkdirSync(path.join(tmp,'.claude','spec','foo','wave-1-rt'),{recursive:true});fs.writeFileSync(path.join(tmp,'.claude','spec','foo','wave-1-rt','meta.json'),'{\"stage\":\"Plan\",\"outcome\":\"Active\",\"phase\":\"PLAN\"}');fs.writeFileSync(path.join(tmp,'.claude','spec','foo','wave-1-rt','spec.md'),'# x\\n');fs.writeFileSync(path.join(tmp,'mustard.json'),'{}');process.chdir(tmp);execSync('mustard-rt run emit-pipeline --kind pipeline.wave.complete --spec foo --payload \\\"{\\\\\\\"wave\\\\\\\":1}\\\"');const m=JSON.parse(fs.readFileSync(path.join(tmp,'.claude','spec','foo','wave-1-rt','meta.json'),'utf8'));if(m.stage!=='Close'||m.outcome!=='Completed'||m.phase!=='CLOSE')process.exit(1);"`
- [ ] **AC-TF2.** Após o mesmo evento, parent `meta.json` tem `currentWave: N`, `N ∈ completedWaves`, `phase: "EXECUTE"` (ou `"CLOSE"` se for última wave). Command: AC integrado no AC-TF1 com asserts adicionais sobre parent meta.
- [ ] **AC-TF3.** `cargo test -p mustard-rt emit_pipeline -- wave_complete` passa (testes novos cobrindo as duas funções).

## Arquivos

- `apps/rt/src/run/emit_pipeline.rs` — adicionar:
  - `fn sync_wave_meta_sidecar(cwd, spec, wave, ts)` — abre `.claude/spec/{spec}/wave-{wave}-*/meta.json` (glob para casar role), seta `stage=Close, outcome=Completed, phase=CLOSE, checkpoint=ts`.
  - `fn bump_parent_progress(cwd, spec, wave, ts, total_waves)` — abre `.claude/spec/{spec}/meta.json`, lê `raw.completedWaves` (array), insere `wave` se ausente, seta `raw.currentWave=wave`, define `phase` (`EXECUTE` se `wave < totalWaves` ou `totalWaves` ausente; `CLOSE` se igual ou maior).
  - Branch novo no dispatch: `if kind_str == EVENT_PIPELINE_WAVE_COMPLETE { let wave = payload.get("wave").and_then(Value::as_u64); if let Some(w) = wave { sync_wave_meta_sidecar(...); bump_parent_progress(...); } }`.
- `apps/rt/src/run/emit_pipeline.rs` (tests module) — 2 testes novos: `wave_complete_updates_wave_meta`, `wave_complete_bumps_parent_progress`.

## Limites

IN: `apps/rt/src/run/emit_pipeline.rs` (1 arquivo).
OUT: meta model em `packages/core/src/meta.rs` (já tem `raw` flatten — suficiente), formato de evento, contrato `resume-flow.md`.

## Dependências

Stand-alone. Parent já está Closed. Roda sozinha em <10 min de implementação + reinstall.
