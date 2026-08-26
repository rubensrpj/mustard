# fix(dashboard): teste de telemetria mede o código, não a máquina

Um teste do dashboard passava apenas num runner de integração contínua pelado e falhava em toda
máquina de desenvolvimento do Mustard. Ele não media o código: media se o `rtk` estava instalado.
Este pull request troca essa afirmação pela invariante que a função realmente garante.

## Por quê

`rtk_summary_is_unavailable_on_clean_repo` criava um diretório temporário vazio e exigia
`!r.available`. Mas `rtk_summary` **não lê** esse diretório — ele **executa** o binário `rtk` lá
dentro (`src/telemetry.rs:857-861`), e o `rtk`, quando não encontra dados do projeto, cai no escopo
global e responde normalmente.

Medido em 22/08/2026:

```
rtk gain  num diretório vazio    →  "Global Scope"      ·  13076 comandos  →  available = true  ✗
rtk gain  dentro do repositório  →  escopo do projeto   ·   3924 comandos  →  correto
```

Ou seja, **não há defeito de produto**: dentro de um projeto real o escopo vem certo e o dashboard
mostra os números certos. O que existia era um teste afirmando uma propriedade da máquina — o `rtk`
não estar instalado. Como o harness deste projeto roteia comandos pelo `rtk`, ele está instalado em
toda máquina de desenvolvimento, e o teste falhava em todas elas.

Isso ficou invisível enquanto o dashboard nem compilava localmente, por faltarem as bibliotecas de
sistema do GTK/WebKit. Assim que elas foram instaladas, este foi o único item vermelho que sobrou no
portão de verificação.

## O que mudou

O teste passa a se chamar `rtk_summary_is_well_shaped_on_clean_repo` e afirma a invariante nos
**dois** ramos, sem depender de quem roda:

| ramo | o que é afirmado |
|---|---|
| indisponível | a forma zerada — `daily` vazio e todo campo medido ausente (`RtkBlock::default()`) |
| disponível | ao menos um campo medido presente — um bloco que se diz disponível e vem vazio é a falha que este teste existe para pegar |

Nenhum dos dois ramos fica vazio: numa máquina com `rtk` roda o segundo, numa sem ele roda o
primeiro. A garantia que o teste original queria proteger continua afirmada; ela só deixou de ser um
pressuposto sobre o ambiente.

**Junto, pela mesma causa.** Agora que o dashboard compila, o build gera
`apps/dashboard/src-tauri/gen/schemas/*.json`, e esse diretório não estava no `.gitignore`. Ele é
gerado e regenerável — a mesma classe de `target/` e `payload/`, que o arquivo já ignora com
comentário explícito. Sem a linha, toda compilação do dashboard suja a árvore, inclusive a do próprio
portão de fechamento.

A regra ignora **`gen/schemas/`**, e não `gen/` inteiro. A primeira versão deste pull request ignorava
o diretório todo, o que seria uma armadilha silenciosa: `gen/android/` e `gen/apple/` são projetos de
verdade, gerados uma vez por `tauri android init` e **versionados**. Ignorar o pai os engoliria no dia
em que alguém adicionasse um alvo móvel — o diretório existiria na máquina de quem rodou o `init` e em
mais nenhuma.

## Como validar

Num diretório descartável, sem tocar em nada seu:

```sh
D=$(mktemp -d) && [ -n "$D" ] && cd "$D"
git clone --branch fix/teste-telemetria-assume-maquina --depth 20 \
  https://github.com/rubensrpj/mustard.git .

# passa numa máquina COM rtk instalado — que é onde o antigo falhava
cargo test --manifest-path apps/dashboard/src-tauri/Cargo.toml --test telemetry_test

# e o diretório gerado não suja mais a árvore
cargo build --manifest-path apps/dashboard/src-tauri/Cargo.toml
git status --porcelain apps/dashboard/src-tauri/gen    # sem saída
```

## Testes

Os três critérios foram provados **vermelhos** contra a árvore sem a correção e confirmados verdes
depois. Medido, não estimado: 5 suítes, 145 testes, zero falhas em `apps/dashboard/src-tauri`.

| critério | o que garante | comando |
|---|---|---|
| AC-1 | a suíte de telemetria passa numa máquina **com** `rtk` instalado | `cargo test --manifest-path apps/dashboard/src-tauri/Cargo.toml --test telemetry_test` |
| AC-2 | o teste substituto existe e **roda** — não basta a suíte ficar verde por remoção | `cargo test --manifest-path apps/dashboard/src-tauri/Cargo.toml --test telemetry_test rtk_summary_is_well_shaped_on_clean_repo 2>&1 \| grep -q 'ok. 1 passed'` |
| AC-3 | compilar o dashboard não suja mais a árvore | `cargo build --manifest-path apps/dashboard/src-tauri/Cargo.toml > /dev/null 2>&1; test -z "$(git status --porcelain apps/dashboard/src-tauri/gen)"` |
| AC-4 | rede de segurança: o build fecha verde | `cargo build --manifest-path apps/dashboard/src-tauri/Cargo.toml` |

## Decisões que valem explicação

**O código de produção não foi tocado, de propósito.** A tentação era mudar `rtk_summary` para não
cair no escopo global. Medi antes: dentro de um projeto real o escopo já vem certo, então mexer ali
seria consertar o que não está quebrado e arriscar o que funciona. O spec desta unidade proíbe
explicitamente editar `telemetry.rs`.

**AC-2 existe para impedir o conserto errado.** Sozinho, o AC-1 seria satisfeito simplesmente
apagando o teste incômodo. O AC-2 exige que um teste com o nome novo **rode** — daí o
`grep 'ok. 1 passed'`, porque um filtro do `cargo test` que não casa com nada roda zero testes e sai
com código zero, ou seja, passa por verde sem ter verificado nada.

**Descartei um critério que cobriria o outro ramo.** Cheguei a desenhar um que rodasse a suíte com o
`PATH` sem o `rtk`, para exercitar o ramo indisponível. O `rtk` mora em `/usr/bin`, então escondê-lo
levaria junto o compilador e o ligador — o critério ficaria amarrado a esta máquina, que é exatamente
o defeito sendo consertado. O ramo indisponível fica coberto pela asserção, não por um critério
executável.

**Nota sobre o diff.** A branch carrega quatro commits, não um: o conserto, o estreitamento da regra
do `.gitignore` que a revisão pediu, e um revert. O revert existe porque um `git add -A` rodou
enquanto o portão de verificação compilava o dashboard em segundo plano e varreu o `Cargo.lock` dele
para dentro do commit. O arquivo foi tirado; o diff líquido desta branch são dois arquivos.

## Fora de escopo

**Colocar o dashboard na integração contínua.** Ele é excluído de propósito por exigir bibliotecas de
sistema por sistema operacional. Este pull request faz o teste parar de mentir; decidir se a matriz
de integração contínua passa a pagar esse custo é outra conversa.

**O `Cargo.lock` do dashboard.** Ele está desatualizado nesta branch e o build o repara sozinho — o
conserto disso é de outra unidade, e a alteração foi deliberadamente revertida antes do commit para
não duplicá-la aqui.
