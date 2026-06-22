---
id: wave.matar-prd-standalone-fazer-feature.3-dashboard
---

# wave-3-dashboard

## Resumo

Tirar o PRD do dashboard como página de autoria/disparo (a rota /prd inteira sai) e, no lugar, adicionar um atalho de LEITURA: uma aba "PRD" no detalhe da spec que fatia e mostra só a camada PRD (entre os marcadores <!-- PRD --> e <!-- PLAN -->) do spec.md já buscado. Read-only, sem IA, sem disparo.

## Rede

- Pai: [[matar-prd-standalone-fazer-feature]]
- Depende de: [[wave-1-grill]], [[wave-2-purge-prd]]

## Tarefas

- [ ] Remover a rota de autoria de PRD por completo: deletar Prd.tsx, features/prd/IntentHero, hooks/useLapidator.ts, api/prd.ts, lib/types/prd.ts e src-tauri/src/prd_lapidator.rs (e qualquer artefato morto remanescente do funil trigger_feature)
- [ ] Tirar o registro do handler trigger_feature do src-tauri/src/lib.rs, remover a rota /prd e seu import no App.tsx, e remover qualquer link de menu/sidebar para /prd; limpar as chaves i18n da autoria de PRD
- [ ] Adicionar um helper determinístico slicePrdSection(md) que extrai o trecho entre os marcadores `<!-- PRD -->` e `<!-- PLAN -->` do spec.md (fail-open: sem marcadores → string vazia/aviso)
- [ ] Adicionar uma aba "PRD" no SpecDetailDashboard (junto de Ondas/Trace/Qualidade/Rede, via SpecDrillDown) que renderiza o slice da camada PRD com o componente Markdown já existente, reusando o spec.md já buscado (fetchSpecMarkdown) — sem novo comando Tauri

## Arquivos

Removidos (autoria de PRD sai do dashboard):
- `apps/dashboard/src/pages/Prd.tsx` *(delete)*
- `apps/dashboard/src/features/prd/IntentHero/index.tsx` *(delete)*
- `apps/dashboard/src/hooks/useLapidator.ts` *(delete)*
- `apps/dashboard/src/api/prd.ts` *(delete)*
- `apps/dashboard/src/lib/types/prd.ts` *(delete)*
- `apps/dashboard/src-tauri/src/prd_lapidator.rs` *(delete)*

Editados (tirar referências + adicionar a aba):
- `apps/dashboard/src/App.tsx` — remover rota `/prd` + import
- `apps/dashboard/src-tauri/src/lib.rs` — remover o registro do handler `trigger_feature`
- `apps/dashboard/src/lib/i18n.ts` — limpar chaves da autoria de PRD
- `apps/dashboard/src/features/specs/SpecDetailDashboard/index.tsx` (+ `SpecDrillDown`) — nova aba "PRD"
- `apps/dashboard/src/lib/` — novo `slicePrdSection` helper (corte `<!-- PRD -->`..`<!-- PLAN -->`)

<!-- wikilinks-footer-start -->
- [matar-prd-standalone-fazer-feature](?) ⚠ unresolved
- [wave-1-grill](?) ⚠ unresolved
- [wave-2-purge-prd](?) ⚠ unresolved
<!-- wikilinks-footer-end -->