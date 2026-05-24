# Wave 5 — Migration one-shot do repo Mustard

## Resumo

Migração única do repositório Mustard atual para o novo layout. Move tudo de `spec/active/*`, `spec/completed/*` e `spec/superseded/*` para `spec/{name}/`. Faz backfill de eventos para specs cujo SQLite está em desacordo com o header (caso típico do colaborador). Garante o header sync das specs que sofreram emit manual nesta sessão.

## Contexto

Hoje o repo tem 3 pastas em `spec/active/`, 85 em `spec/completed/` e 1 em `spec/superseded/`. Com o código novo o lifecycle não depende mais da pasta, mas o filesystem precisa ser aplainado uma vez para casar com o novo modelo. Em paralelo o SQLite tem 5 fantasmas (status ativo no SQLite, pasta em `completed/`) e 1 preso (pasta em `active/`, status `completed` no SQLite) — esses casos precisam ser conciliados emitindo o evento que falta ou ajustando o header. Decisão (`feedback_no_migration_dev_phase`): sem rollback, sem compat — só converte.

## Arquivos

```
(scripted migration; sem novo source code por wave)
.claude/spec/active/*       — mover dirs
.claude/spec/completed/*    — mover dirs
.claude/spec/superseded/*   — mover dirs
.claude/.harness/mustard.db — backfill eventos sintéticos
```

## Tarefas

- [x] Script de migração via `node -e` (one-shot, idempotente):
  1. Mover cada subdir de `.claude/spec/active/`, `.claude/spec/completed/`, `.claude/spec/superseded/` para `.claude/spec/{slug}/`.
  2. Após o move, apagar as 3 pastas vazias (`active/`, `completed/`, `superseded/`).
- [x] Backfill SQLite via `mustard-rt run rebuild-specs` + verificações por Node:
  1. Para cada spec em `spec/{slug}/`, ler header. Se `### Status:` ∈ {completed, cancelled, abandoned} e SQLite não tem `pipeline.status: <esse>` registrado, emitir o evento sintético.
  2. Para as 5 specs identificadas como fantasma nesta sessão: emitir o evento que falta (status → completed se o header diz completed).
- [x] Para a spec presa (`2026-05-20-economia-moat-unification`): nada precisa mover (já está em flat `spec/`), só confirmar header está alinhado com `pipeline.status: completed`.
- [x] Rodar `mustard-rt run rebuild-specs` no final e validar via Node que zero fantasma + zero preso.

## Acceptance Criteria

- [x] AC-W5-1: As 3 pastas-bucket não existem mais — Command: `node -e "const f=require('fs');const bad=['active','completed','superseded'].filter(b=>f.existsSync('.claude/spec/'+b));process.exit(bad.length===0?0:(console.error('still exist:',bad),1))"`
- [x] AC-W5-2: Todas as specs do repo (89) estão em `.claude/spec/{name}/spec.md` — Command: `node -e "const f=require('fs');const dirs=f.readdirSync('.claude/spec').filter(d=>f.statSync('.claude/spec/'+d).isDirectory());const bad=dirs.filter(d=>!f.existsSync('.claude/spec/'+d+'/spec.md'));process.exit(bad.length===0&&dirs.length>=85?0:(console.error('bad:',bad,'count:',dirs.length),1))"`
- [x] AC-W5-3: Zero fantasmas (specs ativas no SQLite sem pasta) — Command: `node -e "const {DatabaseSync}=require('node:sqlite');const fs=require('fs');const db=new DatabaseSync('.claude/.harness/mustard.db');const onDisk=new Set(fs.readdirSync('.claude/spec').filter(d=>fs.statSync('.claude/spec/'+d).isDirectory()&&fs.existsSync('.claude/spec/'+d+'/spec.md')));const active=db.prepare(\"SELECT name FROM specs WHERE status IN ('planning','implementing','reviewing','qa','blocked','wave-failed','closed-followup')\").all().map(r=>r.name);const ghosts=active.filter(n=>!onDisk.has(n));process.exit(ghosts.length===0?0:(console.error('ghosts:',ghosts),1))"`

## Limites

- `.claude/spec/` (toda a árvore — operação one-shot sobre o repo Mustard)
- `.claude/.harness/mustard.db` (write somente via emit-pipeline / rebuild-specs)

## Network

- Parent: [[2026-05-21-flatten-spec-layout-and-multi-collab]]
- Depende de: [[wave-2-general]], [[wave-3-general]], [[wave-4-general]]
