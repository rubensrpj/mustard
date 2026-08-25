---
id: spec.aviso-cobra-cerimonia-que-nao
---

# O aviso de enriquecimento manda abrir unidade propria em arvore limpa mesmo quando a saida da passada e invisivel ao git — caso em que nenhuma das tres exigencias se aplica

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

O aviso de enriquecimento aparece a cada abertura de unidade e sempre termina com a mesma frase: *"a passada de enriquecimento reescreve arquivos versionados, então é uma unidade de trabalho própria, em árvore limpa — despache-a quando a unidade atual fechar."*

**Numa instalação privada essa frase é falsa nas três exigências.** As Guards vão para `CLAUDE.local.md`, que a instalação pôs no `info/exclude` — o git nunca vê o arquivo. Sem arquivo versionado reescrito não há commit a manter separado; sem commit a manter separado não há necessidade de árvore limpa; e sem nada disso não há razão para abrir unidade. A passada pode rodar ali mesmo, na hora.

Medido nesta sessão: as Guards dos 8 subprojetos reais foram escritas rodando a passada inline, sem branch, sem commit, e a árvore ficou limpa o tempo todo — exatamente o que o aviso dizia ser impossível.

O efeito prático é o pior tipo de aviso: ele reaparece para sempre, pedindo ao operador que **lembre** de agendar algo que não precisa ser agendado.

## Usuários/Stakeholders

Quem trabalha num repositório com instalação privada — hoje leva a mesma cobrança a cada abertura de unidade, com uma instrução que não se aplica ao caso dele.

## Métrica de sucesso

Numa instalação cuja saída de enriquecimento é invisível ao git, o aviso deixa de pedir unidade própria e passa a dizer que a passada pode rodar agora. Onde a saída é versionada, o texto não muda.

## Não-Objetivos

- **Silenciar o aviso.** A lacuna é real nos dois modos: sem as Guards, todo agente que edita aquele subprojeto trabalha sem elas. Calar trocaria uma cobrança errada por uma omissão.
- **Fazer o portão rodar o enriquecimento.** Ele é um processo Rust; Guards e molds são escritos por agente.
- **Mudar a contagem da lacuna.** Já é o que a unidade anterior conserta.

## Critérios de Aceitação

AC = critério de aceitação: uma frase verificável por um comando.

- **AC-1** — when a saída do enriquecimento é invisível ao git, then a linha diz que a passada pode rodar agora e NÃO pede unidade própria nem árvore limpa
  Command: `cargo test -p mustard-rt a_hidden_enrichment_asks_for_no_ceremony 2>&1 | grep -E "[1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt names_a_subproject_whose_guards_are_still_a_scaffold 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-2** — when a saída é versionada, then a linha continua pedindo unidade própria em árvore limpa, palavra por palavra
  Command: `cargo test -p mustard-rt a_versioned_enrichment_still_asks_for_its_own_unit 2>&1 | grep -E "[1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt names_a_subproject_whose_guards_are_still_a_scaffold 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-3** — o build do workspace passa verde
  Command: `cargo build --workspace`

<!-- PLAN -->

## Arquivos

| arquivo | o que muda |
|---|---|
| `apps/rt/src/commands/event/enrichment_gap.rs` | a prescrição da linha passa a depender de a saída ser versionada; testes AC-1/2 |
| `apps/rt/src/commands/scan_patterns/sweep.rs` | cascata: enumeração somente-leitura dos molds que a varredura apagaria — parte do conjunto de escrita |
| `apps/rt/src/commands/event/base_gate.rs` | cascata: `path_is_ignored` vira compartilhado; import morto removido |
| `.claude/mustard/orchestrator.md` + `packages/core/templates/mustard/orchestrator.md` | a prosa do roteador manda LER a prescrição da linha em vez de supor uma |
| `packages/core/src/platform/project_seed.rs` | cascata: impressão digital da versão superada do template |

A medida é o mesmo fato por caminho que o portão de base já usa: uma regra de ignore esconde o alvo do enriquecimento, ou não. Não o modo de instalação — a unidade anterior já trocou esse predicado grosso justamente porque a premissa dele é falsa neste repositório.

## Limites

IN: o texto da linha e a medida que o escolhe.
OUT: tudo em `## Não-Objetivos`; a contagem da lacuna; qualquer mudança nas varreduras.

## Definitions

- **enriquecimento** — a metade do scan que um agente escreve: as Guards de cada subprojeto e os molds de papel. A outra metade, o modelo deterministico, e um processo Rust.
- **instalacao privada** — modo em que o rastro do mustard e escondido do git do repositorio hospedeiro por regras no `info/exclude`; as Guards vao para `CLAUDE.local.md`, que o git nao ve.

## Decisions

- Nao silenciar o aviso: mudar o que ele PEDE.
  Reason: A lacuna e real nos dois modos — sem as Guards, todo agente que edita aquele subprojeto trabalha sem elas. Calar seria trocar uma cobranca errada por uma omissao.
- A prescricao passa a depender de a saida ser versionada ou nao, medida por caminho.
  Reason: A cerimonia (unidade propria, arvore limpa, commit) existe porque a passada reescreve arquivo versionado. Onde ela nao reescreve, cada exigencia dessas e falsa, e pedir que o operador agende algo que nao precisa ser agendado e a forma mais cara de nao fazer.
- Reusar o mesmo fato por caminho que o portao de base ja usa.
  Reason: A unidade anterior ja trocou o predicado grosso de modo de instalacao pelo fato por arquivo; uma terceira forma de perguntar a mesma coisa voltaria a criar duas verdades.

## Evidence

- A linha do aviso termina SEMPRE com a mesma prescricao — `the enrich pass rewrites versioned files, so it is a work unit of its OWN on a clean tree — dispatch it once the current unit closes` — sem consultar se a saida e mesmo versionada.
  Evidence: `apps/rt/src/commands/event/enrichment_gap.rs:156`
- Numa instalacao privada as Guards vao para `CLAUDE.local.md`, que esta no `info/exclude`: medido neste repositorio, `git check-ignore` confirma a regra e `git ls-files` nao devolve nada para o arquivo.
  Evidence: `packages/core/src/platform/project_seed.rs:388`
- As tres exigencias da frase caem juntas quando a saida e invisivel: nao ha arquivo versionado reescrito, logo nao ha commit a manter separado, logo nao ha necessidade de arvore limpa nem de unidade propria.
  Evidence: `apps/rt/src/commands/event/enrichment_gap.rs:36`
- Reproduzido em campo: as Guards dos 8 subprojetos reais foram escritas nesta sessao rodando a passada inline, sem branch e sem commit, e a arvore permaneceu limpa o tempo todo.
  Evidence: `apps/rt/src/commands/scan_guards/apply.rs:34`
- O modulo ja mede a lacuna pelos mesmos dois coletores da varredura, entao a contagem em si nao precisa mudar — so o que a frase pede a partir dela.
  Evidence: `apps/rt/src/commands/event/enrichment_gap.rs:104`
