---
id: spec.base-do-branch-escolhida-numa
---

# a base do branch é escolhida numa lista real do git após fetch, o tipo do branch vira vocabulário aberto e só o branch padrão do remoto fica protegido contra commit e merge diretos

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

Hoje, para abrir qualquer unidade de trabalho, o Mustard faz duas perguntas: de qual base sair e que tipo de branch é. As duas respostas vêm de uma lista que o `mustard init` gravou em `mustard.json#git.flow` no dia da instalação — normalmente `dev` e `main`. Quem trabalha em um repositório só, com convenção própria, nunca sente o limite.

Em uso corporativo ele aparece no primeiro dia. Cada cliente nomeia seus branches do seu jeito, times criam `release/2026-Q3` e `integration/squad-b` durante a semana, e a lista gravada na instalação envelhece imediatamente. O resultado é o pior possível: o portão recusa a abertura da unidade **dizendo que um branch que existe de verdade não é uma base**, e a saída oferecida é editar um arquivo de configuração.

A causa é que o código tem UMA lista fazendo DOIS trabalhos incompatíveis. `integration_bases()` responde ao mesmo tempo "de onde posso cortar?" e "onde é proibido commitar?". Enquanto as duas respostas eram a mesma, a lista precisava ser fechada. Separar as duas perguntas é o que permite a primeira ser aberta — qualquer branch real serve como ponto de partida — sem afrouxar a segunda, que continua fechada e agora com um único membro: o branch padrão do remoto.

Por que agora: o Mustard passou a ser usado em repositórios de cliente, onde o operador não é dono da convenção de branches e não pode reescrever a configuração para cada projeto que abre.

## Usuários/Stakeholders

Quem opera o Mustard dentro de repositórios que não controla — consultoria, squad alocado, manutenção de sistema de terceiro. É quem paga hoje o custo de traduzir a convenção de cada cliente para a lista de duas entradas que a instalação gravou.

Secundariamente, todo projeto que use mais de dois branches de integração: hoje eles não cabem no modelo.

## Métrica de sucesso

Abrir uma unidade a partir de qualquer branch que exista no remoto, sem editar nenhum arquivo de configuração — incluindo um branch criado depois da instalação do Mustard.

A contraprova, igualmente necessária: commit e merge diretos no branch padrão do remoto continuam recusados.

## Não-Objetivos

- **Convenção de nome com identificador de ticket** (`feature/PROJ-123-descricao`). Não foi pedida. Se a empresa exigir, entra como unidade própria.
- **Migração de instalações existentes.** `git.flow` para de ser perguntado e para de restringir, mas continua sendo lido quando existe: nenhum projeto instalado precisa de um passo de atualização.
- **Proteger mais de um branch por padrão.** Times que também protegem `develop` ou `release/*` declaram isso numa lista opcional — o padrão continua sendo um branch só.
- **A pergunta do tipo desaparecer.** Ela permanece; o que muda é que a resposta deixa de ser um conjunto de três e deixa de decidir a base.

## Critérios de Aceitação

- **AC-1** — when a unidade é aberta a partir de um branch que existe no remoto mas não está declarado em `git.flow`, then o portão de base aceita a abertura em vez de recusar.
  Command: `cargo test -p mustard-rt --lib commands::event::base_gate::tests::accepts_any_real_branch_as_base -- --exact 2>&1 | grep -q "test result: ok. 1 passed"`
  Control: `cargo test -p mustard-rt --lib base_gate 2>&1 | grep -q "test result: ok"`
- **AC-2** — when um branch que NÃO é o padrão do remoto recebe uma escrita direta, then a proteção permite; e quando o branch padrão recebe a mesma escrita, then ela é recusada.
  Command: `cargo test -p mustard-rt --lib commands::event::work_branch::tests::only_the_remote_default_branch_is_protected -- --exact 2>&1 | grep -q "test result: ok. 1 passed"`
  Control: `cargo test -p mustard-rt --lib work_branch 2>&1 | grep -q "test result: ok"`
- **AC-3** — when o operador informa um tipo fora da lista sugerida, por exemplo `chore`, then o nome do branch é montado com esse prefixo em vez de ser recusado ou coagido para `feature`.
  Command: `cargo test -p mustard-rt --lib shared::work_kind::tests::accepts_a_type_outside_the_suggested_list -- --exact 2>&1 | grep -q "test result: ok. 1 passed"`
  Control: `cargo test -p mustard-rt --lib work_kind 2>&1 | grep -q "test result: ok"`
- **AC-4** — when o fluxo precisa das bases candidatas, then `run base-candidates` busca o remoto e devolve os branches reais ordenados por recência do último commit.
  Command: `cargo run -p mustard-rt --quiet -- run base-candidates 2>&1 | grep -q '"branches"'`
  Control: `cargo run -p mustard-rt --quiet -- run --help 2>&1 | grep -q "emit-pipeline"`
- **AC-5** — when o `mustard init` roda em um repositório, then ele não pergunta mais qual é o branch de produção nem qual é o de desenvolvimento, e o `mustard.json` que ele grava não contém a chave `git.flow`.
  Command: `cargo test -p mustard-cli --lib commands::git_flow::tests::init_does_not_ask_for_branches -- --exact 2>&1 | grep -q "test result: ok. 1 passed"`
  Control: `cargo test -p mustard-cli 2>&1 | grep -q "test result: ok"`
- **AC-6** — when uma instalação antiga com git.flow preenchido abre uma unidade a partir de um branch que o flow NÃO declara, then o portão de base aceita a abertura e a base declarada aparece apenas como pré-seleção
  Command: `cargo test -p mustard-rt --lib commands::event::base_gate::tests::a_declared_flow_preselects_without_refusing_others -- --exact 2>&1 | grep -q "test result: ok. 1 passed"`
  Control: `cargo test -p mustard-core 2>&1 | grep -q "test result: ok"`
- **AC-7** — o build do projeto passa verde
  Command: `cargo build --workspace`

<!-- PLAN -->

## Arquivos

**packages/core** — o modelo: separa ponto-de-corte de branch-protegido.
- `packages/core/src/domain/config.rs` — `integration_bases()` deixa de ser a única definição de base; nascem `protected_branches()` (padrão do remoto + lista opcional) e a leitura de `git.flow` como pré-seleção, não como restrição.

**apps/rt** — os portões e o vocabulário.
- `apps/rt/src/commands/event/base_gate.rs` — aceita qualquer branch real; a recusa por base inexistente sai, a recusa por base atrasada em relação ao remoto fica.
- `apps/rt/src/commands/event/work_branch.rs` — `is_protected()` passa a consultar `protected_branches()`.
- `apps/rt/src/shared/work_kind.rs` — o enum fechado vira vocabulário aberto; `base_of_kind` deixa de existir.
- `apps/rt/src/commands/event/base_candidates.rs` (novo) — o comando `run base-candidates`: fetch, lista, ordena por recência.
- `apps/rt/src/commands/event/cli.rs` + `tests/run_command_surface.rs` — os quatro registros que um subcomando novo exige.
- `apps/rt/src/hooks/bash/safety.rs` — a proteção contra comando destrutivo passa a ler a lista de protegidos.
- `apps/rt/src/commands/review/pr_door.rs` — o alvo do PR passa a ser a base registrada da unidade.
- `apps/rt/src/commands/work_unit_open.rs`, `apps/rt/src/commands/doctor/doctor.rs` — acompanham a nova origem.

**apps/cli** — a instalação.
- `apps/cli/src/commands/git_flow.rs`, `apps/cli/src/commands/init.rs` — as perguntas de produção/dev saem; a sondagem de `origin/HEAD` que já existe permanece e passa a servir a proteção.

**prosa** — o que ensina a regra.
- `plugin/commands/*.md` e o orquestrador — a pergunta de duas opções vira seletor sobre lista real.

## Limites

IN: a origem da lista de bases (git em vez de arquivo), a separação entre ponto-de-corte e branch-protegido, a abertura do vocabulário de tipo, o comando novo que lista candidatas, e a prosa que ensina as três coisas.

OUT: o formato do nome do branch além do prefixo; qualquer migração obrigatória de `mustard.json`; o comportamento do dashboard (verificado: não depende de `git.flow`); a recusa por base atrasada em relação ao remoto, que continua exatamente como está.

## Definitions

- **base** — o branch de onde a unidade de trabalho é cortada. Hoje é obrigatoriamente um membro do conjunto declarado em mustard.json#git.flow; nesta unidade passa a ser qualquer branch real do remoto que o operador escolher numa lista.
- **integration base** — o termo atual do código para um branch declarado em git.flow. Ele acumula DOIS papéis que esta unidade separa: ser um ponto de corte válido e ser protegido contra commit direto.
- **branch protegido** — o branch que recusa commit e merge diretos. Passa a ser apenas o branch padrão do remoto (origin/HEAD), mais uma lista opcional por projeto — deixa de ser todo o conjunto de bases.
- **tipo (WorkKind)** — o prefixo do nome do branch (feature/, fix/, hotfix/). Hoje é um enum fechado de três valores que DECIDE de qual base a unidade sai; passa a ser um rótulo aberto que não decide nada.
- **hotfix** — hoje não é um terceiro tipo de trabalho e sim um DESTINO — a mesma correção é fix ou hotfix conforme a base de onde sai. Com a base escolhida explicitamente, essa distinção deixa de precisar existir no tipo.

## Decisions

- o git passa a ser a fonte da verdade sobre quais bases existem: antes de abrir a unidade o fluxo roda git fetch e lista os branches reais, ordenados por recência do último commit, para o operador escolher
  Reason: em uso corporativo os branches variam por cliente e por time; qualquer lista declarada em arquivo envelhece no dia seguinte à instalação e passa a recusar bases que existem de verdade
- apenas o branch padrão do remoto (origin/HEAD) é protegido contra commit e merge diretos, com uma lista opcional em mustard.json para times que também protegem develop ou release/*
  Reason: foi a única ressalva que o operador declarou; e com a base livre para escolha, proteger toda base possível equivaleria a proteger todo branch do repositório
- mustard.json#git.flow deixa de ser perguntado no init e deixa de restringir a escolha; quando presente, apenas pré-seleciona a opção no seletor
  Reason: instalações existentes não podem quebrar e não deve haver passo de migração obrigatório — o que o operador pediu que acabasse foi a PERGUNTA, não a compatibilidade
- o tipo do branch vira vocabulário aberto: sugestões usuais (feature, fix, hotfix, chore, refactor, docs), texto livre aceito, lista sobrescrevível por projeto
  Reason: o acoplamento tipo-decide-base morre quando a base é escolhida diretamente, e o que sobra do tipo é só o prefixo do nome
- convenção de nome com identificador de ticket (feature/PROJ-123-descricao) fica FORA desta unidade
  Reason: não foi pedida pelo operador; se a empresa exigir, entra como unidade própria em vez de alargar esta

## Evidence

- integration_bases() deriva o conjunto de bases das chaves nao-* e dos valores de git.flow, caindo em {main, master} quando o mapa esta vazio — e esse conjunto e a UNICA definicao de base no projeto
  Evidence: `packages/core/src/domain/config.rs:84`
- is_protected() protege exatamente esse mesmo conjunto, o que prova que ponto-de-corte e branch-protegido sao hoje a mesma lista e precisam ser separados
  Evidence: `apps/rt/src/commands/event/work_branch.rs:376`
- o base gate recusa a abertura da unidade quando o checkout nao e membro do conjunto declarado, listando as bases do arquivo
  Evidence: `apps/rt/src/commands/event/base_gate.rs:93`
- WorkKind e um enum fechado de tres valores e base_of_kind faz o tipo DECIDIR a base: Feature e Fix saem da base de trabalho, Hotfix sai da base de emergencia
  Evidence: `apps/rt/src/shared/work_kind.rs:347`
- o init do CLI ja sonda origin/HEAD e a lista de branches remotos antes de perguntar producao/dev — a materia-prima do seletor ja existe e nao precisa ser criada
  Evidence: `apps/cli/src/commands/git_flow.rs:69`
- o guard de comandos destrutivos do bash le o mesmo integration_bases(), entao a mudanca de semantica atinge tambem a protecao contra rm/reset em base
  Evidence: `apps/rt/src/hooks/bash/safety.rs:233`
- hipotese refutada: o dashboard NAO depende de git.flow. git_info.rs consulta o git diretamente e ja lista branches locais capados em ~20, entao nenhuma tela quebra com a mudanca
  Evidence: `apps/dashboard/src-tauri/src/git_info.rs:101`
