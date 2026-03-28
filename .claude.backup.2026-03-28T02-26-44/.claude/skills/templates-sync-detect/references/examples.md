<!-- mustard:generated at:2026-03-25T00:00:00.000Z role:general -->
# Sync-Detect Examples

## Example 1: Adding a New Role Signal

To detect a new framework, add scoring in `detectRole()`:

```js
// In detectRole(absPath):
// Python FastAPI → api (HIGH)
if (fileExists(absPath, 'pyproject.toml')) {
  if (pyprojectHas(absPath, ['fastapi', 'django', 'flask'])) {
    scores.api += ROLE_WEIGHTS.HIGH;
  }
}
```
Ref: `scripts/sync-detect.js` (lines 271-276)

## Example 2: Adding Package.json Dep Detection

```js
const deps = getPackageJsonDeps(absPath);
if (deps.length > 0) {
  if (hasAnyDep(deps, ['express', 'fastify', 'hono', 'koa', 'nestjs'])) {
    scores.api += ROLE_WEIGHTS.MEDIUM;
  }
  if (hasAnyDep(deps, ['react', 'next', 'vue', 'nuxt', 'svelte'])) {
    scores.ui += ROLE_WEIGHTS.MEDIUM;
  }
}
```
Ref: `scripts/sync-detect.js` (lines 240-254)

## Example 3: Module-Level Hash Computation

```js
function computeModuleHashes(subprojectPath, role) {
  const modules = {};
  if (role === 'api' || role === 'library') {
    const allModuleDirs = findAllModulesDirs(absPath);
    // ... collect files per module, compute SHA-256 per module
    for (const [modName, files] of Object.entries(moduleFiles)) {
      files.sort();
      const hash = crypto.createHash('sha256');
      for (const f of files) {
        hash.update(f);
        hash.update(fs.readFileSync(path.join(ROOT, f), 'utf-8'));
      }
      modules[modName] = { hash: hash.digest('hex'), files: files.length };
    }
  }
  return modules;
}
```
Ref: `scripts/sync-detect.js` (lines 665-784)
