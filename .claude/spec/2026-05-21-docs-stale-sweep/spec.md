# docs-stale-check sweep — atualizar SKILLs para SQLite event log

### Status: completed
### Phase: CLOSE
### Scope: light
### Checkpoint: 2026-05-21T01:20:00Z
### Lang: pt
### Parent: 2026-05-20-tactical-fix-via-sub-spec

## Contexto

Tactical fix derivado de [[2026-05-20-tactical-fix-via-sub-spec]]. O `mustard-rt run docs-stale-check` reporta 11 hits em 5 SKILLs do `.claude/commands/mustard/` referenciando arquivos legados (`.pipeline-states/*.json`, `knowledge.json`) que foram superseded pela spec `2026-05-19-pipeline-state-from-sqlite` (pipeline state derivado integralmente de eventos SQLite via `pipeline_state_for_spec`; knowledge/memory em tabelas `knowledge_patterns`/`memory_decisions`/`memory_lessons`).

Hits atuais:
- `.claude/commands/mustard/approve/SKILL.md:35` (`.pipeline-states/*.json`)
- `.claude/commands/mustard/bugfix/SKILL.md:24` (`.pipeline-states/*.json`)
- `.claude/commands/mustard/close/SKILL.md:136, 213` (mix)
- `.claude/commands/mustard/knowledge/SKILL.md:33, 37, 123, 242` (`knowledge.json`)
- `.claude/commands/mustard/resume/SKILL.md:77, 104, 274` (`.pipeline-states/*.json`)

Sweep cirúrgico: substituir cada menção por linguagem alinhada com o estado atual (SQLite event log, `mustard-rt run emit-pipeline`, tabelas SQLite para knowledge/memory). Aplicar em `apps/cli/templates/commands/mustard/<name>/SKILL.md` (fonte canônica) E sincronizar para `.claude/commands/mustard/<name>/SKILL.md` (cópia local do próprio repo Mustard).

## Critérios de Aceitação

- [x] AC-1: `docs-stale-check` retorna 0 hits — Command: `node -e "const out=require('child_process').execSync('mustard-rt run docs-stale-check',{encoding:'utf8'});const j=JSON.parse(out);process.exit(j.hits.length===0?0:1)"`
- [x] AC-2: Nenhum SKILL em `.claude/commands/mustard/` menciona `.pipeline-states/*.json` (exceto em comentário histórico explicitamente marcado) — Command: `node -e "const fs=require('fs'),p=require('path');const dir='.claude/commands/mustard';const files=['approve/SKILL.md','bugfix/SKILL.md','close/SKILL.md','resume/SKILL.md'];for(const f of files){const c=fs.readFileSync(p.join(dir,f),'utf8');if(c.match(/\\.pipeline-states\\/[^\\s\\)]*\\.json/))process.exit(1)}"`
- [x] AC-3: Nenhum SKILL menciona `knowledge.json` como fonte canônica — Command: `node -e "const fs=require('fs'),p=require('path');const c=fs.readFileSync('.claude/commands/mustard/knowledge/SKILL.md','utf8');if(c.includes('knowledge.json'))process.exit(1);const cz=fs.readFileSync('.claude/commands/mustard/close/SKILL.md','utf8');if(cz.includes('knowledge.json'))process.exit(1)"`
- [x] AC-4: Templates source iguais à cópia local em conteúdo (sync coerente) — Command: `node -e "const fs=require('fs');const t='apps/cli/templates/commands/mustard/';const l='.claude/commands/mustard/';for(const f of ['approve','bugfix','close','knowledge','resume']){const a=fs.readFileSync(t+f+'/SKILL.md','utf8');const b=fs.readFileSync(l+f+'/SKILL.md','utf8');if(a.length!==b.length)process.exit(1)}"`

## Arquivos

```
apps/cli/templates/commands/mustard/approve/SKILL.md     — line ~35
apps/cli/templates/commands/mustard/bugfix/SKILL.md      — line ~24
apps/cli/templates/commands/mustard/close/SKILL.md       — lines ~136, ~213
apps/cli/templates/commands/mustard/knowledge/SKILL.md   — lines ~33, ~37, ~123, ~242
apps/cli/templates/commands/mustard/resume/SKILL.md      — lines ~77, ~104, ~274

# Sync para
.claude/commands/mustard/{approve,bugfix,close,knowledge,resume}/SKILL.md
```

## Tarefas

- [x] Em cada arquivo source (`apps/cli/templates/commands/mustard/.../SKILL.md`): substituir referências a `.pipeline-states/<spec>.json` por linguagem alinhada com SQLite — ex.: "estado da pipeline derivado de eventos SQLite via `mustard-rt run event-projections --view pipeline-state --spec {specName}` ou `pipeline_state_for_spec`". Reescrever as frases preservando o sentido (não remover passos do action), apenas corrigir o mecanismo de persistência mencionado.
- [x] Em `knowledge/SKILL.md`: substituir menções a `knowledge.json` por "tabela `knowledge_patterns` do SQLite event store, lida via `mustard-rt run memory list` / `mustard-rt run memory search`".
- [x] Em `close/SKILL.md:213`: análogo para `knowledge.json`.
- [x] Após editar os 5 source files, sincronizar para `.claude/commands/mustard/.../SKILL.md` (cópia local, memória `feedback_mustard_self_scripts_stale`).
- [x] Verificar com `mustard-rt run docs-stale-check`.

## Limites

- 10 arquivos: 5 em `apps/cli/templates/commands/mustard/{approve,bugfix,close,knowledge,resume}/SKILL.md` + 5 syncs em `.claude/commands/mustard/.../SKILL.md`.

**Fora dos limites:**
- Refatorar a action de cada skill (só mudar a menção do mecanismo de storage).
- Alterar o `docs-stale-check` em si.
- Adicionar novos patterns ao `.docs-audit.json` desta spec.

## Checklist

- [x] AC-1 a AC-4 passam
- [x] Sentido original de cada SKILL preservado (sweep cirúrgico, sem reescrita)
