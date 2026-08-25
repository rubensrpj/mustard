# Tactical Fix: teste de telemetria assume maquina sem rtk instalado

## Contexto

Tactical fix derivado de [[cargo-lock-src-tauri-fica]].

`rtk_summary_is_unavailable_on_clean_repo` (`apps/dashboard/src-tauri/tests/telemetry_test.rs:20`)
cria um diretorio temporario vazio e exige `!r.available`. Mas `rtk_summary` nao le esse diretorio:
ele EXECUTA o binario `rtk` la dentro (`src/telemetry.rs:857-861`, `run_rtk_gain`), e o `rtk`, quando
nao acha dados do projeto, cai no escopo GLOBAL e responde normalmente.

Medido em 22/08/2026 nesta maquina: `rtk gain` num diretorio vazio devolve "Global Scope" com 13076
comandos, entao `available` vem `true` e a assercao quebra. Dentro do repositorio o mesmo comando
devolve 3924 — o escopo do projeto. Ou seja, NAO ha defeito de produto: o dashboard mostra os numeros
certos. O que existe e um teste que afirma uma propriedade da MAQUINA (o `rtk` nao estar instalado),
nao do codigo.

A consequencia e que ele passa so num runner pelado de integracao continua e falha em toda maquina de
desenvolvimento do Mustard, onde o `rtk` esta sempre instalado. Foi o unico item que sobrou vermelho
no portao de verificacao depois de as bibliotecas de sistema serem instaladas.

A correcao e medir o que a funcao garante: nao entrar em panico e devolver uma forma coerente. A
garantia que o teste original queria proteger — indisponivel implica zerado — continua valendo e deve
continuar afirmada, so que como invariante, e nao como pressuposto sobre a maquina.

Segundo item, mesma causa: agora que o dashboard compila nesta maquina, o build gera
`apps/dashboard/src-tauri/gen/schemas/*.json` e esse diretorio NAO esta no `.gitignore`. Ele e
gerado e regeneravel — a mesma classe de `target/` e `payload/`, que o arquivo ja ignora com
comentario explicito. Sem a linha, toda compilacao do dashboard suja a arvore, e o portao de
fechamento — que compila o dashboard — passaria a encontrar arvore suja justamente quando
finalmente consegue rodar.

## Critérios de Aceitação

- **AC-1** — when a suite de telemetria roda numa maquina COM `rtk` instalado, then ela passa inteira
  Command: `cargo test --manifest-path apps/dashboard/src-tauri/Cargo.toml --test telemetry_test`
  Control: `cargo test --manifest-path apps/dashboard/src-tauri/Cargo.toml --test telemetry_test hook_fire_counts_empty`
- **AC-2** — when o resumo do `rtk` volta indisponivel, then o bloco esta zerado — a garantia original
  segue afirmada, agora como invariante e nao como pressuposto sobre a maquina
  Command: `cargo test --manifest-path apps/dashboard/src-tauri/Cargo.toml --test telemetry_test rtk_summary_is_well_shaped_on_clean_repo 2>&1 | grep -q 'ok. 1 passed'`
  Control: `cargo test --manifest-path apps/dashboard/src-tauri/Cargo.toml --test telemetry_test hook_fire_counts_empty 2>&1 | grep -q 'ok. 1 passed'`
- **AC-3** — when o dashboard e compilado, then a arvore continua limpa
  Command: `cargo build --manifest-path apps/dashboard/src-tauri/Cargo.toml > /dev/null 2>&1; test -z "$(git status --porcelain apps/dashboard/src-tauri/gen)"`
  Control: `test -d apps/dashboard/src-tauri`
- **AC-4** — o build do subprojeto passa verde
  Command: `cargo build --manifest-path apps/dashboard/src-tauri/Cargo.toml`

## Arquivos

- `apps/dashboard/src-tauri/tests/telemetry_test.rs` — trocar a assercao dependente de maquina pela
  invariante, e renomear o teste para o que ele de fato mede.
- `.gitignore` — ignorar `apps/dashboard/src-tauri/gen/`, gerado pelo build e regeneravel.

Nao mexer em `apps/dashboard/src-tauri/src/telemetry.rs`: o comportamento de producao esta correto e
foi medido. Este conserto e do teste, nao do codigo.
