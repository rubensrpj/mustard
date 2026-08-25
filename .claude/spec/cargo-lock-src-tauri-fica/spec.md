---
id: spec.cargo-lock-src-tauri-fica
---

# O Cargo.lock do src-tauri fica para tras a cada release de versao

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

O bump automatico grava o selo de versao a cada merge na main. Ele cuida de tres arquivos: o
`plugin/.claude-plugin/plugin.json`, o `version` do `Cargo.toml` da raiz e o `Cargo.lock` da raiz —
esse ultimo por um `cargo update --workspace`. O proprio workflow chama esses tres de "as pernas do
selo", e um comentario dentro dele conta que esquecer a terceira derrubou o release da v0.1.29 nos
tres sistemas operacionais.

Existe uma QUARTA que ninguem declarou. `apps/dashboard/src-tauri` e workspace root proprio de
proposito, entao tem o SEU `Cargo.lock`, e o `cargo update --workspace` da raiz nao o alcanca.

O problema nao e so ficar velho: e ninguem descobrir. A CI exclui o dashboard de proposito, porque
ele precisa de bibliotecas de sistema por sistema operacional. O release o compila por `tauri build`,
sem `--locked`, entao o lockfile velho e consertado na hora do build e o conserto e jogado fora em
vez de commitado. Resultado medido em 22/08/2026: aquele arquivo ainda fixava `mustard-cli` e
`mustard-core` em `0.1.41` com o repositorio em `0.1.44` — tres releases atras. Quem tentar
compilar o dashboard recebe a arvore suja sem ter pedido nada, e no dia em que alguem acrescentar
`--locked` aquele build, o release quebra pelo mesmo motivo da v0.1.29.

O que muda sao duas coisas, e as duas precisam existir. O workflow ganha a quarta perna, nas DUAS
pernas dele (a da main e a que propaga para o dev): mais um `cargo update --workspace` apontado para
o manifesto do dashboard, a conferencia de que o lock realmente andou, e o arquivo entrando no `git
add`. E `packages/core/tests/version_line.rs` — que hoje explica em prosa por que NENHUM lockfile e
verificado — ganha o guarda para este, com a distincao escrita: o lock da raiz de fato nao pode ser
testado, o do dashboard pode, e a razao e a mesma que criou o defeito.

Como termina: o selo alcanca as quatro pernas sozinho, e se a quarta parar, um teste diz qual comando
resolve em vez de o defeito reaparecer em silencio por mais tres releases.

## Usuários/Stakeholders

Quem compila o dashboard (deixa de receber a arvore suja de graca) e quem opera o release (o
`--locked` deixa de ser uma bomba adormecida). Quem editar o workflow depois: o teste avisa se a
quarta perna cair.

## Métrica de sucesso

O lockfile do dashboard fixa a versao do repositorio, o workflow o atualiza nas duas pernas, e o
teste que exige isso falha contra a arvore de hoje e passa depois.

## Não-Objetivos

- Colocar o dashboard na CI. Ele foi excluido de proposito por precisar de bibliotecas de sistema
  por SO; incluir e uma decisao de custo de matriz, nao parte deste conserto.
- Acrescentar `--locked` ao `tauri build` do release. Seria o guarda mais forte, mas trocaria uma
  falha silenciosa por um release quebrado enquanto o lock ainda estiver velho — a ordem certa e
  primeiro parar a deriva, depois apertar.
- Mexer no lockfile da raiz ou no fluxo de tres pernas que ja funciona.

## Critérios de Aceitação

- **AC-1** — when o lockfile do dashboard e lido, then todo pacote deste repositorio nele esta fixado
  na versao do repositorio
  Command: `cargo test -p mustard-core --test version_line the_dashboard_lock_pins_this_repositorys_crates_at_this_version`
  Control: `cargo test -p mustard-core --test version_line the_workspace_version_equals_the_published_plugin_version`
- **AC-2** — when o workflow de bump e lido, then as DUAS pernas atualizam o lock do dashboard e o
  incluem no commit, e a perna do dev CONSULTA esse lock antes de decidir que nao ha nada a propagar
  Command: `test "$(grep -c 'cargo update --workspace --manifest-path apps/dashboard/src-tauri/Cargo.toml' .github/workflows/bump-on-main.yml)" = 2 && test "$(grep -c 'git add .*apps/dashboard/src-tauri/Cargo.lock' .github/workflows/bump-on-main.yml)" = 2 && grep -qE '^[[:space:]]*if \[.*dash_pin.*\]; then' .github/workflows/bump-on-main.yml`
  Control: `grep -q 'cargo update --workspace' .github/workflows/bump-on-main.yml`
- **AC-3** — o build do projeto passa verde
  Command: `cargo build -p mustard-core`

## Checklist

- [x] T1 — escrever o guarda em `version_line.rs` e ver AC-1 vermelho.
- [x] T2 — reescrever a prosa do modulo que hoje diz que nenhum lockfile e verificado.
- [x] T3 — quarta perna nas duas pernas do `bump-on-main.yml`, com checagem sem cano.
- [x] T4 — consertar o lockfile do dashboard com `cargo update --workspace --manifest-path`.
- [x] T5 — rodar AC-1, AC-2 e AC-3 verdes.
- [x] T6 — fechar o buraco achado na revisao: a perna do dev pulava tudo quando as tres primeiras
  ja estavam em dia, e a quarta nunca andava.

## Definitions

- **lockfile (Cargo.lock)** — Arquivo que registra a versao exata resolvida de cada dependencia, incluindo os pacotes do proprio repositorio. Com `--locked` o build LE esse arquivo e recusa mexer nele; sem `--locked` o cargo o conserta em silencio e segue.
- **workspace root separado** — `apps/dashboard/src-tauri` declara um `[workspace]` proprio de proposito (Cargo.toml:13-17), para que a raiz do repositorio nao reivindique o pacote. Consequencia: ele tem o SEU Cargo.lock, e o `cargo update --workspace` rodado na raiz nao o enxerga.
- **selo de versao (version stamp)** — O numero que o bump automatico grava a cada merge na main. Hoje ele tem tres pernas declaradas — plugin.json, Cargo.toml da raiz e Cargo.lock da raiz — e o workflow cuida das tres.

## Decisions

- Consertar na ORIGEM (o workflow de bump), nao editando o arquivo a mao.
  Reason: Editar o lockfile agora resolve a divergencia de hoje e garante que ela volte na proxima release. O unico commit que move a versao sem o lock e o do bump automatico, e e nele que a quarta perna precisa nascer.

## Evidence

- src-tauri e workspace root proprio, entao seu Cargo.lock e separado do da raiz.
  Evidence: `apps/dashboard/src-tauri/Cargo.toml:17`
- O bump roda `cargo update --workspace` nas DUAS pernas (main e dev), e isso so alcanca o Cargo.lock da raiz. Nada no repositorio atualiza o do src-tauri.
  Evidence: `.github/workflows/bump-on-main.yml`
- A CI exclui o dashboard de proposito: ele precisa de bibliotecas de sistema por SO (webkit2gtk no Linux). Entao nenhuma checagem automatica nunca abre esse lockfile.
  Evidence: `.github/workflows/ci.yml:52`
- O release constroi o dashboard por `tauri build`, SEM `--locked`. O lockfile velho e consertado em silencio na hora do build e o conserto e jogado fora — nunca volta para o git.
  Evidence: `.github/workflows/release.yml:139`
- Divergencia medida hoje: o lockfile do src-tauri registra mustard-cli e mustard-core em 0.1.41 enquanto o workspace esta em 0.1.44 — tres releases atras.
  Evidence: `apps/dashboard/src-tauri/Cargo.lock`
- version_line.rs documenta por que um teste sobre o Cargo.lock DA RAIZ nao pode falhar: o `cargo test` comum conserta o lock antes de rodar qualquer coisa, e o `cargo test --locked` falha dentro do proprio cargo antes de o binario de teste iniciar. Esse argumento NAO se transfere para o lock do src-tauri, que um teste de outro workspace le como ARQUIVO — nada o conserta antes.
  Evidence: `packages/core/tests/version_line.rs`
