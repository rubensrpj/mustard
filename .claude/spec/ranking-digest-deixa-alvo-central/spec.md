# Ranking do digest deixa o alvo central fora das ancoras (fixture sialia all-titles-view)

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

Caso de campo (sialia, 2026-06-12, feature "Painel Todos os Títulos"): a consulta ao digest (`mustard-rt run feature --intent ...`) retornou `strong` após a re-query com vocabulário do repositório, mas **o alvo central da feature — `all-titles-view.tsx` e `titles-table.tsx` (pasta `financial/`) — ficou fora das ~12 âncoras**. O orquestrador localizou os arquivos com Glob manual, quebrando o contrato "leia só as âncoras" que sustenta o baixo consumo de contexto do pipeline.

É a terceira evidência da mesma classe: (1) digest apontou o service errado em payables (mitigado pela nota anchors+hubs no `feature.rs`), (2) primeira query com lixo irrelevante (Safe2Pay/WebhookDeadLetter — comportamento esperado da ladder, mas sintoma do mesmo viés), (3) agora o alvo central ausente mesmo com match `strong`.

**Restrição herdada (decisão registrada):** o grafo está abafado por contrato e o ranking NÃO deve ser recalibrado às cegas — qualquer mudança de peso/seleção precisa ser dirigida por fixture com aferição antes/depois nos benchs existentes (`anchor_ranking.rs`, `stratified_samples.rs`).

**Pista de causa-raiz (verificada em campo, 2026-06-11):** no mesmo repositório sialia, `dig.graph.top_fan_in` (`hubs`) veio **vazio** para as consultas financeiras — sugerindo que o grafo de import do lado C# do monorepo (TypeScript + C#) não está sendo minerado/ligado, deixando o ranking léxico-puro sem o sinal de centralidade que pescaria tanto o service da lógica (caso payables) quanto a view central (este caso). Investigar a mineração do grafo C# ANTES de mexer em pesos: se o sinal de grafo nem chega, calibrá-lo é inútil.

Âncoras (do scan):
- apps/dashboard/src/features/workspace/WorkspaceFilesRanking/index.tsx (ranking)
- packages/core/src/view/projection/card.rs (view)
- apps/scan/tests/php_laravel_fixture.rs (fixture)
- apps/rt/src/commands/agent/digest_adherence_finalize.rs (digest)
- packages/core/src/domain/regression_check/mod.rs (fixture, fixtures)
- packages/core/src/view/projection/workspace.rs (ranks)
- apps/dashboard/src/components/page/EditorialBand/index.tsx (title)
- apps/scan/tests/stratified_samples.rs (ranking, fixture, digest)
- apps/scan/tests/anchor_ranking.rs (ranking, digest)
- apps/scan/tests/generated_class.rs (fixture, digest)
- apps/scan/tests/stack_detection_e2e.rs (fixture, digest)
- packages/core/src/domain/economy/sources/otel.rs (view, fixture)

Fatias recorrentes (precedente a espelhar): Report (×7), args (×2)

Por que agora: existe pela primeira vez uma fixture real e reproduzível (a run sialia, com query, modelo e alvo conhecidos) — a dívida estava registrada desde a avaliação payables sem material para dirigir a correção.

## Usuários/Stakeholders

O orquestrador do `/feature` na fase ANALYZE (consome as âncoras como única janela de leitura) e qualquer usuário do contrato "pesquise via digest; leia só o apontado" — quando o alvo central escapa, o contrato força Glob/Grep manual e o ganho de contexto do Mustard evapora.

## Métrica de sucesso

Na fixture derivada do caso sialia, o arquivo-alvo central entra no top-12 de âncoras para a query do caso real (hoje: 0/12), sem regressão nos benchs de ranking existentes (`anchor_ranking.rs`, `stratified_samples.rs` seguem verdes).

## Não-Objetivos

- Recalibrar pesos do ranking sem fixture que comprove o ganho (restrição herdada).
- Precisão do `lexicon-suggest` (residual próprio, trilha separada).
- Mudar o shape/contrato do report do digest (`matchedTerms`/`anchors`/`reason`) — só a seleção/ordenação interna.

## Critérios de Aceitação

- **AC-1** — Pipeline build green
  Command: `cargo build`
- **AC-2** — Fixture do caso sialia reproduz o miss e passa após o fix (teste vermelho-antes/verde-depois no crate do scan)
  Command: `cargo test --workspace anchor_ranking`
- **AC-3** — Benchs de ranking existentes sem regressão (estratificação + seleção de âncoras)
  Command: `cargo test --workspace stratified_samples`

<!-- PLAN -->

## Arquivos

- Seleção/ranking de âncoras do scan (match-first + escada de tiers do redesenho agnóstico) — localizar o módulo exato é a primeira tarefa do PLAN.
- `apps/scan/tests/fixtures/` — fixture nova espelhando o padrão sialia (página custom em pasta de domínio, ex.: `financial/all-titles-view.tsx`, sem nome homônimo à entidade da query).
- `apps/scan/tests/anchor_ranking.rs` + `apps/scan/tests/stratified_samples.rs` — caso novo + guarda de não-regressão.

## Limites

IN: seleção/ordenação de âncoras, fixtures e benchs do crate de scan.
OUT: lexicon/pontes léxicas, contrato do report do digest, dashboard (o `WorkspaceFilesRanking` listado pelo self-scan é UI — não pertence ao escopo).