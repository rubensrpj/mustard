---
id: spec.upsert-nao-termina-propria-instalacao
---

# O upsert entrega dois passos manuais ao operador — atualizar o plugin e recarregar — quando o primeiro e automatizavel e o segundo so precisa ser dito

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

A instalação termina entregando duas tarefas ao operador. O fim do `init` diz, literalmente: *digite `/plugin marketplace add` e `/plugin install` dentro do Claude Code, depois recarregue*. Na atualização é a mesma coisa por outro caminho — foi assim que isto virou unidade: *"sempre depois preciso ir em plugins e recarregar"*.

**As duas metades não são iguais, e tratá-las como uma só é o defeito.**

**Atualizar é automatizável.** O Claude Code tem CLI para isso: `claude plugin marketplace update <nome>` e `claude plugin update <plugin>`. Medido nesta máquina, `claude plugin list` roda de dentro de uma sessão, sem interação, exit 0 — o spawn é viável e não trava.

**Aplicar não é.** A ajuda do próprio comando declara `(restart required to apply)`. Uma sessão carrega o plugin no início e o segura até terminar; nenhum código dentro dela troca isso, porque quem carrega é o hospedeiro, não o plugin.

Hoje o `upsert` não faz **nem uma coisa nem outra**: não atualiza, e não avisa. A deriva é real neste instante — `installed_plugins.json` registra `0.1.42` enquanto a `main` já publicou `0.1.43`.

## Usuários/Stakeholders

Quem atualiza o mustard. Hoje leva dois passos manuais depois de uma instalação que se declarou concluída.

## Métrica de sucesso

Depois de `mustard-rt run upsert`, a versão instalada do plugin é a que o marketplace oferece, sem nenhum passo manual — e o relatório diz que a sessão em curso continua com a que carregou.

## Não-Objetivos

- **Automatizar o recarregamento.** Não é limitação de esforço: o hospedeiro exige reinício e nenhum plugin reinicia o hospedeiro. Prometer isso seria pior que o estado de hoje.
- **A primeira instalação pelo terminal** (`mustard init` → `print_next_steps`). Ali o plugin pode nem existir e o marketplace pode não estar adicionado; é outro fluxo, com outras recusas.
- **Instalar o plugin quando ele não está instalado.** Esta unidade atualiza o que já existe.

## Critérios de Aceitação

AC = critério de aceitação: uma frase verificável por um comando.

- **AC-1** — when o refresh do plugin roda com sucesso, then o relatório do upsert carrega a versão resultante E a frase de que a sessão em curso segue com a que carregou
  Command: `cargo test -p mustard-rt a_successful_refresh_names_the_version_and_the_restart 2>&1 | grep -E "[1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt ac6_upsert_is_private_unconditionally_and_offers_no_switch 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-2** — when o CLI do Claude Code está ausente ou recusa, then o upsert continua bem-sucedido e o relatório diz por que o refresh não rodou
  Command: `cargo test -p mustard-rt an_unavailable_cli_degrades_to_a_reported_skip 2>&1 | grep -E "[1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt ac6_upsert_is_private_unconditionally_and_offers_no_switch 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-3** — o build do workspace passa verde
  Command: `cargo build --workspace`

<!-- PLAN -->

## Arquivos

| arquivo | o que muda |
|---|---|
| `apps/rt/src/commands/maint/upsert.rs` | o refresh do plugin como último passo do upsert; campo novo no relatório; testes AC-1/2 |
| `plugin/commands/upsert.md` | a prosa da porta passa a relatar o refresh e o reinício |
| `packages/core/src/platform/harness.rs` + `lib.rs` | cascata: o caminho do registro de plugins deixa de ser copiado e passa a ser compartilhado |
| `apps/rt/tests/private_surface.rs` | cascata: a fixture fixa `CLAUDE_CONFIG_DIR` para o teste não mexer na instalação da máquina |

O refresh é dois spawns em sequência — o marketplace primeiro, o plugin depois — atrás de um seam que os testes dirigem sem precisar de um `claude` real. Fail-open em cada ponto: binário ausente, exit não-zero ou saída ilegível viram motivo reportado, nunca erro.

## Limites

IN: o caminho de atualização do `mustard-rt run upsert` e a prosa da sua porta.
OUT: tudo em `## Não-Objetivos`; `mustard init` e o `print_next_steps`.

## Decisão registrada durante a implementação

**O ajuste de tom foi pedido para esta unidade, implementado, e depois MOVIDO para uma unidade própria — por decisão do operador.** O registro completo está em `change-log.md` (entrada de 2026-08-22T08:26:13Z, que reverte as de 07:55:30Z e 08:12:55Z).

O motivo é uma restrição do próprio produto, descoberta na segunda revisão: a prova negativa é tudo-ou-nada — ela exige que TODOS os critérios da unidade falhem na mesma passada. Os três critérios desta unidade já estavam verdes, então nenhum critério novo podia ser provado aqui. Sem critério, a unidade passaria com o recurso apagado, que foi exatamente o que a revisão apontou.

Diante disso o operador escolheu, entre duas opções com o custo de cada uma na mesa, que o tom vira unidade própria — com critérios provados vermelhos antes de o código existir, e já nascendo com a entrega a cada mensagem que ele aprovou depois de perguntar o custo em tokens (126 por mensagem, ~0,04% de uma sessão longa).

Consequência medida: o teto do início de sessão volta de 9.327 para 8.594 de 10.000 caracteres.


**`claude plugin update` recebe `--yes`, e isso aceita algo em nome do operador.** Sem a marca o passo falha sempre: a saída aqui é um cano, não um terminal, e o programa se recusa a perguntar quando não tem a quem. O que ela aceita, dito por extenso: um marketplace pode declarar um comando de instalação próprio, e a pergunta seria o momento de alguém aprovar rodá-lo. A confiança já foi dada antes — o operador escolheu esse marketplace e instalou esse plugin a partir dele, e o alvo vem do registro **dele**, não de nós. O marketplace deste projeto não declara comando nenhum (verificado), então hoje a marca só responde uma pergunta que ninguém faria.

## Definitions

- **recarregar o plugin** — duas coisas distintas que a frase junta: ATUALIZAR (trocar a versao instalada) e APLICAR (a sessao passar a usa-la). A primeira e automatizavel; a segunda exige reinicio e nenhum plugin reinicia o hospedeiro.
- **marketplace** — o clone git de onde o Claude Code instala o plugin; aqui e `mustard-local`, que segue a `main`.

## Decisions

- Automatizar a ATUALIZACAO e ANUNCIAR o reinicio, em vez de tentar automatizar o recarregamento.
  Reason: A ajuda do proprio Claude Code declara `restart required to apply`. Uma sessao carrega o plugin no inicio e o segura ate terminar; nenhum codigo dentro dela troca isso, porque quem carrega e o hospedeiro. Prometer o que nao se pode cumprir seria pior que a situacao de hoje.
- Falhar aberto: `claude` ausente ou recusando nao derruba o upsert.
  Reason: O Guard do crate manda sondar ferramenta externa fail-open, e a instalacao do projeto nao depende do estado do plugin. Um refresh que nao pode rodar e reportado, nunca fatal.
- So o caminho de ATUALIZACAO (`mustard-rt run upsert`), nao a primeira instalacao pelo terminal.
  Reason: E a atualizacao que doi de forma recorrente — o operador descreveu 'sempre depois preciso ir em plugins e recarregar'. Na primeira instalacao o plugin pode nem existir e o marketplace pode nao estar adicionado; e outro fluxo, com outras recusas.

## Evidence

- O fim da instalacao entrega DOIS passos manuais ao operador: digitar `/plugin marketplace add` e `/plugin install` dentro do Claude Code, e depois recarregar.
  Evidence: `apps/cli/src/commands/init.rs:997`
- Existe CLI para a metade automatizavel: `claude plugin marketplace update <nome>` e `claude plugin update <plugin>`, ambos listados em `claude plugin --help`.
  Evidence: `plugin/commands/upsert.md:31`
- A ajuda de `claude plugin update` declara textualmente `(restart required to apply)` — a metade do aplicar nao e automatizavel por ninguem de dentro da sessao.
  Evidence: `plugin/commands/upsert.md:31`
- Medido nesta maquina: `claude plugin list` roda de dentro de uma sessao, sem interacao, exit 0 — entao o spawn e viavel e nao trava.
  Evidence: `apps/rt/src/commands/maint/upsert.rs:33`
- A deriva e real agora: `installed_plugins.json` registra 0.1.42 enquanto a main ja publicou 0.1.43 — exatamente o intervalo em que a sessao roda prosa velha.
  Evidence: `apps/rt/src/hooks/session/session_start_inject.rs:407`
- O `upsert` do lado rt e o ponto onde a porta de instalacao termina hoje, e e ele que a prosa do `/mustard:upsert` chama.
  Evidence: `apps/rt/src/commands/maint/upsert.rs:50`
