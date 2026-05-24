# /scan performance — pre-compute deterministic work in orchestrate.js

- **Lang**: ptbr
- **Checkpoint**: 2026-05-13T00:00:02Z
- **Scope**: Full
- **Type**: feature
- **Model**: opus
- **Depends on**: completed (Bun migration, HARD CONTRACT, cluster cache wipe + force-prebuild registry)
- **Unlocks**: stack-specific brief enrichment (out of scope here)

## Summary

Transferir trabalho determinístico que hoje cada Task agent faz via Bash/Read/Write para `orchestrate.js`. Reduz tool uses por agente em 40-60% (estimativa: ~30-70 → ~10-20) e corta input tokens por scan em ~30-40k. Não muda contrato do prompt nem a saída final — apenas elimina round-trips de LLM em operações que JS faz instantaneamente.

## Problem

Evidência empírica do screenshot de scan no Sialia (`C:\Atiz\Competi\projetos\sialia`, 5 subprojetos, `--force`):

| Subproject     | Tool uses | Tokens |
|----------------|-----------|--------|
| sialia.Backend |        61 |  101.2k |
| sialia-admin   |        34 |   75.7k |
| sialia-app     |        68 |   88.7k |
| sialia-partners|        54 |   97.3k |
| sialia-core    |        22 |   78.3k |

Total: ~440k tokens, ~239 tool uses combinados. Cada tool use custa 300-800ms de round-trip (encode → LLM → execute → decode). Mesmo paralelizado, o agente mais lento (sialia-app) consome ~68 × 600ms = 41s só de round-trip.

Inspeção do `agent-prompt.template.md` revela que cada agente faz, deterministicamente:
- **Step 2 (Backup)**: move N arquivos `<!-- mustard:generated -->` de `commands/` para `commands/_backup/`. 5-10 ops de Bash `mv`/`mkdir` por agente.
- **Force-mode cleanup** (descrito no `forceBlock`): percorre `skills/` e deleta subdirs com `<!-- mustard:generated`. 3-8 ops.
- **Step 3 (notes.md)**: cria skeleton se ausente. 1-2 ops.
- **Step 4.a-c (Stack/Tooling/Structure discovery)**: lê `package.json`/`csproj`/`pyproject.toml` para extrair comandos build/test/lint, faz Glob de top-level dirs. 3-7 ops.

Soma: 12-27 tool uses por agente são **trabalho mecânico** que JS resolve em <50ms cada.

Não tem como "tornar LLM mais rápido". Tem que diminuir o que ela faz.

## Goal

Após este spec:

1. `orchestrate.js` faz backup + force-cleanup + notes.md skeleton + extração tooling/structure **antes** do dispatch.
2. `agent-prompt.template.md` remove os passos correspondentes — agente recebe brief já com tooling/structure pré-computados.
3. Tool uses median por agente cai para 10-20.
4. Input tokens por scan cai em 30-40k (medido via OpenTelemetry spans pós-scan).

## Acceptance Criteria

Todas com comando cross-shell (Windows `cmd.exe` via `execSync` + Unix bash). Cada AC roda sem state externo (cria temp dir, executa, valida, limpa).

1. **orchestrate.js faz backup de mustard:generated MDs antes do dispatch**
   ```bash
   bun -e "const fs=require('fs');const os=require('os');const path=require('path');const {execSync}=require('child_process');const repo=process.cwd();const tmp=fs.mkdtempSync(path.join(os.tmpdir(),'scan-perf-1-'));const sub=path.join(tmp,'sub-a');fs.mkdirSync(path.join(sub,'.claude','commands'),{recursive:true});fs.writeFileSync(path.join(sub,'.claude','commands','stack.md'),'<!-- mustard:generated -->\nold');fs.writeFileSync(path.join(sub,'CLAUDE.md'),'# sub-a');fs.writeFileSync(path.join(tmp,'CLAUDE.md'),'# root\n## Project Structure\n| sub-a | - |');process.chdir(tmp);try{execSync('bun '+path.join(repo,'templates','scripts','scan','orchestrate.js')+' --force',{stdio:'pipe'});}catch(e){}const moved=fs.existsSync(path.join(sub,'.claude','commands','_backup','stack.md'));const empty=!fs.existsSync(path.join(sub,'.claude','commands','stack.md'));process.exit(moved&&empty?0:1)"
   ```

2. **orchestrate.js deleta skills mustard:generated em --force**
   ```bash
   bun -e "const fs=require('fs');const os=require('os');const path=require('path');const {execSync}=require('child_process');const repo=process.cwd();const tmp=fs.mkdtempSync(path.join(os.tmpdir(),'scan-perf-2-'));const sub=path.join(tmp,'sub-a');const sd=path.join(sub,'.claude','skills','old-pattern');fs.mkdirSync(sd,{recursive:true});fs.writeFileSync(path.join(sd,'SKILL.md'),'<!-- mustard:generated -->\nold');fs.writeFileSync(path.join(sub,'CLAUDE.md'),'# sub-a');fs.writeFileSync(path.join(tmp,'CLAUDE.md'),'# root\n## Project Structure\n| sub-a | - |');process.chdir(tmp);try{execSync('bun '+path.join(repo,'templates','scripts','scan','orchestrate.js')+' --force',{stdio:'pipe'});}catch(e){}process.exit(!fs.existsSync(sd)?0:1)"
   ```

3. **orchestrate.js cria notes.md se ausente**
   ```bash
   bun -e "const fs=require('fs');const os=require('os');const path=require('path');const {execSync}=require('child_process');const repo=process.cwd();const tmp=fs.mkdtempSync(path.join(os.tmpdir(),'scan-perf-3-'));const sub=path.join(tmp,'sub-a');fs.mkdirSync(path.join(sub,'.claude','commands'),{recursive:true});fs.writeFileSync(path.join(sub,'CLAUDE.md'),'# sub-a');fs.writeFileSync(path.join(tmp,'CLAUDE.md'),'# root\n## Project Structure\n| sub-a | - |');process.chdir(tmp);try{execSync('bun '+path.join(repo,'templates','scripts','scan','orchestrate.js'),{stdio:'pipe'});}catch(e){}const notes=path.join(sub,'.claude','commands','notes.md');process.exit(fs.existsSync(notes)&&fs.readFileSync(notes,'utf8').includes('## Mandatory Patterns')?0:1)"
   ```

4. **agentPrompt contém bloco `## Tooling detected` com comandos extraídos**
   ```bash
   bun -e "const fs=require('fs');const os=require('os');const path=require('path');const {execSync}=require('child_process');const repo=process.cwd();const tmp=fs.mkdtempSync(path.join(os.tmpdir(),'scan-perf-4-'));const sub=path.join(tmp,'api');fs.mkdirSync(sub,{recursive:true});fs.writeFileSync(path.join(sub,'package.json'),JSON.stringify({name:'api',scripts:{build:'tsc',test:'vitest',lint:'eslint .'}}));fs.writeFileSync(path.join(sub,'CLAUDE.md'),'# api');fs.writeFileSync(path.join(tmp,'CLAUDE.md'),'# root\n## Project Structure\n| api | - |');process.chdir(tmp);const out=execSync('bun '+path.join(repo,'templates','scripts','scan','orchestrate.js'),{encoding:'utf8'});const j=JSON.parse(out);const p=(j.dispatch[0]||{}).agentPrompt||'';process.exit(/## Tooling detected/.test(p)&&/tsc/.test(p)&&/vitest/.test(p)?0:1)"
   ```

5. **agentPrompt contém bloco `## Project structure` com top-level dirs**
   ```bash
   bun -e "const fs=require('fs');const os=require('os');const path=require('path');const {execSync}=require('child_process');const repo=process.cwd();const tmp=fs.mkdtempSync(path.join(os.tmpdir(),'scan-perf-5-'));const sub=path.join(tmp,'web');fs.mkdirSync(path.join(sub,'src'),{recursive:true});fs.mkdirSync(path.join(sub,'tests'),{recursive:true});fs.writeFileSync(path.join(sub,'package.json'),JSON.stringify({name:'web'}));fs.writeFileSync(path.join(sub,'CLAUDE.md'),'# web');fs.writeFileSync(path.join(tmp,'CLAUDE.md'),'# root\n## Project Structure\n| web | - |');process.chdir(tmp);const out=execSync('bun '+path.join(repo,'templates','scripts','scan','orchestrate.js'),{encoding:'utf8'});const j=JSON.parse(out);const p=(j.dispatch[0]||{}).agentPrompt||'';process.exit(/## Project structure/.test(p)&&/src/.test(p)&&/tests/.test(p)?0:1)"
   ```

6. **agent-prompt.template.md removeu Step 2 (Backup) e Step 3 (notes.md)**
   ```bash
   bun -e "const fs=require('fs');const t=fs.readFileSync('templates/scripts/scan/agent-prompt.template.md','utf8');process.exit(!/\\*\\*Backup\\*\\* — move generated/i.test(t)&&!/Ensure .* notes\\.md exists/i.test(t)?0:1)"
   ```

7. **Regression: orchestrate tests passam (16/16)**
   ```bash
   bun test templates/scripts/__tests__/scan-orchestrate.test.js
   ```

8. **Regression: finalize tests passam (7/7)**
   ```bash
   bun test templates/scripts/__tests__/scan-finalize.test.js
   ```

### Parseable AC (cross-shell QA-runner)

- [ ] AC-1: orchestrate --force moves generated MDs to _backup/ — Command: `bun -e "const fs=require('fs');const os=require('os');const path=require('path');const {execSync}=require('child_process');const repo=process.cwd();const tmp=fs.mkdtempSync(path.join(os.tmpdir(),'scan-perf-1-'));const sub=path.join(tmp,'sub-a');fs.mkdirSync(path.join(sub,'.claude','commands'),{recursive:true});fs.writeFileSync(path.join(sub,'.claude','commands','stack.md'),'<!-- mustard:generated -->\nold');fs.writeFileSync(path.join(sub,'CLAUDE.md'),'# sub-a');fs.writeFileSync(path.join(tmp,'CLAUDE.md'),'# root\n## Project Structure\n| sub-a | - |');process.chdir(tmp);try{execSync('bun '+path.join(repo,'templates','scripts','scan','orchestrate.js')+' --force',{stdio:'pipe'});}catch(e){}const moved=fs.existsSync(path.join(sub,'.claude','commands','_backup','stack.md'));const empty=!fs.existsSync(path.join(sub,'.claude','commands','stack.md'));process.exit(moved&&empty?0:1)"`
- [ ] AC-2: orchestrate --force deletes generated skills — Command: `bun -e "const fs=require('fs');const os=require('os');const path=require('path');const {execSync}=require('child_process');const repo=process.cwd();const tmp=fs.mkdtempSync(path.join(os.tmpdir(),'scan-perf-2-'));const sub=path.join(tmp,'sub-a');const sd=path.join(sub,'.claude','skills','old-pattern');fs.mkdirSync(sd,{recursive:true});fs.writeFileSync(path.join(sd,'SKILL.md'),'<!-- mustard:generated -->\nold');fs.writeFileSync(path.join(sub,'CLAUDE.md'),'# sub-a');fs.writeFileSync(path.join(tmp,'CLAUDE.md'),'# root\n## Project Structure\n| sub-a | - |');process.chdir(tmp);try{execSync('bun '+path.join(repo,'templates','scripts','scan','orchestrate.js')+' --force',{stdio:'pipe'});}catch(e){}process.exit(!fs.existsSync(sd)?0:1)"`
- [ ] AC-3: orchestrate creates notes.md skeleton — Command: `bun -e "const fs=require('fs');const os=require('os');const path=require('path');const {execSync}=require('child_process');const repo=process.cwd();const tmp=fs.mkdtempSync(path.join(os.tmpdir(),'scan-perf-3-'));const sub=path.join(tmp,'sub-a');fs.mkdirSync(path.join(sub,'.claude','commands'),{recursive:true});fs.writeFileSync(path.join(sub,'CLAUDE.md'),'# sub-a');fs.writeFileSync(path.join(tmp,'CLAUDE.md'),'# root\n## Project Structure\n| sub-a | - |');process.chdir(tmp);try{execSync('bun '+path.join(repo,'templates','scripts','scan','orchestrate.js'),{stdio:'pipe'});}catch(e){}const notes=path.join(sub,'.claude','commands','notes.md');process.exit(fs.existsSync(notes)&&fs.readFileSync(notes,'utf8').includes('## Mandatory Patterns')?0:1)"`
- [ ] AC-4: agentPrompt contains Tooling block from package.json — Command: `bun -e "const fs=require('fs');const os=require('os');const path=require('path');const {execSync}=require('child_process');const repo=process.cwd();const tmp=fs.mkdtempSync(path.join(os.tmpdir(),'scan-perf-4-'));const sub=path.join(tmp,'api');fs.mkdirSync(sub,{recursive:true});fs.writeFileSync(path.join(sub,'package.json'),JSON.stringify({name:'api',scripts:{build:'tsc',test:'vitest',lint:'eslint .'}}));fs.writeFileSync(path.join(sub,'CLAUDE.md'),'# api');fs.writeFileSync(path.join(tmp,'CLAUDE.md'),'# root\n## Project Structure\n| api | - |');process.chdir(tmp);const out=execSync('bun '+path.join(repo,'templates','scripts','scan','orchestrate.js'),{encoding:'utf8'});const j=JSON.parse(out);const p=(j.dispatch[0]||{}).agentPrompt||'';process.exit(/## Tooling detected/.test(p)&&/tsc/.test(p)&&/vitest/.test(p)?0:1)"`
- [ ] AC-5: agentPrompt contains Project structure block — Command: `bun -e "const fs=require('fs');const os=require('os');const path=require('path');const {execSync}=require('child_process');const repo=process.cwd();const tmp=fs.mkdtempSync(path.join(os.tmpdir(),'scan-perf-5-'));const sub=path.join(tmp,'web');fs.mkdirSync(path.join(sub,'src'),{recursive:true});fs.mkdirSync(path.join(sub,'tests'),{recursive:true});fs.writeFileSync(path.join(sub,'package.json'),JSON.stringify({name:'web'}));fs.writeFileSync(path.join(sub,'CLAUDE.md'),'# web');fs.writeFileSync(path.join(tmp,'CLAUDE.md'),'# root\n## Project Structure\n| web | - |');process.chdir(tmp);const out=execSync('bun '+path.join(repo,'templates','scripts','scan','orchestrate.js'),{encoding:'utf8'});const j=JSON.parse(out);const p=(j.dispatch[0]||{}).agentPrompt||'';process.exit(/## Project structure/.test(p)&&/src/.test(p)&&/tests/.test(p)?0:1)"`
- [ ] AC-6: template removed Backup + notes.md steps — Command: `bun -e "const fs=require('fs');const t=fs.readFileSync('templates/scripts/scan/agent-prompt.template.md','utf8');process.exit(!/\\*\\*Backup\\*\\* — move generated/i.test(t)&&!/Ensure .* notes\\.md exists/i.test(t)?0:1)"`
- [ ] AC-7: orchestrate tests pass — Command: `bun test templates/scripts/__tests__/scan-orchestrate.test.js`
- [ ] AC-8: finalize tests pass — Command: `bun test templates/scripts/__tests__/scan-finalize.test.js`

## Implementation

### Helper module (new)

`templates/scripts/scan/_precompute.js` — pure functions, no orchestrate state:

```
exports.backupGeneratedMds(absCommandsDir) → { moved: string[], created_backup_dir: bool }
  // Walks absCommandsDir for *.md, reads first 200 bytes, if includes '<!-- mustard:generated'
  // moves to absCommandsDir/_backup/. Creates _backup/ if needed. Idempotent.

exports.purgeGeneratedSkills(absSkillsDir) → { removed: string[] }
  // Walks subdirs in absSkillsDir, reads each SKILL.md first 200 bytes,
  // if includes '<!-- mustard:generated' fs.rm(subdir, {recursive:true}). Idempotent.

exports.ensureNotesMd(absCommandsDir, name, role) → bool (created)
  // Creates notes.md with H1, blockquote, and 3 H2 sections if missing.

exports.buildToolingBlock(subprojectPath, stack) → string (markdown block, or '' if nothing)
  // Reads package.json/csproj/pyproject.toml/etc., returns:
  //   ## Tooling detected
  //   - build: <cmd> (source: package.json scripts.build)
  //   - test:  <cmd>
  //   - lint:  <cmd>
  //   - typecheck: <cmd>
  // Stack-aware: TS/JS reads scripts. .NET reads PackageReference + csproj. Python reads pyproject.toml.
  // Returns '' if nothing detected so {{toolingBlock}} renders empty.

exports.buildStructureBlock(subprojectPath) → string (markdown block, or '')
  // Glob top-level dirs (depth 1), filter DEFAULT_IGNORE. Returns:
  //   ## Project structure
  //   - src/ — N files
  //   - tests/ — N files
  //   - ...
  // Up to 12 entries. '' if <=1 dir.
```

### Changed files

- `templates/scripts/scan/orchestrate.js`
  - New step `precomputePerSubproject(detect)` between `generateAgentFiles` (4.5) and registry refresh (4.6). For each subproject:
    1. If FORCE: `purgeGeneratedSkills(<absSub>/.claude/skills)`
    2. If FORCE: `backupGeneratedMds(<absSub>/.claude/commands)`
    3. `ensureNotesMd(<absSub>/.claude/commands, sub.name, sub.role)`
    4. Compute `toolingBlock` + `structureBlock`, attach to `sub.precomputed = { toolingBlock, structureBlock }`
  - `renderPrompt(template, sub, registry)` reads `sub.precomputed.*` and substitutes `{{toolingBlock}}` + `{{structureBlock}}`.
  - `forceBlock` ainda informa o agente que o cleanup foi feito (transparência), mas não pede pra ele rodar.

- `templates/scripts/scan/agent-prompt.template.md`
  - REMOVE Step 2 (Backup).
  - REMOVE Step 3 (notes.md ensure).
  - Renumera Steps 4-7 → 2-5.
  - INSERIR antes do Step 4 atual (agora 2):
    ```
    {{toolingBlock}}

    {{structureBlock}}
    ```
  - Atualizar `forceBlock` para: "FORCE MODE: orchestrate.js already wiped mustard:generated skills/commands and re-built backups. You can proceed directly to source analysis."
  - Step 4.b (Tooling detection) — mantém mas instrui: "Use the `## Tooling detected` block above. Only re-read source files if a command looks wrong or incomplete."
  - Atualizar Budget guidance: "Target: ~20 tool uses, ~15k tokens (post-perf-spec). The orchestrator pre-computed tooling/structure/backup — agent only writes new artifacts."

- `templates/scripts/scan/finalize.js`
  - Sem mudança funcional. Já valida HARD CONTRACT (dispatchVerify).

### New unit tests (file)

`templates/scripts/__tests__/scan-precompute.test.js` — testa cada função pura de `_precompute.js`:
- backupGeneratedMds com mix de generated+user-authored
- purgeGeneratedSkills com mix
- ensureNotesMd idempotência
- buildToolingBlock TS+NPM, TS+pnpm, .NET, Python
- buildStructureBlock filtragem de DEFAULT_IGNORE

### Env vars novas

Nenhuma. Tudo controlado pelo flag existente `--force`.

## Decisions

- **Backup/cleanup só em --force**: incremental run preserva artefatos com hash inalterado; sem `--force` orchestrate nem entra nesses passos.
- **Tooling block é stack-aware mas opt-out**: se o subprojeto não tem package.json/csproj/pyproject.toml, o bloco vira string vazia. Agent cai no Step 4 manual normalmente.
- **Não pré-computar guards.md nem patterns.md**: esses precisam de inferência LLM — pré-computar geraria conteúdo genérico que polui o registry.
- **Não tocar em sync-detect**: a detecção atual já roda em <2s no Sialia. Otimização aqui não vale o blast radius.

## Out of scope

- Stack-specific guard inference (LLM job).
- Migrações de finalize.js (already in shape).
- Mudança no contrato HARD CONTRACT (apenas mudou a partir do prompt — orchestrator-side já alinhado).
- Acelerar dispatch via reduzir paralelismo (manter 5 em paralelo).

## Risks (eliminados por design)

- ~~Quebrar backup de arquivos user-authored~~ → `_precompute.js` lê primeiros 200 bytes e só toca arquivos com marker. Idempotente.
- ~~Force mode apagar skills user-authored~~ → mesmo critério: só toca subdirs cuja `SKILL.md` tem `<!-- mustard:generated`.
- ~~package.json corrompido derrubar orchestrate~~ → `buildToolingBlock` em try/catch, retorna '' em erro. Brief continua válido.
- ~~Mudança no template quebrar template existente~~ → AC-6 garante que steps removidos não existem mais; AC-4/-5 garantem novos blocks injetados.

## Checklist

- [x] `templates/scripts/scan/_precompute.js` criado com 4 funções
- [x] `templates/scripts/__tests__/scan-precompute.test.js` criado (>=8 testes)
- [x] `templates/scripts/scan/orchestrate.js` integra `_precompute.js` antes do registry refresh
- [x] `renderPrompt` substitui `{{toolingBlock}}` e `{{structureBlock}}`
- [x] `templates/scripts/scan/agent-prompt.template.md` remove Step 2+3, renumera, insere blocks
- [x] `.claude/` mirror sincronizado (`cp templates/scripts/scan/* .claude/scripts/scan/`)
- [x] AC-1..AC-8 todos PASS via `bun .claude/scripts/qa-run.js`
- [x] Atualizar `templates/commands/mustard/scan/SKILL.md` se algum passo do procedimento mudar — N/A: SKILL.md não referencia Steps 2-3 do template (grep vazio)

## Follow-up (post-CLOSE, user action)

- [ ] Validação manual no Sialia: rodar `/scan --force` após `mustard update`, comparar tool-use total antes/depois (target: -30% por agente).
- [ ] Commit em branch separada (`feat/scan-perf-precompute`); merge só após validar Sialia.

## Validation strategy

Para medir o ganho real **antes** de declarar o spec completo:

1. **Antes** do merge: rodar `/scan --force` no Mustard local, capturar `events.jsonl` da sessão. Contar `tool.use` events emitidos por cada agent (filtrar por `actor.kind=agent`).
2. **Depois** do merge: mesma medição. Calcular delta median tool uses por agente.
3. **Critério**: delta >= -30%. Caso contrário, reabrir investigação (talvez precise das otimizações Wave B — agentes mais lentos via guard inference local).
4. Se Sialia for validado, abrir PR para `dev_rubens` linkando este spec + screenshots de medição antes/depois.

## Concerns

- **CONCERN — ROOT resolution mudou de `__dirname` para `process.cwd()` (orchestrate.js:43).** Necessário para os ACs funcionarem rodando o script diretamente de `templates/scripts/scan/`. Mantém compatibilidade com a invocação canônica (`bun .claude/scripts/scan/orchestrate.js` a partir da raiz do projeto, exatamente como `templates/commands/mustard/scan/SKILL.md` documenta) e com o test scaffold (`spawnSync(..., {cwd: root})`). Risco residual: se algum caller passar a executar `cd .claude/scripts/scan && bun orchestrate.js`, o ROOT vira `.claude/scripts/scan/` em vez da raiz do projeto. Não há caller assim hoje (grep em `templates/`, `bin/`, `src/` confirma). Mitigação adicional opcional: fallback `__dirname`-based se `<cwd>/.claude/CLAUDE.md` não existir — fica fora deste spec.

- **CONCERN — `fallbackDetectFromClaudeMd` em orchestrate.js.** Adicionado pelo impl agent para que os ACs rodem em tmp dirs sem `.claude/scripts/sync-detect.js` instalado. Em projetos reais o sync-detect sempre existe (gerado por `mustard init`), então o fallback é dead code no caminho feliz. Aceito como inevitável para suportar ACs cross-shell sem copiar todo o `.claude/scripts/` para tmp.

## Contexto para sessão fresh

- Migration Bun-only acabou nesta branch (`dev_rubens`); CLI/hooks/scripts/tests todos em Bun.
- HARD CONTRACT no agent prompt foi adicionado: agentes devem escrever SKILL.md OU `_no-patterns.md`.
- `finalize.js` tem `dispatchVerify` que catches contrato quebrado pós-dispatch.
- `orchestrate.js` com `--force` já apaga `.cluster-cache.json` e roda `sync-registry.js --force` antes do dispatch (resolve registry stale).
- Este spec é o próximo passo: pre-computar trabalho determinístico que ainda sobrou no agent.
- Spec Sialia rodando: `2026-04-28-telegram-alerting` (não conflita, mexe em outras áreas).
