---
id: spec.every-finding-must-have-declared
---

# todo achado precisa de destino declarado antes de fechar a spec

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

**O que acontece hoje.** Numa unidade real do projeto sialia, o portão de provas rodou e acertou: marcou o critério `! rg -q "hasOtherTypes" apps/sialia-app` como `survived` — ou seja, o critério continuou passando mesmo com o trabalho arrancado da árvore, então ele não verifica o que diz verificar. O motivo foi escrito por extenso no ledger. Outros quatro critérios, entre as duas unidades daquele dia, ficaram `evidence-removed`: a prova de remoção declinou julgá-los e explicou por quê. Cinco descobertas corretas, produzidas pela própria máquina. Nenhuma teve consequência. A mesma coisa acontece do lado humano: o revisor grava seus achados em `review/findings.md`, e o único leitor desse arquivo é o renderizador de retentativa — que só roda quando o veredito é `rejected`. Revisão **aprovada com ressalvas** não tem consumidor nenhum.

**Por que isso é problema.** O caro já foi pago. Alguém — pessoa ou máquina — investigou, entendeu e escreveu a causa. Deixar isso parado joga fora precisamente o trabalho mais caro do ciclo, e garante que o mesmo defeito volte pela porta da frente: como relato de usuário, um ciclo inteiro depois, custando uma investigação nova para chegar ao diagnóstico que já estava em disco. Pior: como o portão denuncia sem custo, denunciar deixa de significar alguma coisa.

**O que muda.** O achado ganha o mesmo tratamento que o item de checklist já tem: ou ele tem destino declarado, ou ele está aberto — e um portão no fecho recusa enquanto houver aberto. O destino é uma das quatro coisas: virou critério, virou pedido de mudança, virou trabalho enfileirado, ou foi descartado **com motivo**. Descartar é uma decisão legítima; descartar em silêncio não é.

**Como termina.** Os dois arquivos de achado deixam de ser cemitério e viram fila. O portão que já sabe denunciar passa a ter consequência.

```
HOJE                                    DEPOIS
findings.md ─┐                          findings.md ─┐
             ├─→ (ninguém lê)                        ├─→ coletor → meta.json#findings
ac-proof.json┘                          ac-proof.json┘              │
                                                                    ▼
fecho: dívida → checklist → QA → build  fecho: dívida → checklist → ACHADOS → QA → build
                                                                    │
                                                        recusa enquanto houver aberto
```

## Usuários/Stakeholders

Quem opera o harness: a pessoa que aprova o plano e fecha a unidade. Hoje ela precisa lembrar de abrir dois arquivos que nada no fluxo manda abrir. Depois, o fecho não anda sem que ela decida — e a decisão fica registrada, inclusive quando a decisão é não fazer nada.

Em segundo lugar, quem lê a unidade depois: o motivo do descarte fica no sidecar, então "por que ninguém agiu nisso?" passa a ter resposta em vez de silêncio.

## Métrica de sucesso

1. Nenhuma spec fecha com achado sem destino declarado — o portão recusa, não avisa.
2. Todo veredito `survived` e `evidence-removed` do ledger de provas aparece como achado, sem ninguém retipar nada.
3. O motivo do descarte sobrevive no sidecar e é auditável depois do fecho.

## Não-Objetivos

- **Não** adicionar detector novo de critério fraco. O que existe já acerta — foi ele que produziu o achado que motivou esta unidade. Somar detector antes de dar saída ao cano é produzir mais prosa.
- **Não** reescrever o formato das specs nem trocar o modelo de ondas.
- **Não** afrouxar critério nenhum: a correção torna o fecho mais exigente, nunca menos.
- **Não** tratar a separação entre estado do harness e trabalho da pessoa (submódulo, árvore suja). É defeito real e fica para uma unidade própria, que precisa de um repositório com submódulo ativo para ser provada.

## Critérios de Aceitação

- AC-1 — quando um meta.json carrega achados, entao um achado sem destino e distinguido de um descartado com motivo, e um sidecar escrito antes do campo existir volta byte-identico. Command: `cargo test -p mustard-core finding_item` Expect: `[1-9][0-9]* passed` Control: `cargo test -p mustard-core checklist_round_trips_with_done_state`
- AC-2 — quando o coletor roda numa spec que tem findings do revisor E um ledger com criterio `survived`, entao os dois aparecem como achados abertos no meta.json, cada um com o motivo que sua fonte escreveu. Command: `cargo test -p mustard-rt finding_collect_reads_both_sources` Expect: `[1-9][0-9]* passed` Control: `cargo test -p mustard-rt run_close_gates_allows_when_everything_passes`
- AC-3 — quando o coletor roda de novo depois de um destino declarado, entao o destino sobrevive e o achado nao volta a ficar aberto. Command: `cargo test -p mustard-rt finding_collect_preserves_declared_route` Expect: `[1-9][0-9]* passed` Control: `cargo test -p mustard-rt run_close_gates_allows_when_everything_passes`
- AC-4 — quando sobra achado aberto, entao o fecho e recusado e a recusa nomeia o achado e o comando que o resolve. Command: `cargo test -p mustard-rt findings_gate_denies_open_finding` Expect: `[1-9][0-9]* passed` Control: `cargo test -p mustard-rt run_close_gates_denies_missing_qa_when_strict`
- AC-5 — quando todo achado tem destino declarado, inclusive um descartado COM motivo, entao o portao de achados deixa o fecho seguir. Command: `cargo test -p mustard-rt findings_gate_allows_when_every_finding_routed` Expect: `[1-9][0-9]* passed` Control: `cargo test -p mustard-rt run_close_gates_denies_missing_qa_when_strict`
- AC-6 — quando o operador declara o destino pela porta, entao o sidecar registra destino E motivo, e a porta recusa a declaracao sem motivo. Command: `cargo test -p mustard-rt mark_finding_records_route_and_refuses_without_reason` Expect: `[1-9][0-9]* passed` Control: `cargo test -p mustard-rt run_close_gates_denies_missing_qa_when_strict`
- **AC-8** — quando o fecho vem pelo caminho do dia a dia (close-orchestrate) e existe achado sem destino, entao o fecho e recusado ali tambem, e nao apenas no emit-phase --to CLOSE
  Command: `cargo test -p mustard-rt close_orchestrate_blocks_on_open_finding`
  Expect: `[1-9][0-9]* passed`
- AC-7 — o workspace continua verde. Command: `cargo test --workspace` Expect: `[1-9][0-9]* passed`

<!-- PLAN -->

## Arquivos

Onda 1 — o modelo:

- `packages/core/src/domain/spec/contract.rs` — nasce `FindingItem`, espelhando `ChecklistItem` que já vive aqui
- `packages/core/src/domain/meta.rs` — o campo `findings` entra ao lado de `checklist`, com o mesmo contrato aditivo

Cascata da onda 1 — acrescentar um campo ao `Meta` é aditivo no arquivo, mas quebra a compilação em todo lugar que constrói a struct por inteiro em vez de `..Meta::default()`. São cinco pontos, uma linha cada, e eles vivem em `apps/rt`, fora da fronteira da onda 1:

- `apps/rt/src/commands/wave/wave_scaffold.rs` — dois pontos: o meta do plano-pai e o de cada onda
- `apps/rt/src/commands/spec/spec_draft.rs` — o meta recém-desenhado
- `apps/rt/src/commands/spec/tactical_fix_create.rs` — o meta do tactical-fix
- `apps/rt/src/commands/spec/spec_scaffold.rs` — o construtor usado pelos testes

Onda 2 — o coletor:

- `apps/rt/src/commands/review/finding_collect.rs` — novo; lê as duas fontes e semeia o sidecar
- `apps/rt/src/commands/review/mod.rs` — registra o módulo
- `apps/rt/src/commands/review/cli.rs` — a variante no enum e o braço no dispatch
- `apps/rt/tests/run_command_surface.rs` — a lista trancada de comandos publicados

Cascata da onda 2 — dois arquivos fora da fronteira, ambos com aviso do portão:

- `apps/rt/src/commands/review/ac_negative_check.rs` — `resolve_spec_file` sobe para `pub(crate)`. O coletor lê o ledger que esse módulo escreve; um segundo localizador é exatamente como os dois passariam a apontar para specs diferentes com o mesmo nome
- `apps/rt/tests/template_parity.rs` — linha justificada na `RUNTIME_WHITELIST`, porque o quarto registro do comando (um chamador real) só chega na onda 3. A catraca reprova quando a justificativa vira redundante, o que força a remoção da linha

Onda 3 — a porta e o portão:

- `apps/rt/src/commands/spec/mark_finding.rs` — novo; a porta que declara o destino
- `apps/rt/src/commands/spec/cli.rs` — a variante e o braço da porta
- `apps/rt/src/commands/pipeline/close_gates.rs` — o sub-gate de achados entra entre o checklist e o QA

Onda 4 — o caminho do dia a dia:

- `apps/rt/src/commands/pipeline/close_orchestrate.rs` — o fecho documentado do `/pr` passa a atravessar `gate_close_for_spec` ANTES do finalize; a recusa é um portão reprovado como qualquer outro (overall fail, chained false, motivo no relatório)

Cascata da onda 4 — a prosa que enumera os portões desse fecho, e que passaria a mentir:

- `plugin/commands/pr.md` — a lista de portões ganha o quinto, e o aborto de checklist deixa de ser "só precondição"
- `plugin/pipeline-config.md` — a mesma lista, na página de configuração do pipeline

## Limites

IN: o modelo do achado no núcleo; o coletor determinístico das duas fontes que já existem em disco; a porta que declara o destino; o sub-gate de fecho que recusa enquanto houver achado aberto; a variável `MUSTARD_FINDINGS_GATE_MODE` seguindo a mesma cascata dos irmãos.

OUT: qualquer detector novo de critério fraco (o existente já acerta); mudança no formato da spec ou no modelo de ondas; separação entre estado do harness e trabalho da pessoa; tratamento de submódulo; superfície no painel para os achados — o registro nasce no sidecar, e quem quiser exibi-lo lê de lá.

## Definitions

- **achado** — uma descoberta verificada produzida DENTRO da unidade de trabalho, por duas fontes hoje sem consumidor: o revisor (`review/findings*.md`) e o proprio portao de provas dos criterios (`ac-proof.json`, colunas `Removal::Survived` e `Removal::EvidenceRemoved`).
- **destino** — o que se decidiu fazer com um achado: virou criterio de aceitacao, virou pedido de mudanca, virou trabalho enfileirado, ou foi descartado COM MOTIVO. Um achado sem destino nao e um achado registrado — e trabalho esquecido.
- **achado aberto** — achado que ainda nao tem destino declarado. E o predicado que o portao de fecho consulta, no mesmo espirito de `ChecklistItem::is_open()` — nunca um simples 'nao tratado', que contaria uma decisao deliberada como esquecimento.
- **survived** — veredito do ledger de provas para o criterio que continuou VERDE com o trabalho arrancado da arvore: ele verifica algo que o trabalho nao fez. E um achado sobre o criterio, produzido pela propria maquina, e hoje ninguem o le.
- **evidence-removed** — veredito do ledger para o criterio cuja propria evidencia a remocao levou junto, de modo que o vermelho estava decidido antes de rodar. Nao e falha do criterio: e uma LACUNA DE COBERTURA declarada, e hoje tambem ninguem a le.

## Decisions

- reusar o modelo de ChecklistItem em vez de inventar uma estrutura nova para o achado
  Reason: o checklist ja resolve exatamente este problema para tarefas: um item ou esta `done`, ou esta `dropped` com motivo, e `is_open()` decide se alguem ainda deve algo. O comentario em contract.rs:173 declara que essa e a predicate que um gate DEVE usar, porque um `!done` contaria uma decisao deliberada como trabalho esquecido — a mesma armadilha que um portao de achado encontraria no primeiro dia.
- o destino do achado vem ANTES de qualquer detector novo de criterio fraco
  Reason: o detector que recusaria criterio fraco JA EXISTE e JA ACERTOU em campo: no projeto sialia o criterio `! rg -q "hasOtherTypes" apps/sialia-app` foi marcado `survived` com o motivo correto, e outros quatro criterios ficaram `evidence-removed`. Cinco descobertas corretas, zero consequencia. Somar detector a um cano sem saida produz mais prosa — que e precisamente o defeito diagnosticado.
- as duas fontes de achado entram pelo MESMO portao
  Reason: hoje sao dois arquivos diferentes com o mesmo destino: nenhum. Um portao que lesse so o `findings.md` do revisor deixaria o ledger de provas exatamente como esta, e um que lesse so o ledger deixaria o revisor. O defeito e o cano sem saida, nao a fonte.
- o sub-gate novo e strict por padrao, com sua propria variavel MUSTARD_*_MODE
  Reason: close_gates.rs declara strict como o default dominante da familia de fecho, por design, e a excecao deliberada ao fail-open do resto do harness. Um portao de achado que nascesse advisory repetiria em outra forma o defeito que ele existe para fechar.
- o achado e semeado por um coletor deterministico, nunca digitado a mao pelo modelo
  Reason: as duas fontes ja estao em disco em formato que a maquina le. Pedir ao modelo que retipe achado em outro arquivo reintroduz a perda que o canal --material existe para remover: o que a mao nao retipa, some.

## Evidence

- review_result grava o findings.md do revisor e seu UNICO leitor e o renderizador de retry, que so roda quando o veredito e `rejected` — uma revisao APROVADA com achados nao tem consumidor nenhum
  Evidence: `apps/rt/src/commands/review/review_result.rs:89`
- nenhum sub-gate do fecho le achado: run_close_gates roda debt-marker, checklist, QA, composicao de QA e build/test, nesta ordem, e nenhum deles abre review/findings*.md nem ac-proof.json
  Evidence: `apps/rt/src/commands/pipeline/close_gates.rs:896`
- o ledger de provas registra Removal::Survived ('verifica algo que o trabalho nao fez') e Removal::EvidenceRemoved ('a remocao nao foi tomada'), cada um com o motivo por extenso, e nenhum consumidor no fecho le essas colunas
  Evidence: `apps/rt/src/commands/review/ac_negative_check.rs:230`
- o precedente do destino declarado ja existe e e explicito: ChecklistItem::is_open() e documentado como o predicado que um gate deve usar, porque um !done contaria uma decisao deliberada como trabalho esquecido
  Evidence: `packages/core/src/domain/spec/contract.rs:177`
- o campo dropped carrega o MOTIVO do descarte e e serde-compativel por omissao: sidecar historico sem o campo le como None e volta byte-identico — o mesmo contrato aditivo que o registro de achado precisa
  Evidence: `packages/core/src/domain/spec/contract.rs:129`
- o campo checklist em Meta e o precedente estrutural do lado do sidecar: aditivo, elide a chave quando vazio e preserva a forma em bytes de meta.json escritos antes de existir
  Evidence: `packages/core/src/domain/meta.rs:123`
- o conceito de destino de achado nao existe no codigo — provado por enumeracao: as ocorrencias de 'destination'/'disposition' sao todas caminho de arquivo (copia, rename, pid-file) ou disposicao terminal de spec, nenhuma sobre triagem de achado
  Evidence: `packages/core/src/domain/model/view/spec.rs:64`
- medido em campo no projeto sialia: na unidade vinculo-usuario-x-colaborador-exclusao o criterio AC-8 ficou verdict=unproven removal=survived com o motivo correto por extenso, e outros quatro criterios entre as duas unidades do dia ficaram evidence-removed — a maquina produziu cinco achados corretos e nenhum teve consequencia
  Evidence: `apps/rt/src/commands/review/ac_negative_check.rs:626`