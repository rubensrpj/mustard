# Salto de selecao (LLM hop, Haiku) sobre o funil deterministico — medicao final

**Hop Acc@5: 8/13 (61.5%) vs barra >=11/13 (84.6%) -> veredito: MORRE.** Deterministico na mesma sessao: 6/13 @5 (referencia historica 46.2%).

Generated 2026-07-09 00:08 by `hop-eval.ps1`. Product path: `mustard-rt run feature --intent <pt>` em projeto temporario com `retrieval.hop=haiku` (modelo sialia pre-buildado; equivalences = artefato C2 `equivalences-mt.json`; gloss sidecar: True). Pool ~25 candidatos (RRF k=60 rank+digest com evidencia por linha), 1 chamada `claude -p --model claude-haiku-4-5-20251001` (cwd neutro sem .claude, timeout 45s, fail-open), re-query no maximo 1 quando o modelo pede e menos de 5 picks validos. Ruler identico a serie: n=13, hit = target OU secundario, igualdade exata de caminho.

## Acc@5 / Acc@10 (n=13)

| Variant | Acc@5 | Acc@10 | hit ids @5 |
|---|---|---|---|
| deterministico (sanidade, mesma sessao) | 6/13 (46.2%) | 9/13 (69.2%) | 1,2,4,6,7,8 |
| **hop haiku (produto)** | **8/13 (61.5%)** | **9/13 (69.2%)** | 1,2,3,4,5,6,7,8 |

## Custo / latencia do hop (13 queries)

- chamadas claude: 9 (re-queries disparadas: 0; fallbacks para deterministico: 4)
- latencia media: hop 20022ms por query; run completa (funil+hop) 35.1s de parede
- tokens medios por query: in 21774 (inclui cache do CLI) / out 2103; total da medicao: in 283068 / out 27336

## Per-label best rank (target-or-secondary)

| id | det | hop | hit@5 det | hit@5 hop | mode | requeried |
|---|---|---|---|---|---|---|
| 1 | 4 | 1 | Y | Y | hop | False |
| 2 | 1 | 1 | Y | Y | hop | False |
| 3 | 7 | 1 | . | Y | hop | False |
| 4 | 2 | 2 | Y | Y | deterministic | False |
| 5 | 7 | 1 | . | Y | hop | False |
| 6 | 4 | 1 | Y | Y | hop | False |
| 7 | 2 | 1 | Y | Y | hop | False |
| 8 | 2 | 1 | Y | Y | hop | False |
| 9 | -1 | -1 | . | . | deterministic | False |
| 11 | -1 | -1 | . | . | hop | False |
| 12 | 8 | 8 | . | . | deterministic | False |
| 13 | -1 | -1 | . | . | hop | False |
| 14 | -1 | -1 | . | . | deterministic | False |

(-1 = fora do top-10 emitido; ranks sao o MELHOR entre alvo e secundarios.)

## REGRA DE PARADA

**Veredito: MORRE** — regra escrita antes da medicao: hop Acc@5 >= 11/13 -> VENCEU; senao MORRE. Resultado: 8/13 (61.5%) @5, 9/13 (69.2%) @10.

## Decomposicao honesta (por que 8 e nao 11)

Tres modos de perda, com pesos muito diferentes:

1. **Selecao (a inteligencia do salto) NAO e o gargalo.** Nas 9 queries em que a chamada
   completou, o hop acertou 7/9 @5 (77.8%) — e TODAS as 7 no top-1 (det tinha 4, 7, 7, 4, 2, 2, 2).
   Nenhuma alucinacao passou pela validacao; todo pick e um candidato verbatim do pool.
2. **Timeout de 45s = 4/13 queries (31%).** As chamadas que completaram levaram 19.5-38.8s
   (CLI `claude` frio + Haiku); as que passaram de 45s foram mortas (fail-open correto, insumos
   deterministico seguiu, exit 0). Custo direto na regua: id12 (alvo no pool em det#8 — o padrao
   das 7 promocoes diz que uma chamada completa muito provavelmente viraria hit) e id4 (salvou-se
   pelo floor deterministico det#2). id9/id14 timeoutaram mas o det tambem erra (-1). Um teto de
   ~90s ou um CLI mais quente mudaria ~1-2 hits; NAO foi re-rodado para nao maquiar a medicao.
3. **Teto do pool (o funil, nao o salto): id9, id11, id13.** O alvo nunca entrou nos ~25
   candidatos — o seletor nao pode escolher o que o funil nao oferece. O re-query (a valvula
   desenhada para isso) disparou 0 vezes: o modelo sempre teve >=5 picks plausiveis e nunca
   declarou "a lista nao tem o alvo". A condicao `<5 picks validos` quase nunca e verdadeira —
   se houver proxima iteracao, o gatilho certo e "confianca baixa", nao "poucos picks".

Leitura de produto: o salto CONFIRMA que a peca de selecao funciona (top-1 quase perfeito sobre
pool que contem o alvo, +2 hits @5 sobre o deterministico, 61.5% vs 46.2%) — mas morre na regua
de 84.6% porque (a) 1/3 das chamadas nao volta em 45s no Windows e (b) os 3 casos dificeis
historicos continuam fora do pool de 25. O ganho real e a barra sao problemas do FUNIL e da
LATENCIA, nao da chamada Haiku.

## Caso-estresse id15 — as 4 secoes do prompt sialia-partners pelo hop

Referencias do gabarito id15: target `apps/sialia-partners/app/(dashboard)/sistemas/page.tsx`; secundarios ``backend/Sialia.Backend/Application/Modules/v1/Partners/Services/IPartnerPortalService.cs``, ``packages/sialia-core/src/shared/schemas/partners-portal/partner-channels.graphql.ts``.

### revisar o projeto sialia-partners (portal de revenda): dashboard de indicações ativas e planos disponíveis

(mode=hop, 19.5s)

1. `apps/sialia-partners/app/(dashboard)/dashboard/page.tsx` [digest] — Main dashboard for active indications and plans
2. `packages/sialia-core/src/shared/schemas/partners-portal/partner-stats.graphql.ts` [both] — Stats schema for active indications data
3. `backend/Sialia.Backend/Application/Modules/v1/Partners/DTOs/PartnerPortalStatsDto.cs` [digest] — Backend data transfer object for portal stats
4. `backend/Sialia.Backend/Backend/Modules/v1/Partners/EndPoints/PartnerPortalEndPoints.cs` [digest] — API endpoints serving partner portal data
5. `packages/sialia-core/src/shared/schemas/contracts/contracts.zod.ts` [rank] — Contract and plan definitions schema

Avaliacao: fatia vertical COERENTE do dashboard do portal — a pagina real do dashboard do
sialia-partners em #1 + o pipeline de stats inteiro (schema graphql -> DTO -> endpoints). Os
`why` sao honestos e uteis. Nao bate o target do gabarito (`sistemas/page.tsx`, que representa o
prompt id15 INTEIRO, nao esta secao), mas para "dashboard de indicacoes ativas e planos" e
exatamente onde um dev abriria. Qualitativo: forte.

### listar planos por canal de venda do parceiro somente-leitura igual sales-plans do sialia-admin

(mode=deterministic, 46.4s)

1. `apps/sialia-app/app/(dashboard)/sales-plans/_components/form/fields/channels-section.tsx` [digest]
2. `packages/sialia-core/src/shared/schemas/contracts/contracts.zod.ts` [rank]
3. `backend/Sialia.Backend/Application/Modules/v1/SalesPlans/Repositories/ISalesPlanChannelRepository.cs` [digest]
4. `backend/Sialia.Backend/Application/Seeds/DatabaseSeeder.cs` [rank]
5. `backend/Sialia.Backend/Application/Modules/v1/SalesPlans/Repositories/SalesPlanChannelRepository.cs` [digest]

Avaliacao: TIMEOUT do hop (46.4s) — lista e o fallback deterministico. Ainda assim razoavel:
o exemplar do sales-plans/channels que a secao manda espelhar esta em #1 e o par de repositorios
de canal em #3/#5; o secundario `partner-channels.graphql.ts` do gabarito nao aparece. Mediano.

### CRUD de indicar clientes no padrão de cadastro de cliente do sialia-app com abas e validações

(mode=deterministic, 46.7s)

1. `packages/sialia-core/src/client/hooks/use-crud-permissions.ts` [both]
2. `packages/sialia-core/src/client/components/ui/tab-error-indicator.tsx` [digest]
3. `packages/sialia-core/src/shared/schemas/payables/payables.zod.ts` [rank]
4. `apps/sialia-app/app/(dashboard)/companies/_components/form/fields/address-tab.tsx` [digest]
5. `apps/sialia-app/app/(dashboard)/financial/_components/all-titles/all-titles-view.tsx` [rank]

Avaliacao: TIMEOUT do hop (46.7s) — fallback deterministico, e aqui doi: a lista tem ruido
classico do funil (payables.zod, all-titles-view — nada a ver com cadastro de cliente com abas).
So `use-crud-permissions` e `address-tab.tsx` (o padrao de aba do cadastro a espelhar) prestam.
E o tipo de lista que o salto existia para limpar. Fraco.

### dashboard meus ganhos dos clientes indicados que fecharam contrato

(mode=hop, 27.6s)

1. `apps/sialia-partners/app/(dashboard)/earnings/page.tsx` [digest] — Main earnings dashboard page for partner indicated customers
2. `apps/sialia-partners/app/(dashboard)/earnings/loading.tsx` [digest] — Loading state for earnings dashboard
3. `packages/sialia-core/src/client/hooks/use-contracts-dashboard.ts` [digest] — Dashboard-specific contracts hook with filter logic
4. `backend/Sialia.Backend/DataAccess/Enums/PartnerIndicationStatus.cs` [digest] — Enum for partner indication status filtering
5. `packages/sialia-core/src/client/hooks/use-contracts.ts` [rank] — Contracts hook for closed contract queries

Avaliacao: o melhor pacote das 4 secoes — a pagina REAL `earnings/page.tsx` do portal em #1
("meus ganhos" e literalmente earnings), status de indicacao (`PartnerIndicationStatus`) para o
filtro "clientes indicados" e os hooks de contrato para "fecharam contrato". Um dev implementa a
secao abrindo exatamente esses arquivos. Qualitativo: forte.

