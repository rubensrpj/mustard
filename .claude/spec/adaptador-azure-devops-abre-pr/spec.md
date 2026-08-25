---
id: spec.adaptador-azure-devops-abre-pr
---

# o adaptador Azure DevOps abre PR, atualiza o corpo e tira de rascunho via REST com a credencial do git

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

o adaptador Azure DevOps abre PR, atualiza o corpo e tira de rascunho via REST com a credencial do git.

Por que agora.

## Usuários/Stakeholders

Quem se beneficia.

## Métrica de sucesso

Métrica de sucesso.

## Não-Objetivos

O que fica de fora.

## Critérios de Aceitação

- **AC-1** — quando open é chamado, então o adaptador faz POST em pullrequests com sourceRefName/targetRefName como refs completas, título do primeiro cabeçalho do corpo e isDraft honrado — medido num transporte fake, sem rede.
  Command: `cargo test -p mustard-rt --lib shared::pr_provider::tests::azure_open_posts_the_pullrequest_contract -- --exact` Expect: `[1-9][0-9]* passed`
- **AC-2** — quando nem AZURE_DEVOPS_EXT_PAT nem o cofre do git têm credencial, então a operação recusa nomeando as DUAS fontes, nunca um erro mudo.
  Command: `cargo test -p mustard-rt --lib shared::pr_provider::tests::azure_without_credential_refuses_naming_both_sources -- --exact` Expect: `[1-9][0-9]* passed`
- **AC-3** — quando a REST responde um GitPullRequest real, então view devolve PrView normalizado: estados traduzidos, refs curtas, mergeStatus verbatim.
  Command: `cargo test -p mustard-rt --lib shared::pr_provider::tests::an_azure_response_folds_into_the_normalized_view -- --exact` Expect: `[1-9][0-9]* passed`
- **AC-4** — quando o remoto é https, ssh v3 ou visualstudio.com legado, então a base da API e a URL do PR são derivadas certas das três grafias.
  Command: `cargo test -p mustard-rt --lib shared::pr_provider::tests::every_azure_remote_spelling_yields_the_rest_base -- --exact` Expect: `[1-9][0-9]* passed`
- **AC-5** — o build do workspace passa verde.
  Command: `cargo build --workspace`

## Checklist

- [ ] T1 — o transporte injetável (trait + impl ureq + fake de teste) e a credencial (env → git credential fill → recusa nomeando as duas).
- [ ] T2 — derivação da base REST e da URL de PR das três grafias de remoto (https, ssh v3, visualstudio.com).
- [ ] T3 — as quatro operações: open (POST), edit_body e ready (PATCH), view (GET normalizado; None = PR do branch atual via searchCriteria.sourceRefName).
- [ ] T4 — base64 encode-only local (~20 linhas) para o Basic auth, com teste de vetores.
- [ ] T5 — NENHUMA operação de merge: não-objetivo por alçada (na Suzano o operador abre e atualiza, não mergeia).

## Definitions

- **credencial do git** — o que `git credential fill` devolve para a URL do remoto — o mesmo cofre que autentica um git push; na máquina do operador, credential.https://dev.azure.com.useHttpPath=true guarda a credencial POR CAMINHO de repositório
- **transporte injetável** — o adaptador fala com a REST por um trait de transporte; o real usa ureq, os testes usam um fake em tabela que grava as requisições — nenhum teste toca rede, o mesmo desenho do FakePr de branch_state

## Decisions

- a credencial vem de AZURE_DEVOPS_EXT_PAT quando setada (sobrescrita explícita), senão de `git credential fill` com GIT_TERMINAL_PROMPT=0; sem nenhuma das duas, recusa nomeando as DUAS fontes
  Reason: o operador já autentica o git no dev.azure.com pelo credential helper — onde o push funciona, o adaptador funciona sem configuração nova; a env var é a convenção da CLI oficial az e serve de sobrescrita deliberada
- a URL da API e a URL do PR são DERIVADAS do remoto origin (https://dev.azure.com/{org}/{proj}/_git/{repo} e a forma ssh v3), nunca lidas da resposta
  Reason: o remoto é o fato local que já temos; depender de campos da resposta cria dois caminhos de verdade
- operações: open (POST pullrequests com sourceRefName/targetRefName completos, title do primeiro cabeçalho do corpo, isDraft), edit_body e ready (PATCH), view (GET normalizado); NENHUMA operação de merge/complete
  Reason: restrição de alçada real do operador na Suzano: pode abrir e atualizar PR, não pode mergear — e a porta PrProvider já não expõe merge, então a restrição é arquitetura, não configuração
- HTTP via ureq, já dependência do rt
  Reason: cliente bloqueante sem runtime, já no workspace — zero dependência nova; um curl subprocess seria um segundo caminho de erro sem ganho
- Basic auth com base64 próprio de ~20 linhas (encode-only) em vez de crate novo
  Reason: o workspace não tem crate base64 e o encode de ':PAT' é a única necessidade

## Evidence

- ureq já é dependência do rt, descrita como o cliente HTTP bloqueante do workspace
  Evidence: `apps/rt/Cargo.toml:54`
- AzurePrRest existe como esqueleto honesto respondendo provider-unsupported em toda operação
  Evidence: `apps/rt/src/shared/pr_provider.rs:353`
- o trait PrProvider tem exatamente open/edit_body/ready/view — merge não existe na porta
  Evidence: `apps/rt/src/shared/pr_provider.rs:170`
- provider_for já roteia PROVIDER_AZURE para AzurePrRest; nenhum chamador muda
  Evidence: `apps/rt/src/shared/pr_provider.rs:422`
- o contrato REST 7.1 foi conferido na doc oficial pela onda 1 da unidade anterior: status notSet/active/abandoned/completed, mergeStatus com seis valores, sourceRefName como ref completa
  Evidence: `apps/rt/src/shared/pr_provider.rs:352`

## Concerns

- Achado menor da revisão: `do_view_branch` não percent-encoda o branch na query `searchCriteria`; um branch com `+` ou `%` responderia um falso `no-pr-for-branch`. Não bloqueia (nomes assim são raríssimos e o fluxo gera slugs ascii); corrigir na próxima passada do módulo.
