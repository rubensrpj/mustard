# Plan: Improve Memory Usage & Token Economy

## Context

Duas frentes de melhoria para o Mustard:

**A) Runtime:** Sync scripts e hooks acumulam dados desnecessários na memória em monorepos grandes.
**B) Tokens:** Conteúdo redundante entre commands/skills inflaciona o contexto dos agentes, desperdiçando tokens.

### Problemas de Runtime
1. Leitura completa de arquivos para hashing (`readFileSync`) — acumula conteúdo na RAM
2. Varreduras recursivas duplicadas — `collectSourceFiles()` chamada múltiplas vezes nos mesmos diretórios
3. Fila de agentes sem limite — queue cresce indefinidamente
4. Scan duplo no registry — `scanDotNetEntities()` e `scanDotNetEnums()` leem os mesmos `.cs` independentemente

### Problemas de Tokens/Contexto
5. `pipeline-execution/SKILL.md` (131 linhas) **repete ~70%** do conteúdo de `feature/SKILL.md` (132 linhas) — mesmas tabelas Signal/Layers, Scope Detection, Explore rules
6. `pipeline-config.md` Role Rules **repete** Role Rules de `pipeline-execution/SKILL.md`
7. Commands como `/feature` e `/bugfix` reinlinham regras de pipeline que já existem em `pipeline-execution`
8. Skills de referência (`senior-architect/references/`, `react-best-practices/references/rules/`) são muitos arquivos pequenos (~80 linhas cada) que poderiam ser consolidados

## Changes

### 1. Stream-based hashing in sync-detect.js (HIGH impact)

**File:** `templates/scripts/sync-detect.js`

**Problem:** `computeSourceHash()` (line 643-659) and `computeModuleHashes()` (line 665-784) read entire file contents into memory with `fs.readFileSync()` before hashing.

**Fix:** Replace `readFileSync` with incremental chunk-based reading using `fs.openSync`/`fs.readSync` with a fixed 64KB buffer, updating the hash incrementally. This avoids holding entire file contents in memory.

```js
// Before (lines 649-653):
const content = fs.readFileSync(path.join(ROOT, file), "utf-8");
hash.update(file);
hash.update(content);

// After:
hash.update(file);
hashFileStream(path.join(ROOT, file), hash);
```

New helper function `hashFileStream(filePath, hash)`:
- Opens file with `fs.openSync`
- Reads in 64KB chunks via `fs.readSync` into a reusable `Buffer.alloc(65536)`
- Calls `hash.update(buffer.subarray(0, bytesRead))` per chunk
- Closes fd in finally block

Apply this pattern in 4 places:
- `computeSourceHash()` line 651
- `computeModuleHashes()` API/library branch line 721
- `computeModuleHashes()` mobile branch line 745
- `computeModuleHashes()` UI branch line 771

### 2. Memoized directory collection in sync-detect.js (MEDIUM impact)

**File:** `templates/scripts/sync-detect.js`

**Problem:** `collectSourceFiles()` is called repeatedly for overlapping paths in `computeModuleHashes()`. Each call recursively walks the same directory tree.

**Fix:** Add a simple module-level `Map` cache keyed by absolute directory path. On cache hit, return cached results. Clear cache after `computeModuleHashes()` returns to avoid holding references.

```js
const _collectCache = new Map();

function collectSourceFiles(dir, maxDepth = 10, currentDepth = 0) {
  if (currentDepth === 0) {
    const cached = _collectCache.get(dir);
    if (cached) return cached.slice(); // return copy
  }
  // ... existing logic ...
  if (currentDepth === 0) {
    _collectCache.set(dir, results.slice());
  }
  return results;
}

function clearCollectCache() {
  _collectCache.clear();
}
```

Call `clearCollectCache()` at the end of the main execution block (around line 945).

### 3. Queue size cap in subagent-tracker.js (LOW-MEDIUM impact)

**File:** `templates/hooks/subagent-tracker.js`

**Problem:** Queue (`_queue.json`) grows unbounded if `SubagentStart` events don't consume entries. Pruning only happens on `SubagentStop` (60s TTL).

**Fix:**
- Add `MAX_QUEUE_SIZE = 10` constant
- In `handlePreToolUse()` (line 64-70): after pushing, if `queue.length > MAX_QUEUE_SIZE`, slice to keep only the last `MAX_QUEUE_SIZE` entries
- In `handlePreToolUse()`: also call `pruneQueue(stateDir)` before pushing (not just on Stop)

```js
// In handlePreToolUse, after line 69:
pruneQueue(stateDir); // prune stale before adding
const queue = readQueue(stateDir);
queue.push({ description, type: subagentType, queued_at: new Date().toISOString() });
// Cap size
if (queue.length > MAX_QUEUE_SIZE) {
  queue.splice(0, queue.length - MAX_QUEUE_SIZE);
}
writeQueue(stateDir, queue);
```

### 4. Single-pass file scanning in sync-registry.js (MEDIUM impact)

**File:** `templates/scripts/sync-registry.js`

**Problem:** `scanDotNetEntities()` (line 187) and `scanDotNetEnums()` (line 222) each independently call `collectFiles()` and read every `.cs` file. On a 500-file .NET project, this means 1000 file reads instead of 500.

**Fix:** Create a unified `scanDotNet(subprojectPath)` function that:
1. Calls `collectFiles()` once for `.cs` files
2. In a single loop, reads each file once
3. Runs both entity regex and enum regex on the same content
4. Returns `{ entities: Set, enums: Map }`

Update the caller (around line 340+) to use the unified function.

### 5. Guard-verify early termination (LOW impact, quick win)

**File:** `templates/hooks/guard-verify.js`

**Problem:** Line 111 uses `[...content.matchAll()]` which collects all matches into an array. Line 136 does the same for `using` statements.

**Fix:** For the cross-module check (line 111), use iterative `regex.exec()` with early `break` once `hasCrossModule` is found — no need to collect all matches. The `usings` matchAll on line 136 is already iterated linearly, so just convert from spread to a `while (exec)` loop.

## Files to Modify

| File | Type of Change |
|------|---------------|
| `templates/scripts/sync-detect.js` | Stream hashing + memoized collection |
| `templates/scripts/sync-registry.js` | Single-pass .NET scanning |
| `templates/hooks/subagent-tracker.js` | Queue size cap + eager pruning |
| `templates/hooks/guard-verify.js` | Early termination on regex matches |

---

## Part B: Token Economy & Context Quality

### 6. Deduplicar pipeline-execution vs feature/bugfix (HIGH impact)

**Arquivos:**
- `templates/skills/pipeline-execution/SKILL.md` (131 linhas)
- `templates/commands/mustard/feature/SKILL.md` (132 linhas)
- `templates/commands/mustard/bugfix/SKILL.md` (56 linhas)

**Problema:** `pipeline-execution/SKILL.md` contém quase todo o conteúdo detalhado das fases (ANALYZE, PLAN, EXECUTE, CLOSE, Role Rules). O `/feature` SKILL.md **repete** essas mesmas seções com variações mínimas. Quando um agente carrega ambos, gasta ~260 linhas de tokens em conteúdo redundante.

**Fix:** Refatorar `pipeline-execution/SKILL.md` como a **fonte autoritativa** das fases e regras. Nos commands `/feature` e `/bugfix`:
- Manter apenas: Trigger, Description, regras específicas do command, e referência explícita: `"Load skill: pipeline-execution for full phase details"`
- Remover seções duplicadas: Signal/Layers table, Scope Detection table, Explore rules, EXECUTE Phase, Role Rules
- Resultado: `/feature` de ~132 → ~50 linhas, `/bugfix` mantém ~56 linhas (já enxuto)

**Economia estimada:** ~80 linhas por invocação de feature (~400+ tokens)

### 7. Deduplicar Role Rules entre pipeline-config.md e pipeline-execution (MEDIUM impact)

**Arquivos:**
- `.claude/pipeline-config.md` (68 linhas)
- `templates/skills/pipeline-execution/SKILL.md` (131 linhas)

**Problema:** Role Rules (Role, Color, Boundary, Validate, Return sections) aparece em ambos os arquivos. `pipeline-config.md` é lido pelo orchestrator, `pipeline-execution` é carregado por agents.

**Fix:** Manter Role Rules completo apenas em `pipeline-config.md` (que é a referência canônica lida pelo orchestrator). Em `pipeline-execution/SKILL.md`, substituir a tabela Role Rules por uma referência: `"Role boundaries and validation: see pipeline-config.md § Role Rules"`.

**Economia estimada:** ~10 linhas por load de pipeline-execution (~50 tokens)

### 8. Consolidar reference files de skills grandes (LOW-MEDIUM impact)

**Arquivos:**
- `templates/skills/react-best-practices/references/rules/*.md` (18+ arquivos, ~80 linhas cada)
- `templates/skills/senior-architect/references/*.md` (3 arquivos, ~103 linhas cada)
- `templates/skills/skill-creator/agents/*.md` (3 arquivos, ~200+ linhas cada)

**Problema:** Muitos arquivos pequenos de referência. Quando Claude carrega uma skill, ele pode ler múltiplos arquivos de referência, cada um com overhead de ferramenta (Read tool call). 18 arquivos de 80 linhas = 18 Read calls com overhead.

**Fix:** Consolidar os `rules/*.md` em um único `rules-catalog.md` com seções por regra (usando headers `### rule-name`). O Claude pode buscar por seção específica com Grep em vez de ler arquivos individuais.

Para `senior-architect/references/`: manter separados (são 3 arquivos de 103 linhas, tamanho razoável).
Para `skill-creator/agents/`: manter separados (são prompts de agente, devem ser carregados individualmente).

**Economia estimada:** ~15 Read tool calls reduzidos a 1-2 Grep calls para react-best-practices

---

## Arquivos a Modificar

### Part A: Runtime
| Arquivo | Mudança |
|---------|---------|
| `templates/scripts/sync-detect.js` | Stream hashing + cache de coleta |
| `templates/scripts/sync-registry.js` | Scan unificado de .NET |
| `templates/hooks/subagent-tracker.js` | Limite de fila + prune eagerly |
| `templates/hooks/guard-verify.js` | Terminação antecipada de regex |

### Part B: Tokens
| Arquivo | Mudança |
|---------|---------|
| `templates/commands/mustard/feature/SKILL.md` | Remover seções duplicadas, referenciar pipeline-execution |
| `templates/skills/pipeline-execution/SKILL.md` | Remover Role Rules duplicado, referenciar pipeline-config |
| `templates/skills/react-best-practices/references/rules/*.md` | Consolidar em rules-catalog.md |

## Verificação

1. **sync-detect.js**: `node templates/scripts/sync-detect.js` — JSON idêntico ao anterior (mesmo algoritmo de hash)
2. **sync-registry.js**: `node templates/scripts/sync-registry.js` — entity-registry.json idêntico
3. **subagent-tracker.js**: Fila capped a 10 entries
4. **guard-verify.js**: `node --test templates/hooks/__tests__/hooks.test.js` se testes existem
5. **Hooks**: Todos mantêm fail-open (exit 0)
6. **Feature/Pipeline skills**: Verificar que `/feature` ainda funciona corretamente referenciando pipeline-execution
7. **react-best-practices**: Verificar que a skill ainda carrega regras corretamente via rules-catalog.md
8. **npm run build**: Build do projeto sem erros
