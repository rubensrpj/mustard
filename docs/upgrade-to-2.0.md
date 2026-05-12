# Upgrading Mustard 1.x → 2.0

## TL;DR

Mustard 2.0 adiciona um Event Store SQLite (FTS5), OpenTelemetry token tracking, e um MCP server para memory queries. Hooks continuam compatíveis com Node E Bun. Zero breaking changes em projetos existentes — a migration roda automaticamente. Esta doc descreve passos seguros de upgrade, backup e rollback.

## What changes

### Adicionado

- `.claude/.harness/mustard.db` — SQLite + FTS5 (Phase 1) — projeção single-writer de `events.jsonl`
- `.claude/.harness/spans.jsonl` — OTLP JSON spans (Phase 2) — token usage por modelo/fase/agente
- `.claude/.harness/.active-spans/` — sidecar dir para bridging Pre/Post hooks
- MCP server `mustard-memory` auto-spawned via `settings.json.mcpServers` (Phase 3)
- `.claude/mustard.json` — runtime info (Phase 0) — `runtime: bun|node|auto`
- `templates/hooks/_lib/{event-store.js,span-emitter.js,runtime-shim.js}` — CJS wrappers

### Removido

- `.pipeline-states/*.metrics.json` (substituído por `metrics_projection` table)
- Campo `agentAttempts` em scripts/dashboards (substituído por `dispatchFailuresByPhase`)
- `.subagent-registry.json` (substituído por in-memory query via `EventStore.query({event:'agent.start'})`)

### Mantido (compat)

- `events.jsonl` continua sendo writer principal (dual-write design — DB é projeção)
- `knowledge.json` continua sendo writer (DB é projeção via migration)
- `mustard.json` (root) com git-flow config

## Backup

Antes de qualquer upgrade, snapshot completo do `.claude/`:

```bash
# Linux/macOS
cp -r .claude .claude.backup-pre-2.0
```

```powershell
# Windows PowerShell
Copy-Item -Recurse .claude .claude.backup-pre-2.0
```

`mustard update` também cria backup automático em `.claude.backup.{timestamp}/` (timestamp = epoch ms). A partir do bugfix de 2026-05-12 (Phase 0), o backup roda mesmo com `--force`.

Recomenda-se manter ao menos uma cópia explícita (`.claude.backup-pre-2.0`) fora do fluxo de auto-backup, para o caso de você precisar reverter depois de várias rodadas de `mustard update`.

## Upgrade steps

1. **Update Mustard CLI**:

   ```bash
   cd /path/to/mustard
   git pull
   npm install
   npm run build
   ```

2. **Update target project** (cria backup automático):

   ```bash
   cd /path/to/your/project
   node /path/to/mustard/bin/mustard.js update --force
   ```

   - Substitui: `hooks/`, `scripts/`, `commands/`, `skills/`, `refs/`, `recipes/`, `context/`
   - Preserva: `CLAUDE.md`, `pipeline-config.md`, `mustard.json` (root + `.claude/`), `docs/`, `spec/`, `agent-memory/`, `knowledge.json`

3. **Run migration** (popula SQLite a partir do `events.jsonl` + `knowledge.json` existentes):

   ```bash
   bun /path/to/mustard/dist/migrate/jsonl-to-sqlite.js .claude/.harness
   ```

   - **Idempotente**: rodar 2x produz mesmo `eventCount` (validado em snapshot do sialia: 1787 events, 56 knowledge entries)
   - Cria `mustard.db` em `.claude/.harness/`
   - Importa: events, knowledge, specs, metrics projection

4. **Validate**:

   ```bash
   node .claude/scripts/dashboard.js --check
   ```

   Deve retornar `{"ok":true,...}` com exit code 0.

5. **Próxima sessão Claude Code**:
   - Em SessionStart, se `mustard.db` ausente, migration roda automaticamente
   - MCP server `mustard-memory` é spawn pelo Claude Code via `settings.json.mcpServers`

## Rollback

Se algo quebrar:

```bash
# Linux/macOS
rm -rf .claude
mv .claude.backup-pre-2.0 .claude
```

```powershell
# Windows PowerShell
Remove-Item -Recurse -Force .claude
Move-Item .claude.backup-pre-2.0 .claude
```

Ou usar o backup automático criado pelo `mustard update`:

```bash
rm -rf .claude
mv .claude.backup.{timestamp} .claude
```

A versão 2.x mantém compat layer (hooks fallback para `events.jsonl` legacy se EventStore indisponível). Versão 3.0 remove o compat shim — só faça upgrade pra 3.0 depois que sua infra estiver estável em 2.x.

## Known limitations (2.0 release)

1. **EventStore wrapper requer Mustard em `node_modules/`**: projetos que rodam Mustard via path absoluto (`node C:/path/to/mustard/bin/mustard.js`) caem no fallback legacy. Resolve quando Mustard for publicado como npm package.

2. **MCP server path em settings.json**: `dist/mcp/mustard-memory.js` (cwd = raiz do Mustard). Quando Mustard virar npm package, o path passa para `node_modules/mustard/dist/mcp/mustard-memory.js`.

3. **Bun obrigatório para EventStore**: o store usa `bun:sqlite`. Hooks rodam sob Bun OU Node — mas o DB só inicializa sob Bun. Sem Bun instalado, dashboard cai para fallback legacy (lê `events.jsonl` direto).

4. **Windows CI advisory**: Bun on Windows ainda flaky em CI; o job `windows` em `.github/workflows/ci.yml` é `continue-on-error: true` em 2.0. Promove para hard-required em 2.x quando estável.

5. **knowledge_fts**: schema corrigido na Phase 4 Wave 1 (standalone FTS5 com UNINDEXED `id`). Migrations criadas antes deste fix crashavam com "database disk image is malformed". `EventStore.init()` auto-detecta o schema antigo via `sqlite_master` e dropa antes de recriar.

## Migration timeline

- **2.0**: Migration roda automaticamente em SessionStart se DB ausente. Reads via EventStore, writes ainda em `events.jsonl`.
- **2.1**: Dual-write — writes também passam por EventStore.
- **3.0**: `events.jsonl` vira backup. DB é truth.
- **3.0**: Compat shim removido. Mustard CLI vira npm package.

## Verificação pós-upgrade

Checklist mínimo de smoke test após o upgrade:

- [ ] `node .claude/scripts/dashboard.js --check` retorna `{"ok":true}` exit 0
- [ ] `.claude/.harness/mustard.db` existe e abre sob Bun (`bun -e "require('bun:sqlite').Database; console.log('ok')"`)
- [ ] Rodar `mustard update` segunda vez: backup novo criado em `.claude.backup.{timestamp2}/`
- [ ] MCP server aparece em `claude mcp list` (se Claude Code CLI instalado)
- [ ] Sessão Claude Code abre sem erros — hooks logam para `events.jsonl` normalmente

## Troubleshooting

| Sintoma | Diagnóstico | Fix |
|---------|-------------|-----|
| `database disk image is malformed` ao rodar migration | knowledge_fts old schema (pre-Wave 1) | Apague `mustard.db*` e rode migration de novo — `EventStore.init` regenera com schema novo |
| Dashboard retorna `ok:false` | Bun ausente ou DB corrompido | Instale Bun (`scoop install bun` no Windows) e rode migration manualmente |
| MCP tools não aparecem | `settings.json.mcpServers` não registrou | Verifique `settings.json` tem entry `mustard-memory` apontando para `dist/mcp/mustard-memory.js` |
| Migration trava em projeto grande | Sem progress reporter ainda em 2.0 | Verifique tail do `events.jsonl` — migration lê linha-a-linha, demora ~500ms por 1000 events |
