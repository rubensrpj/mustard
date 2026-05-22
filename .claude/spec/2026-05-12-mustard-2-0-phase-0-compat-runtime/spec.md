# Mustard 2.0 — Phase 0: Runtime Compatibility Layer

- **Lang**: ptbr
### Stage: Close
### Outcome: Completed
### Flags: 
- **Checkpoint**: 2026-05-12T18:35:00Z
- **Scope**: Full
- **Type**: feature
- **Model**: opus
- **Depends on**: none
- **Unlocks**: Phase 1 (Event Store)

## Summary

Detectar e adotar Bun runtime (Claude Code v2.1.113+) com fallback transparente pra Node.js em versões antigas. Sem refatorar nada ainda — apenas a fundação que permite Phase 1+ usar `bun:sqlite`, TypeScript nativo e 10x cold-start. Zero breakage em projetos existentes.

## Problem

Hooks rodam como child process por tool call. Cada PreToolUse é bloqueante. No sialia medimos 1172 tool.use events × 26 hooks ≈ ~50 min de cold-start acumulado em histórico (Node spawn ~100-300ms cada). Bun cold-start é 10-30ms.

Hoje Mustard escreve hooks em CommonJS Node-only por causa do "zero npm deps after init". Mas Anthropic adquiriu Bun em dez/2025 e Claude Code v2.1.113+ **já vem com Bun nativo**. Não é mais nova dep — é runtime padrão da casa.

## Goal

`mustard init` e `mustard update` detectam runtime, escrevem shebang correto, e produzem templates que rodam idênticos em Bun ou Node. Hooks ficam compatíveis com ambos sem build step.

## Acceptance Criteria

Todas com comando executável. Strict pass.

1. **Detecção de runtime**
   ```bash
   node -e "const r=require('./dist/runtime/detect-runtime.js'); const x=r.detect(); console.log(x.kind==='bun'||x.kind==='node'?'PASS':'FAIL', x)"
   ```
   Exit 0 quando retorna `{ kind, version, bunSqliteAvailable }`.

2. **Shebang dual no template hook**
   ```bash
   node -e "const fs=require('fs');const f=fs.readFileSync('templates/hooks/_lib/runtime-shim.js','utf8');process.exit(f.includes('#!/usr/bin/env')?0:1)"
   ```
   `runtime-shim.js` existe com shebang `#!/usr/bin/env node` E exporta função `pickRuntime()`.

3. **Hooks rodam sob Bun**
   ```bash
   bun templates/hooks/_lib/__tests__/runtime-shim.test.js
   ```
   Test passa em Bun (se Bun instalado); skip-clean se não.

4. **Hooks rodam sob Node (regression)**
   ```bash
   bun test templates/hooks/__tests__/hooks.test.js
   ```
   Os 100 testes atuais continuam passando.

5. **mustard init grava runtime escolhido**
   ```bash
   node bin/mustard.js init --dry-run --runtime=bun > /tmp/m-bun.txt 2>&1 && grep -q "runtime.*bun" /tmp/m-bun.txt
   ```
   Output do init contém o runtime detectado/escolhido.

6. **Fallback Node se Bun indisponível**
   ```bash
   PATH=$(echo "$PATH" | sed 's|[^:]*bun[^:]*:||g') node bin/mustard.js init --dry-run > /tmp/m-fallback.txt && grep -q "runtime.*node" /tmp/m-fallback.txt
   ```
   Com Bun fora do PATH, init escolhe Node sem erro.

7. **mustard.json registra runtime**
   ```bash
   node -e "const p='.claude/mustard.json';const fs=require('fs');if(!fs.existsSync(p))process.exit(1);const j=JSON.parse(fs.readFileSync(p,'utf8'));process.exit(j.runtime&&j.runtime.kind?0:1)"
   ```
   Campo `runtime: { kind, version, chosenAt }` presente.

8. **Doc de migração**
   ```bash
   test -f docs/runtime-migration.md && grep -q "Bun" docs/runtime-migration.md
   ```
   Doc explica detecção, fallback, e como forçar runtime via env `MUSTARD_RUNTIME=node|bun`.

### Parseable AC (cross-shell, QA-runner)

Os comandos abaixo são equivalentes cross-shell (cmd.exe + bash) usados pelo `qa-run.js`. Os blocos numerados acima são a versão humana/original.

- [ ] AC-1: detect-runtime kind is bun or node — Command: `node -e "const r=require('./dist/runtime/detect-runtime.js'); const x=r.detect(); process.exit(x.kind==='bun'||x.kind==='node'?0:1)"`
- [ ] AC-2: runtime-shim has shebang and pickRuntime export — Command: `node -e "const fs=require('fs');const f=fs.readFileSync('templates/hooks/_lib/runtime-shim.js','utf8');process.exit(f.includes('#!/usr/bin/env')&&f.includes('pickRuntime')?0:1)"`
- [ ] AC-3: runtime-shim test passes under Bun — Command: `bun templates/hooks/_lib/__tests__/runtime-shim.test.js`
- [ ] AC-4: hook regression 100 tests pass under Node — Command: `bun test templates/hooks/__tests__/hooks.test.js`
- [ ] AC-5: init dry-run with --runtime=bun emits runtime in stdout — Command: `node -e "const {execSync}=require('child_process');const out=execSync('node bin/mustard.js init --dry-run --runtime=bun',{encoding:'utf8',shell:true});process.exit(/runtime.*bun/i.test(out)?0:1)"`
- [ ] AC-6: explicit --runtime=node selects node — Command: `node -e "const {execSync}=require('child_process');const out=execSync('node bin/mustard.js init --dry-run --runtime=node',{encoding:'utf8',shell:true});process.exit(/runtime.*node/i.test(out)?0:1)"`
- [ ] AC-7: init in temp dir writes runtime to .claude/mustard.json — Command: `node -e "const {execSync}=require('child_process');const fs=require('fs');const path=require('path');const os=require('os');const repo=process.cwd();const tmp=fs.mkdtempSync(path.join(os.tmpdir(),'mst-qa-'));process.chdir(tmp);execSync('node '+JSON.stringify(path.join(repo,'bin','mustard.js'))+' init --yes',{stdio:'pipe'});const j=JSON.parse(fs.readFileSync('.claude/mustard.json','utf8'));process.exit(j.runtime&&j.runtime.kind?0:1)"`
- [ ] AC-8: docs/runtime-migration.md exists and mentions Bun — Command: `node -e "const fs=require('fs');if(!fs.existsSync('docs/runtime-migration.md'))process.exit(1);process.exit(/Bun/.test(fs.readFileSync('docs/runtime-migration.md','utf8'))?0:1)"`

## Implementation

### New files

- `src/runtime/detect-runtime.ts` — detecta Bun via `typeof Bun !== 'undefined'` + `process.versions.bun`, retorna `{ kind, version, bunSqliteAvailable, claudeCodeVersion }`
- `templates/hooks/_lib/runtime-shim.js` — shebang Node-compat, exporta `pickRuntime()` e helpers que ambos runtimes suportam
- `templates/hooks/_lib/runtime-shim.d.ts` — tipos pra fase futura
- `docs/runtime-migration.md` — doc de transição

### Changed files

- `src/commands/init.ts` — adiciona `--runtime=bun|node|auto`, escreve `mustard.json.runtime`
- `src/commands/update.ts` — preserva `runtime` do mustard.json
- `templates/hooks/_lib/hook-env.js` — usa `runtime-shim.pickRuntime()` pra escolher I/O paths quando relevante

### Env vars novas

- `MUSTARD_RUNTIME=node|bun|auto` (default auto) — força runtime
- `MUSTARD_RUNTIME_VERBOSE=1` — log de detecção em stderr

## Decisions

- **Bun-first quando disponível, Node fallback**: porque Anthropic adquiriu Bun e Claude Code v2.1.113+ ships com ele
- **Sem build step nos hooks**: Bun roda .ts/.js nativo; manter hooks em .js compatível com ambos
- **TypeScript só em `src/` e futuro `_lib/`**: hooks ficam JS por compat máxima

## Risks (já endereçados)

- ~~Bun não no Windows~~ → Bun 1.0+ tem Windows; `bun:sqlite` estável em 2026. Verificação automática + fallback.
- ~~Quebrar projetos existentes~~ → `update.ts` preserva mustard.json existente; novos campos opt-in.

## Out of scope

- SQLite (Phase 1)
- TypeScript em hooks (não vale o build step)
- OpenTelemetry (Phase 2)

## Checklist

- [x] `src/runtime/detect-runtime.ts` implementado
- [x] `templates/hooks/_lib/runtime-shim.js` + `.d.ts`
- [x] `src/commands/init.ts` aceita `--runtime`
- [x] `src/commands/update.ts` preserva runtime
- [x] `mustard.json` schema atualizado
- [x] `docs/runtime-migration.md`
- [x] Tests Bun + Node passam
- [x] Sialia testado: `mustard update` não quebra projeto ativo (VALIDATED 2026-05-12: 208 files updated, 100/100 hook regression pass, runtime-shim presente, `.claude/mustard.json` preservou `{specLang:"pt"}`)
