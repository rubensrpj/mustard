# W3 — Spec meta sidecar (absorve `2026-05-24-meta-sidecar`)

## Contexto

Absorve integralmente a spec ativa `2026-05-24-meta-sidecar`. Hoje cada spec guarda metadata (stage, outcome, phase, scope, lang, checkpoint, parent) como `### X:` headers no início do `spec.md`. O parser (`apps/rt/src/run/spec_sections.rs`) varre regex e mantém tabela bilíngue de variantes. Esta onda move metadata para `meta.json` lateral, simplifica o parser, e atualiza todos os escritores.

A absorção significa: as 4 ondas originais da spec `2026-05-24-meta-sidecar` viram as tasks T3.1..T3.4 desta onda.

## Tarefas

- [ ] **T3.1 (W1 original).** Schema `meta.json` em `packages/core/src/meta.rs` (struct `Meta` com `stage`, `outcome`, `phase`, `scope`, `lang`, `checkpoint`, `parent`, `isWavePlan`, `totalWaves`). Leitor + escritor com `core-lenient-serde-model`. `pipeline_state_ingest` lê JSON primeiro, fallback ao parser antigo se ausente.
- [ ] **T3.2 (W2 original).** Migração one-shot via novo `mustard-rt run migrate-to-meta`: percorre `.claude/spec/**`, cria `meta.json` ao lado de cada `spec.md` copiando valores dos headers atuais. Headers continuam no `.md` por enquanto (espelho). Atomic per-file via tempfile+rename, idempotente.
- [ ] **T3.3 (W3 original).** Escritores adaptados: `wave-scaffold`, `emit-pipeline`, `tactical-fix-create` (W6 entrega), `spec-scaffold` (W6 entrega) passam a escrever `meta.json` ao criar specs novas. Dashboard ganha comando Tauri `read_spec_meta` em `apps/dashboard/src-tauri/src/commands/specs.rs`.
- [ ] **T3.4 (W4 original).** Cleanup: remove headers `### Stage:`, `### Outcome:`, `### Phase:`, `### Scope:`, `### Lang:`, `### Checkpoint:`, `### Parent:` dos `.md` (passam a viver só no JSON). Simplifica `spec_sections.rs` (tira tabela de variantes pros campos de máquina). Decide o destino do `migrate_spec_headers.rs` (deletar ou converter para escritor de meta.json).
- [ ] **T3.5.** Schema `meta.json#lang` aceita formato BCP-47 (`pt-BR`/`en-US`) — alinha com W4. Aceita também formas curtas durante migração (`pt`/`en`) com warning para retrocompatibilidade, mas escritores novos só geram BCP-47.
- [ ] **T3.6.** Emit `pipeline.economy.operation.invoked { operation: "meta-sidecar-read", duration_ms }` quando `read_spec_meta` é chamado pelo dashboard, em vez do parser antigo.

## Files

- `packages/core/src/meta.rs` (novo)
- `packages/core/src/lib.rs` (exportar `meta`)
- `apps/rt/src/run/migrate_to_meta.rs` (novo)
- `apps/rt/src/run/pipeline_state_ingest.rs` (lê JSON primeiro)
- `apps/rt/src/run/wave_scaffold.rs` (escreve meta.json)
- `apps/rt/src/run/emit_pipeline.rs` (escreve meta.json)
- `apps/rt/src/run/spec_sections.rs` (simplificar parser)
- `apps/rt/src/run/migrate_spec_headers.rs` (destino: simplificado ou deletado)
- `apps/dashboard/src-tauri/src/commands/specs.rs` (novo `read_spec_meta`)
- `.claude/spec/**/meta.json` (criados em massa pela migração)
- `.claude/spec/**/spec.md` (headers removidos pela migração)

## Critérios de Aceitação

- [ ] AC-W3-1: Toda spec em `.claude/spec/**` tem `meta.json` válido com campos obrigatórios. Command: `node -e "const fs=require('fs'),path=require('path');const root='.claude/spec';for(const d of fs.readdirSync(root)){const p=path.join(root,d);if(!fs.statSync(p).isDirectory())continue;const m=path.join(p,'meta.json');if(!fs.existsSync(m)){console.error('missing',m);process.exit(1)}const j=JSON.parse(fs.readFileSync(m,'utf8'));for(const k of ['stage','outcome','phase','scope','lang','checkpoint']){if(!(k in j)){console.error('missing field',k,'in',m);process.exit(1)}}}"`
- [ ] AC-W3-2: `pipeline_state_ingest.rs` lê `meta.json`. Command: `node -e "const t=require('fs').readFileSync('apps/rt/src/run/pipeline_state_ingest.rs','utf8');if(!/meta\\.json/.test(t))process.exit(1)"`
- [ ] AC-W3-3: `wave_scaffold.rs` e `emit_pipeline.rs` escrevem meta.json. Command: `node -e "const fs=require('fs');for(const f of ['apps/rt/src/run/wave_scaffold.rs','apps/rt/src/run/emit_pipeline.rs']){if(!/write_meta|meta\\.json/.test(fs.readFileSync(f,'utf8'))){console.error(f);process.exit(1)}}"`
- [ ] AC-W3-4: Dashboard tem comando Tauri `read_spec_meta`. Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src-tauri/src/commands/specs.rs','utf8');if(!/read_spec_meta|meta\\.json/.test(t))process.exit(1)"`
- [ ] AC-W3-5: Após migração, `.md` não contém headers `### X:` mais. Command: `node -e "const fs=require('fs'),path=require('path');const root='.claude/spec';let bad=[];for(const d of fs.readdirSync(root)){const p=path.join(root,d);if(!fs.statSync(p).isDirectory())continue;for(const f of fs.readdirSync(p)){if(!f.endsWith('.md'))continue;const txt=fs.readFileSync(path.join(p,f),'utf8');if(/^###\\s+(Stage|Outcome|Phase|Scope|Lang|Checkpoint|Parent):/m.test(txt))bad.push(path.join(p,f))}}if(bad.length){console.error(bad);process.exit(1)}"`
- [ ] AC-W3-6: `spec_sections.rs` não contém tabela de variantes para `stage`/`outcome`/`phase`/`scope`. Command: `node -e "const t=require('fs').readFileSync('apps/rt/src/run/spec_sections.rs','utf8');if(/\"stage\"|\"outcome\"|\"phase\"|\"scope\"/.test(t))process.exit(1)"`

## Notas

- W3 é gargalo: W4 e W5 dependem.
- A migração one-shot é idempotente (rerun não duplica nem corrompe).
- Cuidado: a própria mega-spec (parent) e suas 14 sub-waves precisam ter `meta.json` gerado.
