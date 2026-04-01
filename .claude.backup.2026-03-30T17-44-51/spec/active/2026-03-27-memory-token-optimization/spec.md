# Enhancement: Memory Usage & Token Economy Optimization

### Status: completed | Phase: CLOSE | Scope: full
### Checkpoint: 2026-03-27T00:00:00Z

## Summary

Otimizar runtime (memory) dos sync scripts/hooks e reduzir redundância de tokens nos skills/commands do Mustard. Part A ataca RAM em monorepos grandes; Part B elimina conteúdo duplicado entre skills.

## Entity Info

Não envolve entidades de domínio — alterações em tooling interno (templates/).

## Part A: Runtime Optimization

### templates-impl Agent (Wave 1)

#### sync-detect.js — Stream hashing + memoized collection
- [x] Criar helper `hashFileStream(filePath, hash)` com `openSync`/`readSync` em chunks de 64KB
- [x] Substituir `readFileSync` em `computeSourceHash()` por `hashFileStream`
- [x] Substituir `readFileSync` em `computeModuleHashes()` — branch api/library
- [x] Substituir `readFileSync` em `computeModuleHashes()` — branch mobile
- [x] Substituir `readFileSync` em `computeModuleHashes()` — branch ui
- [x] Adicionar cache `_collectCache` (Map) em `collectSourceFiles()` com limpeza ao final
- [x] Build/type-check: 15/15 testes passando

#### sync-registry.js — Single-pass .NET scanning
- [x] Criar `scanDotNet(subprojectPath)` unificado que retorna `{ entities: Set, enums: Map }`
- [x] Leitura única de `.cs` files com ambos os regex (entity + enum) na mesma passagem
- [x] Atualizar caller para usar função unificada
- [x] Build/type-check: 15/15 testes passando

#### subagent-tracker.js — Queue size cap
- [x] Adicionar `MAX_QUEUE_SIZE = 10`
- [x] Em `handlePreToolUse`: chamar `pruneQueue` antes do push + cap com `splice`
- [x] Verificar que hook mantém fail-open

#### guard-verify.js — Early termination
- [x] Linha 111: trocar `[...matchAll()]` por `regex.exec()` loop com early break
- [x] Linha 136: trocar `[...matchAll()]` por `while (exec)` loop
- [x] Verificar que hook mantém fail-open

## Part B: Token Economy

### templates-impl Agent (Wave 2)

#### feature/SKILL.md — Dedup vs pipeline-execution
- [x] SKIPPED — /feature é intencionalmente self-contained; overlap é load-bearing para operação autônoma do command

#### pipeline-execution/SKILL.md — Dedup Role Rules
- [x] Substituir Role Rules inline por referência: "see pipeline-config.md § Role Rules" (132→125 linhas)
- [x] Manter fases (ANALYZE, PLAN, EXECUTE, CLOSE) como fonte autoritativa

#### react-best-practices rules — Consolidação
- [x] Consolidar 43 arquivos em 8 por categoria (async, bundle, rendering, rerender, server, js, client, advanced)
- [x] Atualizar `_sections.md` para referenciar arquivos consolidados
- [x] Deletar 43 arquivos individuais

## Files (~12)

| File | Action |
|------|--------|
| `templates/scripts/sync-detect.js` | modify — stream hashing + cache |
| `templates/scripts/sync-registry.js` | modify — single-pass scan |
| `templates/hooks/subagent-tracker.js` | modify — queue cap |
| `templates/hooks/guard-verify.js` | modify — early termination |
| `templates/commands/mustard/feature/SKILL.md` | modify — dedup |
| `templates/skills/pipeline-execution/SKILL.md` | modify — dedup Role Rules |
| `templates/skills/react-best-practices/references/rules/*.md` | delete 43 → create 5 consolidated |

## Dependencies

- Wave 1 (runtime) e Wave 2 (tokens) são independentes, podem rodar em paralelo
- Dentro de Part A: cada arquivo é independente

## Risks

1. **Hash compatibility**: Stream hashing produz bytes raw vs `readFileSync("utf-8")` que decodifica. Resultado: hash diferente → invalidação de cache única. Aceitável, mas deve ser documentado no commit.
2. **Feature skill reference**: Após dedup, `/feature` deve referenciar pipeline-execution corretamente. Testar fluxo completo.
3. **Rules consolidation**: 43 arquivos em 5 pode gerar arquivo grande. Se > 500 linhas cada, subdividir mais.

## Verification

1. `node templates/scripts/sync-detect.js` — JSON válido
2. `node templates/scripts/sync-registry.js` — entity-registry.json válido
3. Hooks mantêm fail-open (exit 0 on error)
4. `/feature` funciona corretamente referenciando pipeline-execution
5. `npm run build` — sem erros
