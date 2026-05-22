# Abas para o dashboard de specs

### Stage: Close
### Outcome: Active
### Flags: followup_open
### Scope: full (wave plan)
### Checkpoint: 2026-05-21T16:00:00Z
### Lang: pt
### Total waves: 6

## PRD

## Contexto

Hoje a rota `/specs` do dashboard mostra uma lista de cards e, quando o usuário clica num card, expande um drill-down inline embaixo dele com cinco sub-abas (Ondas, Trace, Qualidade, Rede, Sub-specs). Esse drill-down funciona pra uma spec, mas não para comparar duas. O usuário precisa fechar a primeira pra abrir a segunda, perde scroll, e o dashboard ocupa só metade da página enquanto a lista de cards fica visível em cima. O comportamento desejado é o do próprio Claude Code — uma barra horizontal de abas no topo, com botão `+` para abrir nova aba, `×` para fechar cada aba, e botão de atualizar dados. Cada card de spec ganha um botão "Detalhes" que abre o dashboard completo daquela spec como uma nova aba, em vez do drill-down inline.

Junto da troca do contêiner, cinco bugs específicos quebram a confiança no que cada sub-aba mostra. Ondas exibe "0 arquivos" em todas as ondas e não responde a clique. Trace tem expand mas o painel expandido não mostra payload nem actor (campos vazios). Qualidade marca "passou" sem revelar qual AC rodou e com qual comando. Rede renderiza um grafo desformatado e a coluna de memória cross-wave fica zerada (a query do `mustard-rt run memory cross-wave` quebra porque writers gravam `pipeline = "{spec-name}"` enquanto o reader busca por wave-name). Sub-specs não lista as tactical-fix sub-specs criadas no CLOSE, porque o reader hoje só consulta o projeção `spec_children` do SQLite (eventos `spec.link`) e ignora o cabeçalho `### Parent:` que vive no `spec.md` da sub-spec versionado em git. Em colaboração isso é falso: a sub-spec já tem o link explícito no header.

## Usuários/Stakeholders

Quem usa o dashboard pra acompanhar pipeline ativa em projeto compartilhado, principalmente o desenvolvedor que quer comparar duas specs (a que rodou e a que vai rodar, ou parent e sub-spec lado a lado) e verificar com confiança o que cada sub-aba mostra. O pedido nasceu desta sessão observando a spec `2026-05-21-flatten-spec-layout-and-multi-collab` recém-fechada — as seis ondas dela aparecem como "0 arquivos", a memória cross-wave fica zerada e os três tactical-fix sub-specs criados no CLOSE somem do painel.

## Métrica de sucesso

A rota `/specs` abre múltiplas specs em abas independentes, cada aba tem o dashboard completo daquela spec (cabeçalho com a fase corrente + 5 sub-abas), e cada sub-aba mostra dados verdadeiros e ricos (arquivos contam corretos, eventos abrem painel com payload + ts + actor, AC com id/comando/status/link, grafo legível com memória por wave preenchida, sub-specs incluindo as detectadas pelo header `### Parent:`). Saindo da rota `/specs` para qualquer outra rota descarta o conjunto de abas — sem state global no app por ora.

## Não-Objetivos

- Abas globais cross-route (decisão futura). Hoje cada rota pode reabrir o conceito; só `/specs` ganha tabs.
- Persistência de abas entre sessões/reload — sair e voltar pra `/specs` começa do zero (apenas a aba "Lista" abre).
- Atalho de teclado (Cmd+T para nova aba, Cmd+W para fechar, etc.) — fica pra spec futura.
- Mudar o conteúdo do drill-down como conceito (continua sendo Ondas/Trace/Qualidade/Rede/Sub-specs); só muda o contêiner e o conteúdo das cinco abas.
- Re-design do grafo de Rede pra outra biblioteca pesada (force-directed simulator custom basta — sem D3, cytoscape, react-flow).
- Renomear / reordenar tabs em runtime — abas abertas têm a ordem em que foram abertas, não há drag-and-drop.

## Critérios de Aceitação

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: Workspace compila sem erro (check sem relink, robusto a file-lock no Windows) — Command: `cargo check --workspace`
- [x] AC-2a: Testes do core passam — Command: `cargo test -p mustard-core --no-fail-fast`
- [x] AC-2b: Testes do rt passam — Command: `cargo test -p mustard-rt --bin mustard-rt --no-fail-fast`
- [x] AC-3: Dashboard tipa e builda — Command: `pnpm --filter mustard-dashboard build`
- [x] AC-4: A rota `/specs` renderiza um `<SpecTabBar>` no topo (componente existe e está montado na página) — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/pages/Specs.tsx','utf8');process.exit(/SpecTabBar/.test(s)?0:(console.error('SpecTabBar missing'),1))"`
- [x] AC-5: O `<SpecCard>` expõe um botão "Detalhes" que dispara `onOpenSpec` (callback prop) — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecCard.tsx','utf8');process.exit(/Detalhes/.test(s)&&/onOpenSpec/.test(s)?0:(console.error('Detalhes button or onOpenSpec missing'),1))"`
- [x] AC-6: `memory cross-wave` retorna memórias quando elas existem (a query agora considera `payload.spec` ou o campo `wave` do evento, e o fallback de filesystem do wave-plan funciona) — Command: `cargo test -p mustard-rt --bin mustard-rt memory_cross_wave::tests`
- [x] AC-7: `spec_children_v2` faz union entre eventos `spec.link` e specs com cabeçalho `### Parent:` no header — Command: `cargo test -p mustard-rt --bin mustard-rt spec_children`
- [x] AC-8: Dashboard Tauri command `dashboard_spec_wave_files` retorna o número real de arquivos listados no bloco `## Arquivos` do `wave-N-{role}/spec.md` — Command: `cargo test -p mustard-rt --bin mustard-rt wave_files`

## Plano

## Informações da Entidade

`SpecTab` (novo) — entidade interna de UI no `apps/dashboard/src/pages/Specs.tsx`:

```ts
type SpecTab =
  | { id: "list"; kind: "list" }
  | { id: string; kind: "spec"; specName: string };
```

`SpecChild` (já existe em `mustard-core` + `apps/dashboard/src-tauri/src/spec_views.rs`) — sem mudança de schema. A union nova adiciona linhas; o tipo permanece igual.

`memory cross-wave` — sem novo schema; muda a query (passa a casar por `spec` + `wave`, e o parser de `wave-plan.md` ganha fallback de filesystem).

Sem mudança em `HarnessEvent` nem em projeções do SQLite.

## Arquivos

Distribuídos por wave (cada `wave-N/spec.md` traz a lista exata). Resumo cross-wave:

```
apps/dashboard/src/pages/Specs.tsx                              — wave 1: state de abas + roteamento interno
apps/dashboard/src/components/specs/SpecTabBar.tsx              — wave 1: NOVO componente
apps/dashboard/src/components/specs/SpecDetailDashboard.tsx     — wave 1: NOVO contêiner por aba
apps/dashboard/src/components/specs/SpecCard.tsx                — wave 1: botão "Detalhes" + prop onOpenSpec
apps/dashboard/src/components/specs/SpecDrillDown.tsx           — wave 1: extraído para virar conteúdo de aba (sub-abas reaproveitadas)
apps/dashboard/src/components/specs/SpecWavesTab.tsx            — wave 2: contagem real + onda clicável
apps/dashboard/src/components/specs/WaveMarkdownDrawer.tsx      — wave 2: NOVO drawer para markdown da wave
apps/dashboard/src-tauri/src/lib.rs                             — wave 2 e 6: novos comandos Tauri
apps/dashboard/src-tauri/src/spec_views.rs                      — wave 2 e 6: spec_children_v2 ganha union de header
apps/dashboard/src/components/trace/ExecutionTrace.tsx          — wave 3: painel expandido com payload+ts+actor
apps/dashboard/src/components/trace/ToolEventRow.tsx            — wave 3: render do painel
apps/dashboard/src/components/specs/SpecQualityTab.tsx          — wave 4: AC details + link arquivo
apps/dashboard/src/components/specs/SpecNetworkTab.tsx          — wave 5: grafo Obsidian + render memórias
apps/rt/src/run/memory_cross_wave.rs                            — wave 5: fix query (spec+wave) + fallback filesystem
apps/rt/src/run/wave_files.rs                                   — wave 2: NOVO subcommand pra contar arquivos da wave
apps/rt/src/run/spec_children.rs                                — wave 6: NOVO subcommand union (events + Parent: header)
apps/rt/src/run/mod.rs                                          — wave 2 e 6: registra novos run-faces
```

## Tarefas

Wave-by-wave; detalhes em cada `wave-N/spec.md`. Resumo de dependências:

```
wave-1 (tab system)  ─┬─►  wave-2 (Ondas)
                     ├─►  wave-3 (Trace)
                     ├─►  wave-4 (Qualidade)
                     ├─►  wave-5 (Rede + memory)
                     └─►  wave-6 (Sub-specs)
                                ▼
                          review → qa
```

Wave 1 é bloqueante para todas as demais (todas as sub-abas vivem dentro do `SpecDetailDashboard` que Wave 1 cria). Wave 2-6 são independentes entre si (cada uma toca uma sub-aba diferente e, no máximo, um par de arquivos Rust/Tauri novos). Execução sequencial por padrão; `/resume` pode paralelizar 2/3/4 se o orquestrador detectar zero overlap.

## Tabela de Waves

| Wave | Spec                            | Role    | Resumo                                                 |
|------|---------------------------------|---------|--------------------------------------------------------|
| 1    | [[wave-1-ui]]                   | ui      | Tab system + SpecDetailDashboard + botão "Detalhes"     |
| 2    | [[wave-2-ui]]                   | ui      | Ondas: contagem real + drawer com markdown da wave      |
| 3    | [[wave-3-ui]]                   | ui      | Trace: painel expandido com payload + ts + actor        |
| 4    | [[wave-4-ui]]                   | ui      | Qualidade: AC details (id/label/cmd/status) + link teste|
| 5    | [[wave-5-general]]              | general | Rede: grafo Obsidian + fix memory cross-wave            |
| 6    | [[wave-6-general]]              | general | Sub-specs: union events + Parent: header                |

## Dependências

- shadcn `Tabs` já existe (`@/components/ui/tabs`) — reutilizado pra sub-abas dentro do `SpecDetailDashboard`.
- shadcn `Dialog` / `Sheet` (para o drawer da wave) — Sheet já existe em `@/components/ui/sheet`; se não, usar Dialog.
- `react-markdown` v10 (já no projeto) — render do markdown da wave.
- Sem nova dependência npm. Sem nova dependência Cargo.
- Rust: `mustard-core::SpecReader` já expõe `children_of`. O novo subcommand `spec_children` (Wave 6) chama esse + scan de filesystem; vive em `mustard-rt`.

## Limites

- `apps/dashboard/src/pages/Specs.tsx`
- `apps/dashboard/src/components/specs/SpecTabBar.tsx` (novo)
- `apps/dashboard/src/components/specs/SpecDetailDashboard.tsx` (novo)
- `apps/dashboard/src/components/specs/WaveMarkdownDrawer.tsx` (novo)
- `apps/dashboard/src/components/specs/SpecCard.tsx`
- `apps/dashboard/src/components/specs/SpecDrillDown.tsx`
- `apps/dashboard/src/components/specs/SpecWavesTab.tsx`
- `apps/dashboard/src/components/specs/SpecQualityTab.tsx`
- `apps/dashboard/src/components/specs/SpecNetworkTab.tsx`
- `apps/dashboard/src/components/specs/SpecChildrenTab.tsx`
- `apps/dashboard/src/components/trace/ExecutionTrace.tsx`
- `apps/dashboard/src/components/trace/ToolEventRow.tsx`
- `apps/dashboard/src/lib/dashboard.ts` (novas wrappers de invoke)
- `apps/dashboard/src-tauri/src/lib.rs`
- `apps/dashboard/src-tauri/src/spec_views.rs`
- `apps/rt/src/run/memory_cross_wave.rs`
- `apps/rt/src/run/wave_files.rs` (novo)
- `apps/rt/src/run/spec_children.rs` (novo)
- `apps/rt/src/run/mod.rs`

Out-of-boundary explicit: abas cross-route (Não-Objetivo), persistência de abas (Não-Objetivo), atalho de teclado (Não-Objetivo), troca do shadcn `Tabs` por outro componente, mudança no schema de eventos (`HarnessEvent` / projeções SQLite).

## Cobertura de Críticas

Cada item levantado no pedido e seu destino:

| Crítica do usuário | Bucket | Onde |
|---|---|---|
| Substituir drill-down inline por sistema de abas no padrão Claude Code | Coberto | Wave 1 |
| Barra horizontal de abas no topo, botão + e fechar por aba, refresh | Coberto | Wave 1 |
| Botão "Detalhes" em cada card abre nova aba | Coberto | Wave 1 |
| Header com fases ANALYZE/PLAN/EXECUTE/REVIEW/QA/CLOSE | Coberto | Wave 1 |
| Sub-abas Ondas/Trace/Qualidade/Rede/Sub-specs (reaproveitadas) | Coberto | Wave 1 |
| Bug 1: Ondas mostram "0 arquivos" + não clicáveis | Coberto | Wave 2 |
| Bug 1b: Cada onda abre modal/drawer com markdown renderizado | Coberto | Wave 2 |
| Bug 2: Trace lista exibida mas não expande com payload+ts+actor | Coberto | Wave 3 |
| Bug 3: Qualidade mostra "passou" sem AC details / link teste | Coberto | Wave 4 |
| Bug 4: Rede formatação ruim, refazer padrão Obsidian | Coberto | Wave 5 |
| Bug 4b: Memória por onda zerada — investigar query cross-wave | Coberto | Wave 5 |
| Bug 5: Sub-specs com `### Parent:` no header não aparecem | Coberto | Wave 6 |
| Bug 5b: Sub-specs via `spec.link` continuam aparecendo | Coberto | Wave 6 (union) |
| Abas globais cross-route | Não-Goal | Não-Objetivos |
| Persistência de abas entre sessões | Não-Goal | Não-Objetivos |
| Atalho de teclado Cmd+T etc | Não-Goal | Não-Objetivos |
| Escopo limitado à rota `/specs` (sair descarta) | Coberto | Wave 1 (state local em `Specs.tsx`) |

Todos os pontos do pedido mapeados. Zero items órfãos.
