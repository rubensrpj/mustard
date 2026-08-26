---
id: wave.aprovacao-moldes-padrao.plan
---

# Wave Plan

## Wave Table

| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave.aprovacao-moldes-padrao.1-approval]] | approval | — | A fila que resolve QUAL spec a porta de aprovacao decide passa a parar no primeiro degrau que satisfaz o fato 1, o recuo passa a falar, e /mustard:spec r seco dentro do branch da unidade vira gesto de aprovacao. |
| 2 | [[wave.aprovacao-moldes-padrao.2-molds]] | molds | — | A instrucao entregue ao autor de moldes passa a mostrar paths na forma YAML exata que o molde deve carregar, o validador passa a conferir os quatro titulos e a tolerar as duas formas de paths, e o relay para de responder ok:true para um arquivo que leu e nao entendeu. |
| 3 | [[wave.aprovacao-moldes-padrao.3-prose]] | prose | [[wave.aprovacao-moldes-padrao.1-approval]] | A prosa e a mensagem de recusa param de prometer um caminho que nao existe: nomeiam os gestos que realmente cunham o marcador, inclusive o r seco, e a apresentacao do plano diz de saida qual gesto conta. |

## Acceptance Criteria
- AC-1 — quando o palpite mais recente de .pipeline-states/ nomeia uma spec que NAO esta na janela full+Plan e existe um unico plano full+Plan sem aprovar, a resolucao entrega o plano pendente e a aprovacao e cunhada. Command: `cargo test -p mustard-rt a_stale_hint_never_shadows_the_pending_full_plan 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-2 — quando o operador seleciona uma opcao de aprovacao oferecida e o fato 1 e que recusa, o observador nomeia em stderr qual condicao falhou, em vez de sair calado. Command: `cargo test -p mustard-rt a_fact_one_decline_names_its_reason 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-3 — quando o prompt inteiro e /mustard:spec r e o checkout E o branch da unidade cujo plano full esta em Plan sem aprovar, o marcador .approved-by-user e cunhado; fora desse branch, nada e cunhado. Command: `cargo test -p mustard-rt a_bare_r_inside_the_units_branch_mints_the_marker 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-4 — o worklist entregue ao autor mostra paths como o bloco YAML que o molde deve carregar, e o valor copiado ao pe da letra dele passa no validador. Command: `cargo test -p mustard-rt the_worklist_prints_paths_as_the_yaml_the_mold_must_carry 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-5 — um molde que declara paths na forma inline e aceito, e o arquivo gravado carrega paths em lista em bloco. Command: `cargo test -p mustard-rt an_inline_paths_value_is_accepted_and_written_as_a_list 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-6 — um molde cujos quatro titulos faltem, dupliquem ou estejam fora de ordem e recusado sem ser escrito, e um molde com os quatro na ordem certa e aceito. Command: `cargo test -p mustard-rt a_mold_whose_headings_are_wrong_is_refused 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-7 — um arquivo de envelope que foi lido, nao e JSON e nao demarca nenhum bloco volta ok:false nomeando o arquivo, nunca ok:true blocks:0. Command: `cargo test -p mustard-rt a_read_file_that_demarcates_nothing_is_never_a_silent_ok 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-8 — resume-loop secao A deixa de prometer que a resposta ao modal cunha o marcador sem dizer o que conta, e commands/spec.md registra a forma /mustard:spec r. Command: `! grep -q 'the answer mints the same marker' plugin/refs/spec/resume-loop.md && grep -q '/mustard:spec r' plugin/commands/spec.md`
- AC-9 — a recusa do approve-spec por marcador ausente nomeia os tres gestos que cunham, inclusive a forma digitada. Command: `cargo test -p mustard-rt the_refusal_names_the_gestures_that_actually_mint 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-10 — a arvore compila inteira depois das tres ondas. Command: `cargo build --workspace`
