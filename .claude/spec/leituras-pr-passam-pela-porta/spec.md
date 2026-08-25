---
id: spec.leituras-pr-passam-pela-porta
---

# as leituras de PR passam pela porta: a evidencia do branch-state e o prefetch de revisao falam Azure tambem

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

as leituras de PR passam pela porta: a evidencia do branch-state e o prefetch de revisao falam Azure tambem.

Por que agora.

## Usuários/Stakeholders

Quem se beneficia.

## Métrica de sucesso

Métrica de sucesso.

## Não-Objetivos

O que fica de fora.

## Critérios de Aceitação

- **AC-1** — quando a REST do Azure responde as linhas de PR de um branch, então a evidência reduz certo: merged vence, heads congelados vêm de lastMergeSourceCommit, lista vazia é ausência medida.
  Command: `cargo test -p mustard-rt --lib shared::pr_azure::tests::azure_evidence_reduces_states_and_merged_heads -- --exact` Expect: `[1-9][0-9]* passed`
- **AC-2** — quando o provedor em vigor é azure, então ProviderPrCli pergunta ao adaptador (um remoto ilegível responde Unknown com o token azure-*, nunca provider-unsupported).
  Command: `cargo test -p mustard-rt --lib shared::branch_state::tests::an_azure_provider_is_asked_through_the_adapter -- --exact` Expect: `[1-9][0-9]* passed`
- **AC-3** — quando o prefetch compõe o documento a partir do PR + threads + reviewers do Azure, então o formato é o MESMO que o caminho GitHub entrega (title/body/base/head/comments/reviews).
  Command: `cargo test -p mustard-rt --lib commands::review::review_prefetch::tests::an_azure_document_is_composed_from_the_port -- --exact` Expect: `[1-9][0-9]* passed`
- **AC-4** — o build do workspace passa verde.
  Command: `cargo build --workspace`

## Checklist

- [ ] T1 — em pr_azure: evidence_rows (GET por sourceRefName, status all) + redução para PrEvidence (merged>open>closed; heads de lastMergeSourceCommit.commitId); fetchers de threads e reviewers — tudo sobre o transporte injetável existente.
- [ ] T2 — em branch_state: ProviderPrCli::evidence_of roteia azure para o adaptador; github inalterado; demais continuam provider-unsupported.
- [ ] T3 — em review_prefetch: rotear por provedor em vigor; caminho azure compõe o documento no MESMO formato do GitHub (comments das threads ativas, reviews dos reviewers com voto), contadores/files do git local; composição em função pura testada com fake.

## Definitions

- **evidência de PR** — o que PrLookup::evidence_of devolve para um branch: o status reduzido (merged>open>closed, vazio=ausência medida) e o conjunto de heads congelados dos merges — é o que git-settle usa para autorizar poda
- **documento de prefetch** — o JSON rico que review-prefetch entrega ao fluxo de revisão: title/body/author/base/head/contadores/files/comments/reviews — fonte única, o fluxo não re-busca

## Decisions

- ProviderPrCli::evidence_of ganha o caminho azure via pr_azure (searchCriteria.sourceRefName + status all; heads congelados de lastMergeSourceCommit.commitId dos completed); github continua no gh; o resto continua Unknown(provider-unsupported)
  Reason: é a leitura que autoriza PODA de branch no settle — sem ela, todo settle em repositório Azure fica eternamente não-medido
- review-prefetch roteia por provedor: github mantém o gh pr view; azure compõe o MESMO formato de documento a partir do GET do PR + threads (comentários) + reviewers (vereditos), com contadores/files vindos do git local
  Reason: o consumidor (o fluxo de revisão) não pode mudar de formato por provedor; a porta absorve a diferença — e diff/arquivos do git já era decisão da série
- os fetchers novos do Azure vivem em pr_azure sobre o MESMO transporte injetável; a redução e a composição são funções puras testadas com fake em tabela
  Reason: nenhum teste toca rede nem credencial real — o desenho que as duas unidades anteriores provaram

## Evidence

- review_prefetch chama gh pr view direto com GH_FIELDS fixo (github-only)
  Evidence: `apps/rt/src/commands/review/review_prefetch.rs:65`
- ProviderPrCli::evidence_of recusa qualquer provedor não-github com Unknown(PR_UNSUPPORTED)
  Evidence: `apps/rt/src/shared/branch_state.rs:581`
- o transporte injetável e o test_support com FakeTransport já existem em pr_azure
  Evidence: `apps/rt/src/shared/pr_azure.rs:258`
