---
id: spec.close-the-qa-verification-loop
---

# Fechar o laço de verificação: um gate no evento Stop que executa os critérios da spec e re-dispara até passarem

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

Sigla usada uma vez: **AC = Acceptance Criteria** (critério de aceite), o par *quando/então* com um
comando executável que a spec declara.

**1 — Hoje a volta é sua.** Quando o `/qa` roda os critérios de uma spec e um falha, o fluxo
devolve o critério que falhou e **para**, esperando você mandar corrigir e rodar de novo. Cada volta
dessas custa um turno seu. Verificação é o passo de maior retorno num harness, e o Mustard **tem** o
critério 100% objetivo — o comando do AC, com código de saída real — mas não colhe: ele para no
primeiro fracasso em vez de fechar o laço sozinho. O que muda: um gate no evento **Stop** (o momento
em que a sessão principal termina um turno) que executa os critérios da spec ativa e aprovada; se um
falha, bloqueia a parada e devolve o critério que falhou como orientação do próximo turno. Fica
assim: a spec dirige a execução até o critério valer, sem uma volta sua a cada tentativa.

**2 — Executa, não julga.** A plataforma tem o `/goal`, que também "trabalha até a condição passar"
— mas ele pede a um juiz de modelo (o Haiku) que **leia a conversa** e decida. Um juiz de modelo é
não-determinístico: a mesma spec pode passar num turno e falhar noutro pela leitura, o que contraria
a lei do Mustard de que o veredito é determinístico. O gate do H1 **executa** o comando do critério
e lê o código de saída real — o mesmo caminho que o `/qa` já percorre. Fica assim: o laço fecha por
fato (o teste passou), não por opinião.

**3 — Um critério, um parser.** O `/qa` já tem o leitor e o executor dos critérios
([parse_ac_items](apps/rt/src/commands/review/qa_run/mod.rs:108) e
[run_for_spec_with_options](apps/rt/src/commands/review/qa_run/mod.rs:404)), e o próprio comentário
do parser diz que o `analyze_validation` reusa esse **mesmo** parser "para não divergir". Se o gate
escrever o seu próprio leitor de AC, o repositório ganha um terceiro parser — o *drift* que ele já
pagou duas vezes para impedir — e o gate passaria a discordar do `/qa` que deveria fechar. O gate
chama o mesmo executor, incluindo os AC de capability
([gather_capability_acs](apps/rt/src/commands/review/qa_run/runner.rs:413)); um teste de paridade
prova que o veredito do gate e o do `qa-run` coincidem para a mesma spec. Fica assim: o gate e o
`/qa` nunca discordam.

**4 — A trava do laço é nossa, não uma promessa da plataforma.** A documentação diz que a plataforma
força a parada após **8 bloqueios seguidos** (o campo `stop_hook_active` sinaliza a repetição). Mas a
nossa própria verificação achou um *issue* aberto dizendo que essa proteção contra laço infinito
**pode não estar implementada de fato** — confiar só nela arriscaria um laço que nunca termina. O
gate carrega o **seu próprio** contador de bloqueios *consecutivos* por-spec (um marcador em disco,
que **zera quando os critérios passam**) e honra `stop_hook_active` como sinal secundário; ao atingir
o teto, libera a parada. Fica assim: o laço é seguro mesmo que o teto da plataforma não dispare — o
Mustard não terceiriza a própria segurança.

**5 — O cinto só aperta quando há o que verificar.** O evento Stop dispara em **todo** fim de turno,
sem filtro (a plataforma não oferece *matcher* para ele). Um gate que bloqueasse qualquer parada
travaria o uso normal — conversas sem spec, sessões de exploração, respostas simples. O gate se
**auto-restringe**: só age quando há spec **ativa e aprovada** com critério executável, e nunca num
*stop* de subagente; em qualquer outro caso, libera em silêncio. Fica assim: o cinto só aperta
diante de uma spec aprovada para verificar; fora disso, é invisível.

**6 — O texto fala a língua do projeto.** O que volta ao Claude (e aparece a você) é texto ao
usuário. Prosa embarcada no código do gate quebraria o agnosticismo de língua — o Mustard fala pt-BR
ou en-US conforme o projeto. O texto do `reason` sai do catálogo
[i18n](packages/core/src/platform/i18n.rs:218), com chaves `stopgate.*`, traduzido para a língua do
projeto; o teto de turnos é constante documentada, **não** um novo botão de configuração.

**Por que agora.** É o item nº 1 do plano — o que faz a spec **dirigir** a execução em vez de só
relatar, fechando o laço de verificação que hoje depende de uma volta humana a cada tentativa.

## Usuários/Stakeholders

Quem roda uma spec até o fim: para de gastar um turno a cada critério que falha — o harness corrige e
re-verifica sozinho, e só devolve a você quando passa (ou quando bate o teto de segurança). E quem
mantém o harness: o veredito do gate é o mesmo do `/qa`, por reusar o mesmo executor.

## Métrica de sucesso

Numa spec com um critério que falha e depois é corrigido, a sessão **não** volta a você entre a falha
e a correção: o gate bloqueia a parada, devolve o critério que falhou, e libera assim que todos
passam — sem nunca ultrapassar o teto próprio de bloqueios consecutivos.

## Não-Objetivos

- **Trocar o `/goal` ou virar um juiz.** O gate executa o comando do critério; não lê a conversa nem
  pede parecer a um modelo. Determinístico é o ponto.
- **Substituir o Fix Loop de review** do `resume-loop` §B (que trata **achados de review**, com teto
  próprio). Este gate fecha o laço de **QA** (os critérios executáveis), não o de review.
- **Um segundo parser de AC.** O gate reusa `parse_ac_items` / `run_for_spec_with_options`; nunca um
  leitor paralelo — é o *drift* que o repositório já pagou duas vezes para impedir.
- **Um novo botão de configuração.** O teto de bloqueios é constante documentada no código, não um
  `MUSTARD_*_MODE` novo.
- **Tocar o `SubagentStop`.** Só o Stop da sessão principal; o *stop* de subagente é ignorado.

## Critérios de Aceitação

- **AC-1** — when o gate roda numa spec ativa e aprovada cujos critérios **todos passam**, then ele
  libera a parada, sem bloqueio
  Command: `cargo test -p mustard-rt stop_gate_allows_when_all_acs_pass`
  Expect: `[1-9][0-9]* passed`
- **AC-2** — when o gate roda numa spec ativa e aprovada com um critério que **falha**, then ele
  emite a decisão de bloqueio (`decision:block`) com o critério que falhou dentro do `reason`
  Command: `cargo test -p mustard-rt stop_gate_blocks_and_names_the_failing_ac`
  Expect: `[1-9][0-9]* passed`
- **AC-3** — when **não** há spec ativa e aprovada (nenhuma, ou ainda não aprovada, ou sem critério
  executável), then o gate libera a parada em silêncio
  Command: `cargo test -p mustard-rt stop_gate_is_inert_without_an_approved_spec`
  Expect: `[1-9][0-9]* passed`
- **AC-4** — when a entrada marca um *stop* de subagente, then o gate nunca bloqueia, porque só a
  sessão principal é verificada
  Command: `cargo test -p mustard-rt stop_gate_ignores_subagent_stops`
  Expect: `[1-9][0-9]* passed`
- **AC-5** — when o gate já bloqueou o próprio teto de vezes **consecutivas** para esta spec, ou
  `stop_hook_active` é verdadeiro, then ele libera a parada — nunca um laço infinito
  Command: `cargo test -p mustard-rt stop_gate_own_counter_caps_the_loop`
  Expect: `[1-9][0-9]* passed`
- **AC-6** — when o gate e o `qa-run` avaliam a **mesma** spec, then o veredito coincide, porque o
  gate reusa o parser e o executor do `qa-run`, não um segundo leitor
  Command: `cargo test -p mustard-rt stop_gate_verdict_matches_qa_run`
  Expect: `[1-9][0-9]* passed`
- **AC-7** — when o gate compõe o texto do bloqueio, then ele vem do catálogo i18n (chaves
  `stopgate.*`), na língua do projeto — nenhuma prosa embarcada no código do gate
  Command: `cargo test -p mustard-rt stop_gate_reason_comes_from_i18n`
  Expect: `[1-9][0-9]* passed`

<!-- PLAN -->

## Arquivos

- `apps/rt/src/hooks/task/stop_gate.rs` (novo) — o Check de Stop — auto-restrição (spec
  ativa+aprovada+AC executável, não-subagente), execução via reuso do `qa-run`, contador próprio,
  veredito de bloqueio com o critério que falhou
- `apps/rt/src/registry.rs` — registra o Check de Stop no módulo do trigger `Stop` (hoje só há o
  `session_stop_observer`, sem `check`)
- `apps/rt/src/dispatch.rs` — já roda `check` para qualquer trigger; carrega o `reason` do veredito
  de Stop até a emissão
- `apps/rt/src/main.rs` — `emit_outcome` passa a emitir a forma `{"decision":"block","reason":…}`
  (exit 0) no evento `Stop`, ao lado da forma `permissionDecision` que já emite no `PreToolUse`
- `apps/rt/src/commands/review/qa_run/mod.rs` — expõe a costura de reuso (o veredito + o primeiro
  critério que falhou) sem um segundo parser
- `apps/rt/src/shared/context.rs` — o caminho do marcador do contador de bloqueios por-spec, ao lado
  dos outros compositores de caminho por-spec (`approval_marker_path`/`clarified_marker_path`)
- `packages/core/src/platform/i18n.rs` — as chaves `stopgate.*` do texto ao usuário
- `packages/core/src/domain/model/contract.rs` — se o `Verdict`/`Outcome` precisar carregar o
  `reason` (a costura veredito → texto de bloqueio no Stop)

## Limites

IN: o Check de Stop e sua fiação (registry, dispatch, `emit_outcome`); o reuso do executor do
`qa-run`; o contador próprio de bloqueios consecutivos + a honra a `stop_hook_active`; as chaves
`stopgate.*` no i18n.
OUT: o `/qa`, o `/goal`, o Fix Loop de review (`resume-loop` §B); um segundo parser de AC; um novo
`MUSTARD_*_MODE`; o `SubagentStop`.

## Checklist

- [x] T1 — o Check de Stop em `stop_gate.rs`: auto-restrição, execução via `run_for_spec_with_options`,
      veredito de bloqueio nomeando o critério que falhou.
- [x] T2 — o contador próprio por-spec (marcador em `context.rs`), que zera quando os critérios
      passam, mais a honra a `stop_hook_active`; ao teto, libera.
- [ ] T3 — a fiação: `registry` (check no módulo Stop) + `dispatch`/`emit_outcome` emitindo
      `decision:block`+`reason` no evento Stop.
- [ ] T4 — as chaves `stopgate.*` no i18n; nenhuma prosa embarcada no código do gate.
- [ ] T5 — testes dos 7 comportamentos, incluindo a paridade com o `qa-run` e o teto do contador.