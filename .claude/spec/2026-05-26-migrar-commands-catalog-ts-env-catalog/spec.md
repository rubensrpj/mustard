# Tactical Fix: Migrar commands-catalog.ts e env-catalog.ts (~250 strings PT) para o dicionario i18n do dashboard

### Stage: Analyze
### Outcome: Active
### Flags: 
### Scope: full
### Lang: pt-BR
### Checkpoint: 2026-05-26T15:42:02.294Z
### Parent: 2026-05-26-template-agnostic-audit

## Contexto

Tactical fix derivado de [[2026-05-26-template-agnostic-audit]].

O dashboard mantém dois catálogos de dados estruturados em `apps/dashboard/src/data/`:

- `commands-catalog.ts` — ~24 entradas de slash-commands, cada uma com 8 campos string (`short`, `simples`, `tecnico`, `when`, `examples`, etc.). 12 hits de PT-BR no language-audit.
- `env-catalog.ts` — ~50 chaves de env vars, cada uma com `title`, `description`, `hint`. 6 hits de PT-BR no language-audit.

São ~250 strings PT-BR em registros estruturados consumidos por `pages/Commands.tsx` e a UI de Settings (env). Durante o fix-loop da spec parent, os arquivos foram marcados com `// SPEC LANG: pt-allowed` (opt-out do audit) — esta sub-spec é o follow-up que paga essa dívida de forma rastreável.

Migração não é mecânica: cada record precisa ou (a) ser re-estruturado para guardar `{pt, en}` variantes (dobra o tamanho do arquivo + retipa consumidores), ou (b) ter cada literal extraído pro dicionário em `apps/dashboard/src/lib/i18n.ts` com chaves estáveis (`cmd.feature.short`, `cmd.feature.simples`, …) + getter `getCommands(lang)`. O round 1 da fix-loop parent já adotou (b) para 11 outros arquivos do dashboard — manter consistência.

## Métrica de sucesso

Após a migração, `mustard-rt run language-audit --format json` no repo retorna zero hits provenientes de `apps/dashboard/src/data/{commands-catalog,env-catalog}.ts`, **sem** o marker `// SPEC LANG: pt-allowed` (que é removido como parte da entrega). `pages/Commands.tsx` e a UI de Settings continuam exibindo as descrições corretas em pt-BR para usuários com `mustard.json#specLang === "pt-BR"` e em en-US quando configurado.

## Critérios de Aceitação

- [ ] AC-1: `apps/dashboard/src/data/commands-catalog.ts` não contém o marker `SPEC LANG: pt-allowed` — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/data/commands-catalog.ts','utf8');process.exit(c.includes('SPEC LANG: pt-allowed')?1:0)"`
- [ ] AC-2: `apps/dashboard/src/data/env-catalog.ts` não contém o marker `SPEC LANG: pt-allowed` — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/data/env-catalog.ts','utf8');process.exit(c.includes('SPEC LANG: pt-allowed')?1:0)"`
- [ ] AC-3: `mustard-rt run language-audit --format json` retorna zero hits para os dois arquivos — Command: `bash -c 'cargo run -q -p mustard-rt -- run language-audit --format json | node -e "let s=\"\";process.stdin.on(\"data\",c=>s+=c).on(\"end\",()=>{const j=JSON.parse(s);const targets=j.hits.filter(h=>h.file.includes(\"data/commands-catalog\")||h.file.includes(\"data/env-catalog\"));process.exit(targets.length===0?0:1)})"'`
- [ ] AC-4: `pnpm --filter mustard-dashboard tsc --noEmit` passa — Command: `pnpm --filter mustard-dashboard tsc --noEmit`

## Arquivos

- `apps/dashboard/src/data/commands-catalog.ts` — extrair strings → i18n dict; remover marker pt-allowed
- `apps/dashboard/src/data/env-catalog.ts` — extrair strings → i18n dict; remover marker pt-allowed
- `apps/dashboard/src/lib/i18n.ts` — adicionar ~250 chaves novas (`cmd.<slug>.<field>` e `env.<KEY>.<field>`) em pt-BR + en-US
- `apps/dashboard/src/pages/Commands.tsx` — consumir via `t(...)` lookup com fallback ao locale ativo
- Consumidores de env (settings/preferences) — atualizar para resolver via `t(...)`
