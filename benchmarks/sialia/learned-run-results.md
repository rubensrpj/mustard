# Medição: prompt sialia-partners na versão final (produto fiado + pontes aprendidas)

Rodada 2026-07-09, binário `mustard-rt` pós-`bccc3e8f`, projeto-prova com o modelo/dict/equivalences do sialia. Mesmas 4 seções das rodadas anteriores (o roteador fatia o prompt-pacote). Pontes aprendidas via `equivalence-learn` durante a sessão (6 comandos de uma linha): `abas→tab,tabs · telas→page,form,screen · indicar→indication,referral · ganhos→earnings · indicação→indication,referral · indicados→indicated,referred`.

## Antes (produto fiado, sem aprendizado) → Depois (com as 6 pontes)

| Seção | Cegos (radar) | Úteis no top-8 (julgado) | Descoberta nova |
|---|---|---|---|
| 0. Início | 5 → 5 (inflexões não ensinadas) | 6/8 → 6/8 | — (já era bom: dashboard/page #3, sistemas #1-2) |
| 1. Planos por canal | 3 → 3 (glue) | 7/8 → 7/8 | — (backend completo já no top-8) |
| **2. Indicar clientes** | **5 → 2** | **~4/8 → 7/8** | **`referrals/page.tsx` (a página real do indicar) + `tabs.tsx` #1 — nunca antes vistas** |
| **3. Meus ganhos** | **5 → 2** | **6/8 → 8/8** | **`earnings/page.tsx` + `earnings/loading.tsx` (a página real do Meus Ganhos) — nunca antes vistas** |

## O achado central

As DUAS páginas que SÃO as features pedidas (`referrals/` = indicar clientes; `earnings/` = meus ganhos) só entraram no resultado DEPOIS do ciclo aprender as pontes — custo total: 6 comandos de uma linha, efeito permanente (overlay sobrevive a re-scans) e imediato (consulta seguinte). O radar de ausências foi o que APONTOU o que ensinar: cada cego resolvido virou ponte.

## Nuance documentada (chave exata)

O lookup do aprendizado é por chave folded EXATA: `indicar` não cobre `indicação/indicados` — inflexões precisam da própria ponte (por isso 3 dos 6 learns). Alternativa futura (stemming no expand) fica anotada como NÃO-medida — a lição da sessão é que expansão a mais dilui; entra medida ou não entra.

## Residual honesto

- Cegos restantes são glue/inflexões (`seguindo, fecharam, meus, acordo, igual...`) — verbos genéricos que a lapidação do SKILL já tira no fluxo real.
- Do gabarito id15: alvo primário `sistemas/page.tsx` presente (seção 0 #2); os 2 secundários rotulados (IPartnerPortalService, partner-channels.graphql) seguem fora — os equivalentes funcionais (LinkedPartnerSalesChannelDto, SalesPlanChannelConfiguration) cobrem a mesma necessidade.
