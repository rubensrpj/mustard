# wave-3-ui — Economia didática + card por-sessão + economias reais

## Resumo

Reformular a página de Economia para falar com o usuário final: cada card com
título claro em PT e uma linha explicando o que é e por que importa, sem nome de
campo interno nem jargão. Adicionar o card por-sessão (data/hora + custo + specs),
exibir as economias agora populadas (RTK real + injeção rotulada "estimado") e a
quebra de custo por spec/onda do estimado (`run_usage`), rotulada. Depende da
Wave 1 (dados) e se beneficia da Wave 2 (economias não-zero).

## Causa raiz

`apps/dashboard/src/pages/Economia.tsx` expõe nomes de campo internos como rótulo
("fonte: `economy_summary.top_agents_by_cost`", "`usage_totals.cost.usage`",
"`savings_breakdown`") e jargão ("spans", "frames", "Prevention breakdown",
"(top 0)", "rtk + routing + bash_guard + budget"). Nenhum card explica o que
significa. As economias de RTK/injeção aparecem 0.

## Arquivos

- `apps/dashboard/src/pages/Economia.tsx` — reescrita didática de todos os cards; card por-sessão; seção de economias; seção estimada por spec/onda
- `apps/dashboard/src/lib/types/economy.ts` — refletir os campos novos de `by_session` (`last_at_ms`, `specs`)
- `apps/dashboard/src/components/page/*` (se útil) — reusar primitivas de card/badge/empty-state existentes

## Component Contract

- **Cada card**: título em PT comum + 1 linha de legenda explicando o que mede e por que importa. Proibido exibir nome de campo (`economy_summary.*`, `usage_totals.*`, `savings_breakdown`) ou nome de módulo (`rtk`, `bash_guard`, `budget`, `routing`) como rótulo de usuário. Jargão traduzido: "spans"→"execuções", "frames"→"trechos de contexto", "Prevention breakdown"→"O que a ferramenta evitou de gastar", "(top 0)"→omitir quando vazio.
- **Custo medido**: título "Custo do projeto (medido)"; legenda "cobrado pela Anthropic, somado por sessão · atualizado há Xs"; manter o selo ao vivo/parado.
- **Por sessão**: lista com data/hora (de `last_at_ms`), custo USD, e as spec(s) trabalhadas (chips); legenda "compare uma sessão com `/cost` do Claude Code para conferir".
- **Economias**: cada intervenção (RTK, routing, injeção) com nome em PT + 1 linha do que faz; valor em tokens; injeção marcada "(estimado)". Sem itens com explicação técnica de mecanismo.
- **Por spec/onda (estimado)**: quebra do `run_usage`, rotulada "estimado (por execução)"; estado vazio didático quando não há dados ainda.
- Preservar layout/estética atual (dark, cards), só trocar conteúdo/cópia e somar o card por-sessão.

## Tarefas

### UI Agent (Wave 3)

- [x] Remover todos os rótulos "fonte: <campo>" e substituir por legenda em linguagem comum; traduzir jargão (mapa acima).
- [x] Card por-sessão: data/hora + custo + chips de spec (consumir `by_session` enriquecido da Wave 1); null-guard `data?.field`.
- [x] Seção de economias: exibir RTK e injeção (agora populadas pela Wave 2); injeção com selo "(estimado)"; cada item com título PT + 1 linha.
- [x] Seção custo por spec/onda (estimado): consumir `per_spec_costs`/`per_wave_costs`, rotulada; empty-state didático.
- [x] `economy.ts`: adicionar `last_at_ms`/`specs` ao tipo de sessão.
- [x] `cargo build -p mustard-dashboard` + `pnpm --filter mustard-dashboard build`.

## Critérios de Aceitação

- [x] AC-1: `cargo build -p mustard-dashboard` passa — Command: `cargo build -p mustard-dashboard`
- [x] AC-2: build do front passa — Command: `pnpm --filter mustard-dashboard build`
- [x] AC-3: sem nome de campo interno nos rótulos — Command: `node -e "const fs=require('fs');const s=fs.readFileSync('apps/dashboard/src/pages/Economia.tsx','utf8');process.exit(/economy_summary\.|usage_totals\.cost|savings_breakdown\b/.test(s)?1:0)"`

## Limites

- `apps/dashboard/src/pages/Economia.tsx`, `apps/dashboard/src/lib/types/economy.ts`, primitivas em `src/components/page/`
- Preservar shapes dos comandos Tauri e estética; só conteúdo/cópia + card novo
- NÃO alterar o reader da Wave 1 (consumir)
