# Enhancement: knowledge-audit-redesign

### Status: closed | Phase: CLOSE | Scope: light
### Checkpoint: 2026-05-13T00:00:00Z
### Lang: pt

## Contexto

A página Knowledge mostra cards com `name`, `confidence%` e um bloco de `description` renderizado como Markdown. Para os tipos `entity-cluster` e `naming-pattern`, o `description` chega como blob estruturado (parece JSON cru) que o usuário lê na tela mas não consegue interpretar — não sabe se aquilo é load-bearing para alguma pipeline do Mustard ou se é só ruído acumulado. O card também esconde, por padrão, campos como `id`/`source` que só aparecem ao expandir, agravando a sensação de "dado que não diz nada". O efeito é dupla cegueira: o usuário não confia no que vê e também não sabe o que pode deletar.

## Resumo

Auditar todos os consumidores de `knowledge` (hooks, scripts, agentes, backend Rust) para mapear quais campos por tipo são load-bearing, e redesenhar o card com renderer schema-aware por tipo (sem JSON cru visível ao usuário).

## Limites

- `src/pages/Knowledge.tsx` (rewrite do bloco de card no browse mode)
- `src/components/KnowledgeCard.tsx` (novo — render por tipo)
- `.claude/spec/active/2026-05-13-knowledge-audit-redesign/audit-report.md` (novo — output da auditoria)
- Sem mudanças em `src-tauri/` nem em `.claude/hooks/` ou `.claude/scripts/`

## Checklist

### Frontend Agent

- [x] Auditar consumidores de knowledge: grep `.claude/hooks/`, `.claude/scripts/`, `.claude/agents/` para usos de `knowledge.json`, `KnowledgeRow`, `recipe-match`, `dashboard_knowledge_browse`. Listar arquivo + função + campos lidos
- [x] Auditar backend Rust: grep `src-tauri/src/` por `knowledge` para descobrir de onde o `description` é populado (arquivo de origem, parsing)
- [x] Escrever `audit-report.md` com seções: (1) Tipos observados, (2) Consumidores por tipo, (3) Campos load-bearing por tipo, (4) Campos não-usados (candidatos a remoção), (5) Recomendação de display
- [x] Criar `src/components/KnowledgeCard.tsx` com render por tipo (`entity-cluster`, `naming-pattern`, `decision`, `lesson`, `recipe`, `convention`, `pattern`) usando o mapping decidido na auditoria — parse de `description` quando for JSON, fallback Markdown caso contrário
- [x] Refatorar `src/pages/Knowledge.tsx` para usar `<KnowledgeCard />` no browse mode (linhas 240-256). Manter list mode (search expand) inalterado
- [x] `bun run build` e `bun run typecheck` passam

## Arquivos (~3)

- `src/pages/Knowledge.tsx` (modify)
- `src/components/KnowledgeCard.tsx` (new)
- `.claude/spec/active/2026-05-13-knowledge-audit-redesign/audit-report.md` (new)

## Acceptance Criteria

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: Build passa sem erros — Command: `bun run build`
- [x] AC-2: Audit report existe e cobre todos os tipos conhecidos — Command: `node -e "const fs=require('fs');const r=fs.readFileSync('.claude/spec/active/2026-05-13-knowledge-audit-redesign/audit-report.md','utf8');const types=['entity-cluster','naming-pattern','decision','lesson','recipe','convention','pattern'];const missing=types.filter(t=>!r.includes(t));if(missing.length){console.error('missing types:',missing);process.exit(1)}console.log('all types covered')"`
- [x] AC-3: KnowledgeCard component existe e é importado pela página Knowledge — Command: `node -e "const fs=require('fs');if(!fs.existsSync('src/components/KnowledgeCard.tsx'))process.exit(1);const k=fs.readFileSync('src/pages/Knowledge.tsx','utf8');if(!k.includes('KnowledgeCard'))process.exit(1);console.log('wired')"`

## Preocupações

- [WARN/layer-gap] `analyze-validation.js` reportou "Spec declares Frontend Agent but Files has no Frontend extensions" — falso positivo: a lista de Arquivos inclui dois `.tsx` válidos; o report `.md` co-listado confundiu a heurística. Não bloqueia.
