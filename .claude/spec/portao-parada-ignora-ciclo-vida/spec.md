---
id: spec.portao-parada-ignora-ciclo-vida
---

# Portao de parada cobra QA numa spec ainda em PLAN, onde todo criterio e vermelho por construcao

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

O portão de parada roda no fim de cada turno do orquestrador e executa os critérios de aceitação da spec ativa. Se um falha, ele bloqueia a parada e devolve o critério como orientação do turno seguinte. A intenção é boa: fecha o laço de verificação sem gastar um turno humano por tentativa.

O defeito está em quando ele decide agir. A função de auto-restrição (`stop_gate.rs:156-171`) faz duas perguntas — existe o marcador `.approved-by-user`? há critério executável? — e nenhuma sobre o ponto do ciclo de vida em que a spec está.

Só que o marcador é cunhado na aprovação do **plano**, e não na liberação do EXECUTE. Entre um e outro existe uma janela, e dentro dela três coisas são verdadeiras ao mesmo tempo:

```
spec full · stage=Plan · marcador presente
  ├─ ac-negative-check já garantiu: TODO critério está vermelho
  │     (é esse vermelho que os qualificou a entrar no plano)
  ├─ stop_gate exige que fiquem verdes
  └─ scope_guard nega escrever o código que os deixaria verdes
```

Não é coincidência de configuração: é garantido por construção. Toda spec `full` atravessa essa janela. Neste repositório são 35 specs `full`, e as 35 carregam o marcador. Há dois contadores de bloqueio gravados em disco, cada um em 1.

O prejuízo é limitado — o portão tem teto próprio de 8 bloqueios consecutivos e depois solta —, mas são até 8 turnos gastos exigindo algo que outro portão do mesmo produto proíbe.

## Usuários/Stakeholders

Quem aprova um plano Full: hoje o turno seguinte é gasto num impasse, não no trabalho.

## Métrica de sucesso

Uma spec aprovada que ainda está em `stage: Plan` não gera nenhum bloqueio de parada. Verificável por teste, sem depender de observar um pipeline real.

E a cobertura atual do portão não muda: os 7 testes que já existem continuam verdes, incluindo o que prova que ele bloqueia por critério vermelho.

## Não-Objetivos

- Não mexe no `scope_guard`. Ele está certo, e é ele que define a janela.
- Não hasteia o sensor de estágio para `shared/`. Ficam duas cópias de duas linhas; a terceira é que justificaria hastear.
- Não cria variável de ambiente nem modo de configuração. O portão é incondicional por decisão da spec que o criou, e isto aqui é condição de ciclo de vida, não de política.
- Não fecha nem reabre a spec `close-the-qa-verification-loop`, que construiu este portão e tem as duas ondas completas.

## Critérios de Aceitação

- **AC-1** — Quando a spec ativa está aprovada mas o `meta.json` dela ainda diz `stage: Plan`, então o portão de parada libera a parada em silêncio em vez de bloquear pelo critério vermelho.
  Command: `grep -q 'fn stop_gate_releases_a_spec_still_in_plan' apps/rt/src/hooks/task/stop_gate.rs && cargo test -p mustard-rt stop_gate_releases_a_spec_still_in_plan 2>&1`
  Expect: `test result: ok\. 1 passed`
  Control: `cargo test -p mustard-rt stop_gate_is_inert_without_an_approved_spec 2>&1`
- **AC-2** — a bateria inteira do portão de parada passa verde, incluindo o teste que prova que ele ainda bloqueia por critério vermelho quando não há estágio a ler.
  Command: `cargo test -p mustard-rt stop_gate 2>&1`
  Expect: `test result: ok\.`

## Checklist

- [x] T1 — `resolve_gated_spec` passa a soltar a spec cujo `meta.json#stage` lê `Plan`; ausência do arquivo mantém a verificação.
- [x] T2 — teste novo `stop_gate_releases_a_spec_still_in_plan`, com o helper que semeia `meta.json`.

## Definitions

- **portao de parada** — o Check que roda no fim de cada turno do orquestrador, executa os criterios de aceitacao da spec ativa e bloqueia a parada quando um deles falha
- **janela de aprovacao** — o intervalo entre o marcador `.approved-by-user` ser cunhado (aprovacao do PLANO) e o evento `pipeline.status to=approved` ser emitido por `/spec` (liberacao do EXECUTE); dentro dela nenhum codigo de producao pode ser escrito

## Decisions

- o portao de parada solta em silencio a spec cujo `meta.json#stage` ainda e `Plan`
  Reason: nessa janela a prova negativa garante que TODO criterio esta vermelho (e esse vermelho e o que os qualificou a entrar no plano) e o `scope_guard` nega escrever o codigo que os tornaria verdes — e a unica janela do produto onde o portao nao tem estado do mundo em que passe
- so uma leitura POSITIVA de `stage: Plan` solta; `meta.json` ausente ou ilegivel mantem a verificacao como e hoje
  Reason: o helper de teste `seed_spec` (stop_gate.rs:255) nao escreve `meta.json`, entao os 7 testes atuais rodam sem estagio; soltar por ausencia de sinal os deixaria todos verdes por acidente e apagaria a cobertura do portao
- o sensor de estagio e escrito dentro do `stop_gate`, nao hasteado para `shared/`
  Reason: sao duas linhas de normalizacao e existiriam duas copias, nao tres; hastear obrigaria a mexer tambem no `scope_guard` e nos seus 9 testes, ampliando um conserto de um arquivo para tres sem ganho de comportamento — a duplicacao fica declarada aqui em vez de virar refatoracao carona

## Evidence

- `resolve_gated_spec` faz duas perguntas apenas — existe o marcador de aprovacao, e ha criterio executavel — e nunca le o estagio do ciclo de vida da spec
  Evidence: `apps/rt/src/hooks/task/stop_gate.rs:156`
- o marcador `.approved-by-user` e cunhado na aprovacao do PLANO, muito antes de EXECUTE ser liberado, e a sua mera existencia e tratada como o portao
  Evidence: `apps/rt/src/hooks/task/stop_gate.rs:161`
- medido, nao lido: o `scope_guard` respondeu `permissionDecision: deny` para editar `route.rs` com a spec full em Plan, e liberou nos dois controles (arquivo da propria spec; outra spec light)
  Evidence: `apps/rt/src/hooks/write/scope_guard.rs:188`
- o gate irmao ja normaliza o estagio com trim + comparacao sem caixa; e essa a forma a espelhar
  Evidence: `apps/rt/src/hooks/write/scope_guard.rs:107`
- o helper de teste do proprio portao nao escreve `meta.json`, so `spec.md` — por isso o sinal ausente precisa preservar o comportamento atual
  Evidence: `apps/rt/src/hooks/task/stop_gate.rs:255`
- o prejuizo e limitado: o portao tem teto proprio de 8 bloqueios consecutivos por spec e depois solta
  Evidence: `apps/rt/src/hooks/task/stop_gate.rs:80`