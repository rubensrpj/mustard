# Tactical-fix — remap `--ds-*` tokens nas primitivas migradas para tokens Binance

## Resumo

Wave 1 do parent eliminou os tokens legados `--ds-*` (sistema índigo/violeta pré-Binance) ao consolidar `src/style.css`. Wave 2 moveu 5 primitivas de `components/ds/` para `components/page/` byte-equivalente — preservou comportamento, mas as referências internas `var(--ds-surface-elevated)`, `var(--ds-intent-success)`, `var(--ds-text-secondary)`, `var(--ds-radius-md)`, etc. agora apontam para tokens **inexistentes**. CSS resolve para `unset`/`initial` silenciosamente: o build passa, o lint passa, mas `StatPill`, `BaseRow`, `CodeBlock`, `DiffViewer`, `TreeNode` renderizam sem cor de fundo, sem borda, sem cor de texto distintiva. Wave 4 vai compor `BaseRow` em listas de spec/economia e `StatPill` em todos os KPIs — se o remap não acontecer antes, a regressão visual escala para todas as páginas. Esta TF mapeia cada `--ds-*` usado para seu equivalente Binance no atual `style.css` e atualiza inline em cada um dos 5 arquivos.

## Token Remap Table

| Token --ds-* (legacy) | Substituto Binance (style.css atual) | Justificativa |
|---|---|---|
| `--ds-surface-elevated` | `--card` | card surface no DESIGN.md Binance |
| `--ds-surface-base` | `--background` | canvas |
| `--ds-text-primary` | `--foreground` | texto primário |
| `--ds-text-secondary` | `--muted-foreground` | texto secundário Binance #848e9c |
| `--ds-text-muted` | `--muted-foreground` | mesmo |
| `--ds-border` | `--border` | hairline Binance |
| `--ds-border-strong` | `--border` | Binance só tem 1 hairline; promover |
| `--ds-radius-sm` | `var(--radius)` (6px) | radius button Binance |
| `--ds-radius-md` | `var(--radius-card)` (8px) | radius card Binance |
| `--ds-radius-lg` | `var(--radius-card)` (8px) | mesmo |
| `--ds-intent-success` | `--intent-success` | já existe (#0ecb81) |
| `--ds-intent-error` | `--intent-error` | já existe (#f6465d) |
| `--ds-intent-warning` | `--intent-warning` | já existe (#f0b90b) |
| `--ds-intent-info` | `--intent-info` | já existe (#1e88e5) |
| `--ds-accent` | `--accent` | card-elevated Binance |
| `--ds-shadow-sm` | (remove ou usar `box-shadow: 0 1px 2px rgba(0,0,0,0.4)`) | Binance é flat; preserva profundidade mínima |

**Antes de editar:** rodar Grep `--ds-` em cada um dos 5 arquivos para listar TODOS os tokens usados; se algum não estiver na tabela acima, adicionar uma linha decisão (qual Binance é o match) — não silenciar.

## Arquivos

- `apps/dashboard/src/components/page/StatPill.tsx`
- `apps/dashboard/src/components/page/BaseRow.tsx`
- `apps/dashboard/src/components/page/CodeBlock.tsx`
- `apps/dashboard/src/components/page/DiffViewer.tsx`
- `apps/dashboard/src/components/page/TreeNode.tsx`

## Tarefas

- [ ] Para cada um dos 5 arquivos: rodar `rtk grep -n -- '--ds-' <arquivo>` e listar TODOS os tokens encontrados (não só os da tabela acima)
- [ ] Para cada token listado, aplicar substituição via Edit conforme tabela acima; se não estiver na tabela, decidir e documentar inline com comentário CSS `/* TF remap: --ds-X → --Y; justificativa */`
- [ ] Após substituições, rodar `rtk pnpm --filter mustard-dashboard build` — deve continuar passando
- [ ] Rodar `rtk grep -rE 'var\(--ds-' apps/dashboard/src/components/page` — deve retornar zero
- [ ] Boot manual rápido: `pnpm --filter mustard-dashboard dev` em background, abrir Workspace (usa StatPill via WorkspaceHero), confirmar visualmente que badges/pills/borders aparecem com cor (verde/vermelho/amarelo, hairline visível, card surface distinta do canvas)

## Acceptance Criteria

- [ ] AC-TF-D1: zero referências `var(--ds-X)` em `apps/dashboard/src/components/page/` — Command: `node -e "const {readdirSync,readFileSync,statSync}=require('fs');const {join}=require('path');function walk(d,out){for(const e of readdirSync(d)){const p=join(d,e);const s=statSync(p);if(s.isDirectory())walk(p,out);else if(/\\.(tsx?|css)$/.test(e))out.push(p)}return out}const files=walk('apps/dashboard/src/components/page',[]);const hits=files.filter(f=>/var\\(--ds-/.test(readFileSync(f,'utf8')));if(hits.length){console.error('still uses --ds-:',hits.join(','));process.exit(1)}console.log('ok')"`
- [ ] AC-TF-D2: build passa — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-TF-D3: tokens remapeados existem em `style.css` (cobrindo `--card`, `--background`, `--foreground`, `--muted-foreground`, `--border`, `--accent`, `--intent-success`, `--intent-error`, `--intent-warning`, `--intent-info`, `--radius`, `--radius-card`) — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/style.css','utf8');const need=['--card','--background','--foreground','--muted-foreground','--border','--accent','--intent-success','--intent-error','--intent-warning','--intent-info','--radius','--radius-card'];const miss=need.filter(t=>!c.includes(t));if(miss.length){console.error('missing tokens:',miss);process.exit(1)}console.log('ok')"`

## Limites

Editar dentro de:
- `apps/dashboard/src/components/page/StatPill.tsx`
- `apps/dashboard/src/components/page/BaseRow.tsx`
- `apps/dashboard/src/components/page/CodeBlock.tsx`
- `apps/dashboard/src/components/page/DiffViewer.tsx`
- `apps/dashboard/src/components/page/TreeNode.tsx`

OUT: tudo fora. NÃO editar `style.css` (Wave 1 entregou tokens). NÃO criar `--ds-*` novamente. NÃO mudar comportamento/JSX — apenas trocar identifier de token CSS.

## Modelo

sonnet (find/replace bem delimitado, sem decisão de design; tokens mapeados na tabela)
