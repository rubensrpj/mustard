# Fix: sync-detect.js não encontra subprojetos nested

## Context

Rodar `sync-detect.js` em `C:\Atiz\Competi\projetos\sialia` retornou `subprojects: []`, quebrando `/scan` e toda pipeline downstream. Causa raiz: `scanForSubprojects()` só olha filhos diretos do root. No Sialia, nenhum filho direto tem `CLAUDE.md` — todos os subprojetos estão nested:

- `apps/sialia-admin/CLAUDE.md`
- `apps/sialia-app/CLAUDE.md`
- `apps/sialia-partners/CLAUDE.md`
- `packages/sialia-core/CLAUDE.md`
- `backend/Sialia.Backend/CLAUDE.md` (único git submodule em `.gitmodules`)

Apenas 1 dos 5 subprojetos aparece via `git submodule status`, então o fallback precisa funcionar de verdade.

**Restrição** (memory `feedback_mustard_agnostic.md`): nada de hardcode de `apps/`, `packages/`, `backend/`. Mustard é 100% agnóstico — deriva tudo do filesystem.

## Approach

Ampliar `scanForSubprojects()` em `templates/scripts/sync-detect.js:523-541` para varredura BFS com profundidade limitada, parando em cada branch no primeiro `CLAUDE.md` encontrado (evita descer em `apps/sialia-app/.claude/CLAUDE.md` ou em nested workspaces).

### Critério de parada agnóstico

- Profundidade máxima: **3 níveis** (cobre `root/`, `apps/X/`, `services/api/v1/`).
- Em cada diretório, se existir `CLAUDE.md` na raiz dele → registra e **não desce** (evita duplicatas `X/` + `X/nested/`).
- Se não tiver CLAUDE.md → desce (respeitando ignore list).
- Ignore: `.*`, `node_modules`, `bin`, `obj`, `dist`, `.next`, `_backup`, `.claude` (evita pegar `.claude/CLAUDE.md` do template), `migrations`, pastas com extensão `.dll`/binary clutter já filtrados por serem files.

## Critical Files

| Arquivo | Mudança |
|---------|---------|
| `templates/scripts/sync-detect.js` | Reescrever `scanForSubprojects()` (linhas 520-541) como BFS até profundidade 3. Manter assinatura `() => string[]` de paths relativos ao ROOT (com separador `/`). |

### Função nova (pseudo)

```js
function scanForSubprojects() {
  const IGNORE = new Set([
    "node_modules", "bin", "obj", "dist", ".next",
    "_backup", "migrations", ".claude", ".git",
  ]);
  const MAX_DEPTH = 3;
  const results = [];

  function walk(absDir, relDir, depth) {
    if (depth > MAX_DEPTH) return;
    // Se o próprio dir tem CLAUDE.md (exceto o ROOT), registra e para.
    if (depth > 0 && fs.existsSync(path.join(absDir, "CLAUDE.md"))) {
      results.push(relDir.split(path.sep).join("/"));
      return;
    }
    let entries;
    try { entries = fs.readdirSync(absDir, { withFileTypes: true }); }
    catch { return; }
    for (const e of entries) {
      if (!e.isDirectory()) continue;
      if (e.name.startsWith(".")) continue;
      if (IGNORE.has(e.name)) continue;
      walk(path.join(absDir, e.name), path.join(relDir, e.name), depth + 1);
    }
  }

  walk(ROOT, "", 0);
  return results;
}
```

Reusa padrões já presentes no arquivo: `readdirSync({ withFileTypes: true })` + ignore list estão em `collectSourceFiles` (linha 671) e `dirExists` (linha 106) — consistente com o resto do script.

## Efeitos Colaterais & Pontos Neutros

- `main()` (linha 982-994) mescla `submodulePaths ∪ scannedPaths` sem deduplicar por path normalizado. Como `getSubmodulePaths` retorna `"backend/Sialia.Backend"` (slash) e o novo scan também retornará `"backend/Sialia.Backend"` (convertido), o `Set` de `seen` na linha 987 já deduplica corretamente.
- Warning "has CLAUDE.md but is NOT a git submodule" (linha 1064-1068) vai aparecer para os 4 subprojetos não-submodule do Sialia. É ruído esperado — o usuário já sabe que a política deles não é submodular tudo. **Não mudar esse warning nesta tarefa** (escopo creep).
- Cache `.detect-cache.json` com `subprojects: []` precisa ser invalidado. O usuário roda `/scan` com flag de force, ou deleta o arquivo manualmente. Se preferir garantir, o arquivo de cache velho tem `subprojects: []` e a checagem `cacheAge < TTL` ainda passaria — usuário precisa rodar `sync-detect.js --no-cache` uma vez. **Adicionar nota no commit**, mas não mexer no gate de cache.
- Erro stderr `no submodule mapping found in .gitmodules for path 'presentation'`: vem de `git submodule status` dentro do submodule `Sialia.Backend` (que tem seus próprios submodules internos quebrados). O catch em `getSubmodulePaths` (linha 515) já silencia, mas stderr vazava. Redirecionar stderr para null: já faz (`stdio: ["pipe", "pipe", "pipe"]`), então stderr é capturado mas não usado. **Sem ação necessária** — o erro já é fail-open.

## Verification

1. No Mustard:
   ```bash
   cd C:\Atiz\Mustard
   # Rodar teste existente do sync-detect
   node --test templates/hooks/__tests__/hooks.test.js
   ```
   (se houver teste específico para sync-detect, rodar; caso contrário confiar em smoke test manual)

2. Smoke test no Sialia:
   ```bash
   cd C:\Atiz\Competi\projetos\sialia
   node .claude/scripts/sync-detect.js --no-cache
   ```
   **Esperado**: JSON com 5 subprojects:
   - `sialia-admin` (ui)
   - `sialia-app` (ui)
   - `sialia-partners` (ui)
   - `sialia-core` (library ou general)
   - `Sialia.Backend` (api)

3. Smoke test negativo (não descer demais):
   - Verificar que `sialia-admin/.claude/` não foi confundido com subprojeto.
   - Verificar que dentro de `Sialia.Backend` o scan parou (já tem CLAUDE.md no topo dele).

4. Smoke test em monorepo flat (Mustard mesmo):
   ```bash
   cd C:\Atiz\Mustard
   node templates/scripts/sync-detect.js --no-cache
   ```
   **Esperado**: subproject `templates` aparece (mesmo comportamento de antes — não regredir).

## Out of Scope

- Erro do `git submodule status` stderr vazando: investigar separadamente se incomodar.
- Deduplicar warnings de submodule missing: escopo creep.
- Fix de cache `subprojects: []` stale: usuário roda com `--no-cache` uma vez.
- Rodar `/scan force` no Sialia: **não fazer neste plano** — é follow-up após o fix estar no Mustard e o template propagado.
