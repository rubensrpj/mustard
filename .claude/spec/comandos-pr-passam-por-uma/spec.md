---
id: spec.comandos-pr-passam-por-uma
---

# os comandos de PR passam por uma porta de provedor, com adaptador GitHub e esqueleto Azure

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

os comandos de PR passam por uma porta de provedor, com adaptador GitHub e esqueleto Azure.

Por que agora: o Mustard entrou em uso corporativo num repositório hospedado no Azure DevOps. A unidade anterior (#165) tornou o FATO do provedor correto — detectado do remoto —, mas nada ainda o CONSOME: toda operação de pull request (criar, editar o corpo, tirar de rascunho) continua indo direto para a CLI `gh`, que só fala GitHub. Metade dessas chamadas nem vive em código: vive na PROSA das portas `/mustard:pr` e `/mustard:git`, que manda o modelo rodar `rtk gh pr create` — um caminho que nenhum teste cobre e que quebra silenciosamente em qualquer provedor que não seja o GitHub.

A arquitetura para isso já existe em miniatura dentro do próprio projeto: `PrLookup` em `branch_state.rs` é uma porta (o trait de que os chamadores dependem) com adaptadores (o único lugar onde um provedor e sua CLI são nomeados), e um provedor sem adaptador responde `provider-unsupported` — honesto, nunca uma ausência fingida. Esta unidade estende esse mesmo desenho às operações de ESCRITA de PR.

## Usuários/Stakeholders

O operador em repositório corporativo (Azure DevOps, hoje na Suzano): ganha a arquitetura que torna o Azure implementável sem tocar em nenhum chamador. O operador em GitHub: nada muda no comportamento — as mesmas operações, agora atrás de um comando testável. O próprio pipeline do Mustard: as portas de prosa passam a chamar um comando com relatório JSON em vez de improvisar `gh` cru.

## Métrica de sucesso

Nenhum chamador — código ou prosa — nomeia `gh pr create/edit/ready` diretamente; o provedor é decidido num único lugar (`provider_for`, sobre o `resolve_provider` do #165); e implementar o Azure de verdade vira uma unidade que toca UM arquivo (o adaptador), zero chamadores.

## Não-Objetivos

- **Implementar o REST do Azure DevOps.** O esqueleto responde `provider-unsupported` honestamente; a implementação real é a unidade seguinte, com credencial de verdade para medir contra a API viva.
- **Migrar as LEITURAS existentes** (`gh pr view` de `review_prefetch`, `gh pr list` de `ProviderPrCli` em `branch_state.rs`). Mexer nos dois leitores aqui dobraria o raio da unidade; eles já são honestos sobre provedor não suportado.
- **GitLab e Bitbucket.** A porta os comporta; nenhum adaptador nasce aqui.

## Critérios de Aceitação

- AC-1 — quando o adaptador recebe uma ref completa do Azure e um nome curto do GitHub, então a porta responde o MESMO nome curto para os dois. Command: `cargo test -p mustard-rt --lib shared::pr_provider::tests::a_full_ref_and_a_short_name_answer_the_same_branch -- --exact` Expect: `[1-9][0-9]* passed`
- AC-2 — quando o Azure responde active/completed/abandoned/notSet, então a porta traduz para OPEN/MERGED/CLOSED/OPEN e carrega o mergeStatus verbatim. Command: `cargo test -p mustard-rt --lib shared::pr_provider::tests::azure_states_map_to_the_canonical_vocabulary -- --exact` Expect: `[1-9][0-9]* passed`
- AC-3 — quando o provedor em vigor não tem adaptador implementado, então toda operação responde provider-unsupported, nunca um sucesso fingido nem uma ausência medida. Command: `cargo test -p mustard-rt --lib shared::pr_provider::tests::a_provider_without_an_adapter_refuses_honestly -- --exact` Expect: `[1-9][0-9]* passed`
- AC-4 — quando pr-open roda, então o relatório nomeia o provedor em vigor e a URL veio do adaptador, não de um gh cru no comando. Command: `cargo test -p mustard-rt --lib commands::review::pr_publish::tests::pr_open_reports_through_the_port -- --exact` Expect: `[1-9][0-9]* passed`
- AC-5 — quando a prosa da porta de PR é lida, então nenhuma linha manda rodar gh pr create/edit/ready direto: o caminho é o comando da porta, e uma catraca em teste guarda a regra Command: `cargo test -p mustard-rt --test pr_prose_door -- --exact` Expect: `[1-9][0-9]* passed`
- AC-6 — quando a unidade termina, então o workspace inteiro compila. Command: `cargo build --workspace`

<!-- PLAN -->

## Arquivos

- `apps/rt/src/shared/pr_provider.rs` — NOVO: a porta `PrProvider`, os tipos normalizados, o adaptador GitHub, o esqueleto Azure e a fábrica `provider_for`
- `apps/rt/src/shared/mod.rs` — registra o módulo novo
- `apps/rt/src/commands/review/pr_publish.rs` — NOVO: os comandos `pr-open`, `pr-edit`, `pr-ready`
- `apps/rt/src/commands/review/cli.rs` — variantes de enum + braços de despacho dos três comandos
- `apps/rt/src/commands/review/mod.rs` — registro do módulo
- `apps/rt/tests/run_command_surface.rs` — a lista travada de nomes ganha os três
- `plugin/commands/pr.md` — a prosa troca `rtk gh pr create/edit` por `mustard-rt run pr-open/pr-edit`
- `plugin/commands/git.md` — troca `rtk gh pr ready` por `mustard-rt run pr-ready`
- `apps/rt/src/commands/review/review_prefetch.rs` — nota de cabeçalho: escrita de PR agora é a porta
- `apps/rt/src/commands/review/pr_door.rs` — idem

## Limites

IN: as operações de ESCRITA de pull request (criar, editar corpo, tirar de rascunho) e a prosa que as invoca; a escolha do adaptador pelo provedor em vigor.
OUT: as LEITURAS de PR já existentes (`gh pr view`/`gh pr list` de `review_prefetch` e `branch_state`); a implementação REST real do Azure; migração de instalações; qualquer provedor além dos dois nomeados.

## Definitions

- **porta (port)** — o trait de que os chamadores dependem; nenhum consumidor nomeia um provedor ou sua CLI — PrLookup em branch_state.rs é o molde já existente
- **adaptador** — o ÚNICO lugar onde um provedor e sua CLI/API são nomeados; um provedor sem adaptador responde Unknown(provider-unsupported), nunca Absent
- **provedor em vigor** — o que resolve_provider(root, declared) devolve: git.provider declarado vence, senão o remoto origin, senão github

## Decisions

- a porta normaliza refs: Azure devolve sourceRefName como ref completa (refs/heads/x), GitHub devolve headRefName curto (x); a porta fala SEMPRE o nome curto
  Reason: medido nos contratos reais das duas APIs (GitPullRequest do Azure DevOps REST vs gh pr view --json); sem normalizar, todo chamador reimplementa a conversão e erra em um dos lados
- estados mapeados na porta: active→OPEN, completed→MERGED, abandoned→CLOSED, notSet→OPEN; o vocabulário canônico é o de PrStatus já existente
  Reason: o classificador de branch_state já reduz sobre MERGED/OPEN/CLOSED; o Azure tem 4 estados e o GitHub 3, e a porta é quem absorve a diferença
- mergeStatus do Azure viaja verbatim num campo próprio, sem mapeamento
  Reason: Azure tem 6 valores (conflicts, queued, rejectedByPolicy…) contra 3 do GitHub; inventar um vocabulário comum aqui seria perder informação que só o Azure dá
- diff e lista de arquivos vêm do git local, nunca do provedor
  Reason: o git já tem os commits após o fetch; a resposta é idêntica nos dois provedores e não consome API
- adaptador GitHub = mover as chamadas gh existentes (gh_out/gh_json de pr_door, o gh pr view de review_prefetch) para trás da porta; Azure nasce esqueleto honesto que responde provider-unsupported até ser implementado
  Reason: a unidade entrega a ARQUITETURA certa com o GitHub funcionando igual a hoje; implementar o REST do Azure é unidade seguinte, com credencial de verdade para medir
- pr create/edit/ready saem da prosa (plugin/commands/pr.md e git.md) para comandos mustard-rt run, que internamente escolhem o adaptador pelo provedor em vigor
  Reason: a prosa hoje manda o modelo rodar rtk gh pr create direto — github fixo em texto que nenhum teste cobre; um comando Rust é testável e o provedor vira detalhe interno

## Evidence

- a porta PrLookup existe e o classificador depende só dela
  Evidence: `apps/rt/src/shared/branch_state.rs:454`
- ProviderPrCli recusa qualquer provedor não-github com Unknown(PR_UNSUPPORTED) — honesto, mas Azure nunca funciona
  Evidence: `apps/rt/src/shared/branch_state.rs:581`
- PR_UNSUPPORTED é o vocabulário do 'não medido' já estabelecido
  Evidence: `apps/rt/src/shared/branch_state.rs:405`
- gh_out/gh_json são os helpers github-fixos que pr_door expõe e outros módulos importam
  Evidence: `apps/rt/src/commands/review/pr_door.rs:97`
- review_prefetch chama gh pr view direto, com o desvio cmd /C para Windows
  Evidence: `apps/rt/src/commands/review/review_prefetch.rs:63`
- a prosa manda rodar rtk gh pr create --base ... --body-file ... (github fixo em texto)
  Evidence: `plugin/commands/pr.md:59`
- a prosa manda rodar rtk gh pr edit <n> --body-file na atualização de corpo
  Evidence: `plugin/commands/pr.md:43`
- a prosa manda rodar rtk gh pr ready no finish do fluxo git
  Evidence: `plugin/commands/git.md:49`
- resolve_provider(root, declared) é o único lugar onde a precedência declarado→remoto→github está escrita
  Evidence: `packages/core/src/platform/git_provider.rs:127`
- o hook pr_detect só TOKENIZA comandos gh pr do bash do operador — é detecção, não chamada; não entra na porta
  Evidence: `apps/rt/src/hooks/bash/pr_detect.rs:42`
## Concerns

- `plugin/refs/git/submodule-rules.md` ainda instrui `rtk gh pr create --fill` e `rtk gh pr ready` cru (achado da onda 3): o fluxo de submódulo usa `--fill` (título/corpo derivados dos commits), modo que `pr-open` não tem — converter exige uma decisão de desenho na porta (abertura sem corpo), unidade própria. A catraca cobre as duas portas principais.
