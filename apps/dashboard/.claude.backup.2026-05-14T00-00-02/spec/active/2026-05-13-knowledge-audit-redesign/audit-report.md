# Knowledge — Audit Report

Spec: `2026-05-13-knowledge-audit-redesign` · Date: 2026-05-13 · Scope: Light

## 1. Tipos observados (em dados reais)

Amostra de três projetos com Mustard ativo (`.claude/knowledge.json`):

| Type | Mustard | Sialia | Zelya | Sample (truncado) |
|---|---:|---:|---:|---|
| `convention` | 5 | 32 | 28 | `high-hook-retry-…` → `Pipeline triggered 3 hook-level retries (sandbox/stash-pop/re-prompts — not agent redispatches). Tool breakdown: {"Write":3,"Edit":6,"Bash":20,"Agent":1}` |
| `pattern` | 1 | 24 | 23 | `heavy-pipeline-…` → `Pipeline used 81 API calls. Consider splitting into smaller scope.` |
| `lesson` | 0 | 0 | 0 | (nenhum entry com `type: "lesson"` — `lesson` aparece apenas em `tags[]` de entries `convention`) |
| `recipe` | 0 | 0 | 0 | (não existe como `type`; recipes vivem em `.claude/recipes/*.json` com schema próprio: `name`, `operations`, `requires_entity`, `files[]`, `checklist`) |
| `decision` | 0 | 0 | 0 | (vive em `.claude/memory/decisions.json` com schema `{id, timestamp, content, source, context}` — nunca passa para a tabela SQLite `knowledge`) |
| `entity-cluster` | 0 | 0 | 0 | (label aspiracional; `cluster-discovery.js` emite `folder-cluster`/`suffix-cluster`/`base-class-cluster`/`decorator-cluster`/`function-prefix-cluster` direto para `entity-registry.json._patterns`) |
| `naming-pattern` | 0 | 0 | 0 | (não encontrado em nenhum knowledge.json nem entity-registry — `TYPE_LABELS` em `Knowledge.tsx` antecipa um nome que ninguém produz) |

**Conclusão**: a query SQLite (`db.rs:639-642`) só retorna o que estiver na tabela `knowledge`, que é alimentada exclusivamente via `scripts/knowledge-update.js` (escreve em `.claude/knowledge.json` — não na DB; a sincronização para SQLite vem do Claude Code core/harness shim, não deste repo). Os "tipos JSON-blob" que o usuário vê são `convention` e `pattern` cuja `description` embute `Tool breakdown: {…}` como **texto plano** (não é JSON renderizável, é literal de string).

## 2. Consumidores por tipo

| Type | Consumer (file:line) | Reads | Effect |
|---|---|---|---|
| `convention` | `.claude/hooks/convention-check.js:106-118` | `type`, `confidence` (≥0.8), `content`/`description`/`pattern` | Deriva regra `keyword in /dir/`; bloqueia/warns Write/Edit que violem path (`MUSTARD_CONVENTION_MODE=warn\|strict\|off`) |
| `pattern` | `.claude/hooks/convention-check.js:110` | mesmas + `type==='pattern'` | Mesmo efeito de path-rule (path-rule heuristic é universal) |
| `pattern` (heavy-pipeline) | `.claude/hooks/_lib/knowledge-extract.js:106-119` | escreve, ninguém lê | Display-only no dashboard |
| `convention` (high-hook-retry) | `.claude/hooks/_lib/knowledge-extract.js:87-102` | escreve, ninguém lê | Display-only no dashboard |
| `lesson`/`decision` | `.claude/scripts/memory-persist.js` + `.claude/hooks/memory-auto-extract.js` | `type`, `content`, `source`, `context` | Persistem em `.claude/memory/{decisions,lessons}.json` — **não chegam à tabela SQLite `knowledge`** lida pela dashboard |
| `recipe` (recipes.json files) | `.claude/scripts/recipe-match.js:140-172` | `name`, `operations[]`, `requires_entity`, `files[].{pattern,action,hint}`, `checklist[]`, `description` | Match por operation; resolve placeholders; **lê de `.claude/recipes/*.json`, não de knowledge.json** |
| `entity-cluster` / `naming-pattern` | (nenhum) | — | Labels existem em `src/pages/Knowledge.tsx:11-12` mas nenhum produtor grava esse `type`. Clusters reais vão para `entity-registry.json._patterns` (lido por `/scan` skill-generator) |

**Backend Rust** (`src-tauri/src/db.rs:441-468, 632-672`): apenas SELECT em `knowledge`/`knowledge_fts`. Nenhuma escrita. Sem parsing/inferência sobre `description`. Repassa string crua.

**Dashboard React** (`src/pages/Knowledge.tsx`): renderiza `description` via `<Markdown />` → o texto `Tool breakdown: {"Write":3,…}` cai como parágrafo de Markdown, exibido visualmente como blob.

## 3. Campos load-bearing por tipo

| Type | Field | Used by | Effect if missing |
|---|---|---|---|
| `convention`/`pattern` | `type` | `convention-check.js:110` | sem filtro → regra ignorada |
| `convention`/`pattern` | `confidence` | `convention-check.js:107` | `<0.8` → entry ignorado |
| `convention`/`pattern` | `description` | `convention-check.js:112-113` | sem regex match → sem regra derivada (silencioso) |
| `convention`/`pattern` | `name` | `knowledge-update.js:79` | dedupe key — duplicatas inflam tabela |
| `convention`/`pattern` | `id` | DB PK (`db.rs:454`) | UNIQUE constraint na tabela; bloqueia INSERT |
| `decision`/`lesson` (memory/*.json) | `content`, `source`, `context` | `memory-persist.js` | Histórico textual — nunca consumido programaticamente após gravar |
| recipe (`.claude/recipes/*.json`) | `operations[]`, `requires_entity`, `files[]`, `checklist[]` | `recipe-match.js:151-172` | sem match → fallback genérico |

## 4. Campos não-usados (candidatos a esconder/remover)

Presentes em entries reais mas nenhum consumidor neste repo:

- `tags[]` — escrito por `knowledge-extract.js`, nunca lido (nem pelo SELECT do backend que não retorna a coluna)
- `source` — retornado pelo backend mas nenhum hook/script lê após a gravação (apenas display)
- `occurrences`, `lastSeen`, `createdAt`, `updatedAt` — só usados para pruning interno (`knowledge-update.js:124`); nunca exibidos no card atual e nenhum hook decide sobre eles
- `prescription` — extraído por `knowledge-extract.js:43-61` mas o `knowledge-update.js` recebe a entry inteira e só persiste `name`/`description`/`source`/`tags` → **`prescription` é silenciosamente perdido na gravação**. Bug latente, não bloqueante.

## 5. Recomendação de display (por tipo)

Princípio: **renderer schema-aware** que extraia o sinal real de cada description sem mostrar JSON cru.

- **`convention` com prefix `high-hook-retry-`**:
  - Mostrar: spec name (parte após o prefix), contador de retries, e tool breakdown como **chips/pills** (`Edit×6  Bash×20  Write×3`) em vez do `JSON.stringify` cru.
  - Esconder: tags array, source string.
- **`pattern` com prefix `heavy-pipeline-`**:
  - Mostrar: spec name, número de API calls, frase recomendatória ("split into smaller scope") em badge informativo.
- **`convention` genérico (regra de path manual)**:
  - Mostrar: a `description` como Markdown (caso atual funciona).
- **`pattern` genérico**: idem `convention` genérico.
- **`decision` / `lesson` (se aparecerem via DB sync futuro)**:
  - Schema diferente (`content`+`context`); renderizar `content` como Markdown principal e `context` como subtítulo cinza.
- **`recipe`** (se aparecer): mostrar `description` + lista de `operations` como chips.
- **`entity-cluster` / `naming-pattern`**: até que alguém de fato grave esse `type`, manter fallback Markdown. Não dropar do `TYPE_LABELS` (zero custo manter), mas documentar como aspiracionais.

**Sobre o JSON-blob específico**: a `description` é uma string que **contém** um literal `{"Write":3,…}` no meio do texto. Tentar `JSON.parse(description)` sempre falha (não é JSON puro). O renderer precisa extrair o trecho `Tool breakdown: {…}` via regex e parsear só essa parte. Fallback: render Markdown como hoje.

## Resumo executivo

- **Tipos consumidos**: `convention`, `pattern` (pelo `convention-check` hook). Tudo o mais é display-only ou vive fora da tabela `knowledge`.
- **Campos load-bearing**: `type`, `confidence`, `description`, `name`, `id`. Restante é metadado de auditoria.
- **Risco de mudar o card**: zero — nenhum hook do `mustard-dashboard` lê o que o card mostra. Mudanças visuais não quebram pipelines.
- **Bug colateral encontrado** (não bloqueante): `knowledge-update.js` perde o campo `prescription` que `knowledge-extract.js:97` tenta anexar.
