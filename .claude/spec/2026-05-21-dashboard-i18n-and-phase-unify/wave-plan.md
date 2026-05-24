# i18n consistente + componente único de fases

## PRD

## Contexto

Dois problemas surgiram em uso real do dashboard depois da polish: (1) a "PipelineTimeline" tem duas implementações visuais — uma na Lista (`MiniTimeline`, scale-[0.82] do mesmo componente, com ring colorido na fase ativa) e outra em Detalhes (PipelineTimeline em escala normal, mas sem o ring/cores que a Lista mostra). O componente "PipelineTimeline" deveria ser o mesmo em ambas as rotas, ocupando o width disponível, com a fase EXECUTE destacada em verde (cor primária de "ação em curso" pedida explicitamente). Há também texto redundante embaixo da primeira fase (Analyze) mostrando o slug da spec — o slug já vive no header acima, então a duplicação polui o cartão. (2) A internacionalização não está consistente — a sidebar mostra "Knowledge" em inglês enquanto o resto está em português. A infraestrutura de i18n existe (`src/lib/i18n.ts` + página de Preferences com seletor de idioma), mas há strings em EN espalhadas pelo app que não passam pela `t(key)`. O usuário quer que TUDO siga o idioma escolhido em Preferences, via função/variável global, para evitar drift futuro.

## Usuários/Stakeholders

O usuário operando o dashboard em PT-BR após a polish anterior. Pedido nasce de inspeção visual real: ele identificou as duas inconsistências em screenshots do app rodando.

## Métrica de sucesso

Abrir a rota `/specs` e ver: o mesmo cartão de fases (`PipelineTimeline`) na lista e em cada aba de Detalhes, full-width, com EXECUTE em verde brilhante e a fase ATIVA pulsando + ring colorido. Sem texto redundante abaixo do Analyze. Trocar o idioma em Preferences para EN: a sidebar passa para "Overview / Specs / Economy / Knowledge / Commands / Preferences" (e títulos/subtitles de páginas seguem). Voltar para PT: tudo retorna. Zero strings hard-coded em EN no caminho da rota /specs e da sidebar.

## Não-Objetivos

- Traduzir TODAS as strings de TODAS as páginas do app (escopo audit é: sidebar + rota /specs + componentes compartilhados; outras rotas ficam para auditoria futura).
- Trocar a engine de i18n existente — usar o que está em `src/lib/i18n.ts` (extend se necessário).
- Adicionar idiomas além de PT e EN nesta passagem.
- Tradução de strings de log/erro/output do `mustard-rt` ou de comentários no código.

## Critérios de Aceitação

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: Workspace compila — Command: `cargo check --workspace`
- [x] AC-2: Dashboard tipa e builda — Command: `pnpm --filter mustard-dashboard build`
- [x] AC-3: `MiniTimeline` (helper local no SpecCard) NÃO existe mais como componente separado OU passa a renderizar via `PipelineTimeline` direto — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecCard.tsx','utf8');process.exit(/function\s+MiniTimeline/.test(s)?(console.error('MiniTimeline ainda separado'),1):0)"`
- [x] AC-4: `PipelineTimeline` aceita prop `variant` (compact|default) — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/telemetry/PipelineTimeline.tsx','utf8');process.exit(s.includes('variant')&&s.includes('compact')?0:1)"`
- [x] AC-5: EXECUTE phase color é green (não mais mustard) — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/lib/phase-palette.ts','utf8');const m=s.match(/execute:\s*\{[^}]+\}/);process.exit(m && /green|emerald-500|--color-ok/.test(m[0])?0:(console.error('execute ainda não é verde'),1))"`
- [x] AC-6: Sidebar usa `t(key)` para labels (não strings hard-coded) — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/layout/Sidebar.tsx','utf8');const hasT=/[\\bt\\(\\\\['\"]/.test(s)||/useTranslation|i18n/.test(s);const hasHardKnowledge=/['\"]Knowledge['\"]/.test(s);process.exit(hasT && !hasHardKnowledge?0:(console.error('hardcoded Knowledge ou sem t()'),1))"`
- [x] AC-7: Existe função global `t(key)` exportada de `@/lib/i18n` — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/lib/i18n.ts','utf8');process.exit(/export.*\\bt\\b|export function t|export const t/.test(s)?0:1)"`
- [x] AC-8: Catálogo de strings PT+EN existe com chave 'knowledge' (ou similar) — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/lib/i18n.ts','utf8');process.exit(/knowledge/i.test(s)&&/conhecimento/i.test(s)?0:(console.error('falta chave knowledge<->conhecimento no catálogo'),1))"`
- [x] AC-9: Subtitle redundante "{spec slug}" abaixo do Analyze no SpecDetailDashboard foi removido — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecDetailDashboard.tsx','utf8');const hasRedundant=/PipelineTimeline[\\s\\S]{0,400}\\{spec\\}/.test(s);process.exit(hasRedundant?(console.error('slug ainda aparece após PipelineTimeline'),1):0)"`

## Plano

## Informações da Entidade

Sem mudança de schema. Apenas refactor visual + extensão do módulo i18n.

`PHASE_COLORS` em `phase-palette.ts` ganha refresh:
- `analyze`: sky (mantém)
- `plan`: violet (mantém)
- `execute`: green-500/600 (TROCA de mustard para verde brilhante, atendendo pedido explícito)
- `review`: amber (TROCA de teal para âmbar, evita conflito com qa)
- `qa`: emerald (mantém)
- `close`: slate (mantém)

`PipelineTimeline` ganha prop `variant: "compact" | "default"`:
- `compact`: escala atual do MiniTimeline (~80%), ícones menores, layout horizontal apertado, usado em SpecCard.
- `default`: full-width, ícones grandes, espaçamento amplo, usado em SpecDetailDashboard.

`i18n.ts` ganha catálogo expandido com pelo menos as chaves: `sidebar.overview`, `sidebar.specs`, `sidebar.economy`, `sidebar.knowledge`, `sidebar.commands`, `sidebar.preferences`, `sidebar.add_project`, `route.specs.subtitle`, `phase.analyze`, `phase.plan`, `phase.execute`, `phase.review`, `phase.qa`, `phase.close`, `empty.no_events`, `count.acs`, `count.files`, `count.tools`, etc.

## Arquivos

```
apps/dashboard/src/components/telemetry/PipelineTimeline.tsx    — wave 1: variant prop + visual unify
apps/dashboard/src/components/telemetry/PhaseStation.tsx        — wave 1: aceita variant + tamanhos
apps/dashboard/src/components/specs/SpecCard.tsx                — wave 1: usa PipelineTimeline variant="compact"
apps/dashboard/src/components/specs/SpecDetailDashboard.tsx     — wave 1: usa PipelineTimeline variant="default" full-width + remove subtitle redundante
apps/dashboard/src/lib/phase-palette.ts                         — wave 1: execute=verde + review=amber
apps/dashboard/src/lib/i18n.ts                                  — wave 2: catálogo expandido + função t() global
apps/dashboard/src/i18n.ts                                      — wave 2: bootstrap/load do idioma de Preferences
apps/dashboard/src/components/layout/Sidebar.tsx                — wave 2: t('sidebar.knowledge') etc.
apps/dashboard/src/components/layout/Topbar.tsx                 — wave 2: t() onde houver string EN
apps/dashboard/src/pages/Specs.tsx                              — wave 2: subtitle via t()
apps/dashboard/src/pages/Knowledge.tsx                          — wave 2: título via t()
apps/dashboard/src/components/page/PageHeader.tsx               — wave 2: subtitle dinâmico (se aplicável)
```

## Tarefas

```
wave-1 (phase visual unify) ──┐
                              ├──►  review → qa
wave-2 (i18n audit)          ──┘
```

Waves independentes: W1 toca componentes de fase (PipelineTimeline, PhaseStation, phase-palette, SpecCard, SpecDetailDashboard). W2 toca i18n + sidebar/topbar/pages. Zero overlap em arquivos — podem rodar em paralelo.

## Tabela de Waves

| Wave | Spec            | Role | Resumo                                                            |
|------|-----------------|------|-------------------------------------------------------------------|
| 1    | [[wave-1-ui]]   | ui   | PipelineTimeline unificado, variant compact|default, execute=verde |
| 2    | [[wave-2-ui]]   | ui   | i18n audit: sidebar, topbar, rota /specs, knowledge label          |

## Dependências

Sem nova dep.

## Limites

- `apps/dashboard/src/components/telemetry/PipelineTimeline.tsx`
- `apps/dashboard/src/components/telemetry/PhaseStation.tsx`
- `apps/dashboard/src/components/specs/SpecCard.tsx`
- `apps/dashboard/src/components/specs/SpecDetailDashboard.tsx`
- `apps/dashboard/src/lib/phase-palette.ts`
- `apps/dashboard/src/lib/i18n.ts`
- `apps/dashboard/src/i18n.ts`
- `apps/dashboard/src/components/layout/Sidebar.tsx`
- `apps/dashboard/src/components/layout/Topbar.tsx`
- `apps/dashboard/src/pages/Specs.tsx`
- `apps/dashboard/src/pages/Knowledge.tsx`
- `apps/dashboard/src/components/page/PageHeader.tsx`

Out-of-boundary explicit: i18n de outras páginas (Economia, PRD, Visão Geral, Configurações) — auditoria futura, escopo restrito a sidebar + /specs + Knowledge nesta passagem. Comentários de código continuam em EN.

## Cobertura de Críticas

| Crítica do usuário | Bucket | Onde |
|---|---|---|
| (1) Aba `/specs` vazia ao entrar | Já corrigido inline antes deste spec (Specs.tsx hash gate) | Contexto |
| (2) Fases diferentes Lista vs Detalhes | Coberto | Wave 1 |
| (2b) Fases ocupam width, Execute verde em destaque | Coberto | Wave 1 |
| (2c) Mesmo componente em todas as rotas | Coberto | Wave 1 |
| (2d) Texto redundante "{slug}" abaixo do Analyze | Coberto | Wave 1 |
| (3) Descrições dos cards no idioma de Preferences | Coberto | Wave 2 |
| (3b) "Knowledge" em EN na sidebar | Coberto | Wave 2 |
| (3c) Variável/função global para evitar drift | Coberto | Wave 2 (`t()` exportada de `@/lib/i18n`) |

Todos os 7 sub-itens dos 3 pontos originais mapeados. Item (1) sai como contexto porque foi corrigido inline antes do spec — não há trabalho aqui.
