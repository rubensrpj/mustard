# W13 — Close and archive

## Contexto

Encerrar a mega-spec consistentemente e gerar artefatos auditáveis. Confirma que tudo o que foi prometido foi entregue: backup das specs, schema do `mustard.db` lean, economia visível em `/economia`, ADR consolidada, vault Obsidian atualizado.

## Tarefas

### T13.1 — Gerar MANIFEST do backup

- [ ] Rodar `mustard-rt run backup-specs --apply --target ~/.mustard-backups/2026-05-24-pre-unification/` (subcomando entregue em W6).
- [ ] Gerar `MANIFEST.json` no diretório-raiz do backup com `{ created_at, source_path, file_count, checksums: { "<rel-path>": "<sha256>" }}`.

### T13.2 — Emit pipeline.status para todas as specs absorvidas/fechadas/superseded

- [ ] `pipeline.status: archived` para todas as ~55 specs Close/Completed do diretório `.claude/spec/`.
- [ ] `pipeline.status: archived` para as 4 absorvidas: `2026-05-24-meta-sidecar`, `2026-05-24-config-idioma-tom`, `2026-05-23-per-spec-event-log-claude-devtools`, `2026-05-22-telemetry-separation` (já fechada pós-W12).
- [ ] `pipeline.status: superseded` para `2026-05-20-economia-moat-unification`.

### T13.3 — Validar sync-registry final

- [ ] Rodar `mustard-rt run sync-registry` via `claude` CLI subprocess (W2 garantiu).
- [ ] Validar `entity-registry.json` tem `entities[]` populado.
- [ ] Diferença de tamanho do registry pré/pós-unificação reportada.

### T13.4 — Relatório final de economia

- [ ] `mustard-rt run economy report --format table --wave all` produz tabela executiva: cada wave + operação + delta.
- [ ] Total de tokens economizados por sessão típica reportado em `docs/adr/2026-05-24-mustard-unification.md`.
- [ ] Tamanho final do `mustard.db` reportado (alvo `< 1MB`).
- [ ] Tamanho final do `telemetry.db` pós-prune reportado.

### T13.5 — ADR consolidada

- [ ] Criar `docs/adr/2026-05-24-mustard-unification.md` documentando:
  - Backup strategy (fora do repo, cópia).
  - meta-sidecar adoption.
  - lang/tone config BCP-47.
  - Opt-in skills via `mustard add skill:nome`.
  - Memory scoping por spec.
  - Context budget per role.
  - NDJSON per-spec events (drop tabela `events`).
  - Telemetry retention 90d default.
  - LLM via `claude` CLI subprocess.
  - Dashboard sem grafo interno (wikilinks → Obsidian).
  - Triggers `Stop` + `Notification`.
- [ ] Referenciar specs absorvidas com `[[wikilinks]]`.

### T13.6 — Atualizar vault Obsidian

- [ ] `.claude/graph/index.md` recebe entry para a nova ADR e para a mega-spec.
- [ ] Confirmar wikilinks `[[2026-05-24-mustard-unification]]` no vault funcionam.

### T13.7 — Verificação global

- [ ] Rodar `mustard-rt run verify-pipeline --json` (W11 entregou multi-stack) — todos os subprojetos verde.
- [ ] Rodar `mustard-rt run qa-run-all` para garantir que nenhuma spec aberta tem AC pendente.
- [ ] Rodar `mustard-rt run docs-stale-check` — sem hits.
- [ ] Rodar `cargo clippy --workspace -- -D warnings` — sem warnings.
- [ ] Rodar `pnpm --filter mustard-dashboard build && pnpm --filter mustard-dashboard lint` — verde.

### T13.8 — Fechar a mega-spec

- [ ] Editar `meta.json` da mega-spec: `Outcome: Completed`, `Phase: CLOSE`.
- [ ] Emit `pipeline.status: archived` para `2026-05-24-mustard-unification` + 14 sub-waves.
- [ ] Atualizar [[MEMORY.md]] do user com `[Unification done](project_unification_done.md)` se aplicável.

## Files

- `~/.mustard-backups/2026-05-24-pre-unification/MANIFEST.json` (novo)
- `docs/adr/2026-05-24-mustard-unification.md` (novo)
- `.claude/graph/index.md` (atualizar)
- `.claude/entity-registry.json` (regenerado)
- `.claude/spec/**/meta.json` (Outcome atualizado das absorvidas)

## Critérios de Aceitação

- [ ] AC-W13-1: Backup em `~/.mustard-backups/2026-05-24-pre-unification/MANIFEST.json` existe. Command: `node -e "const fs=require('fs'),os=require('os'),path=require('path');const p=path.join(os.homedir(),'.mustard-backups/2026-05-24-pre-unification/MANIFEST.json');if(!fs.existsSync(p))process.exit(1);const j=JSON.parse(fs.readFileSync(p,'utf8'));if(!j.checksums||Object.keys(j.checksums).length<50)process.exit(1)"`
- [ ] AC-W13-2: Todas as ~55 specs Close/Completed têm event `pipeline.status: archived`. Command: SQL query no `mustard.db`.
- [ ] AC-W13-3: `entity-registry.json` tem `entities[]` não-vazio. Command: `node -e "const j=JSON.parse(require('fs').readFileSync('.claude/entity-registry.json','utf8'));if(!Array.isArray(j.entities)||j.entities.length===0)process.exit(1)"`
- [ ] AC-W13-4: `docs/adr/2026-05-24-mustard-unification.md` existe e referencia as 4 specs absorvidas via wikilinks. Command: `node -e "const t=require('fs').readFileSync('docs/adr/2026-05-24-mustard-unification.md','utf8');for(const s of ['2026-05-24-meta-sidecar','2026-05-24-config-idioma-tom','2026-05-23-per-spec-event-log-claude-devtools','2026-05-22-telemetry-separation']){if(!t.includes('[['+s)){console.error('missing wikilink',s);process.exit(1)}}"`
- [ ] AC-W13-5: `verify-pipeline --json` retorna `overall: pass`. Command: `rtk mustard-rt run verify-pipeline --json | node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{const j=JSON.parse(s);if(j.overall!=='pass')process.exit(1)})"`
- [ ] AC-W13-6: `cargo clippy --workspace -- -D warnings` limpo. Command: `rtk cargo clippy --workspace -- -D warnings 2>&1 | grep -q "No issues found"`
- [ ] AC-W13-7: `mustard.db` em projeto canário `< 1MB`. Command: `node -e "const fs=require('fs');const s=fs.statSync('.claude/.harness/mustard.db').size;if(s>1024*1024)process.exit(1)"`
- [ ] AC-W13-8: mega-spec `meta.json` tem `Outcome: Completed`. Command: `node -e "const m=JSON.parse(require('fs').readFileSync('.claude/spec/2026-05-24-mustard-unification/meta.json','utf8'));if(m.outcome!=='Completed')process.exit(1)"`
- [ ] AC-W13-9: `economy report --format json --wave all` exportado e anexado ao ADR. Command: derived from AC-13.4.

## Notas

- Sequencial após todas as outras 13 ondas (dependência total).
- Esta onda é a única que escreve em `~/.mustard-backups/` — único caminho destrutivo (cópia, não move).
- Após esta onda, a mega-spec inteira fica `Stage: Close / Outcome: Completed`.
