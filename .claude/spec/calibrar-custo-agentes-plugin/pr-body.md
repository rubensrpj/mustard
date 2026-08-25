Cada um dos três agentes de `plugin/agents/` passa a declarar em qual modelo roda e com quanto
orçamento de raciocínio. Antes os três herdavam a sessão, então destilar linhas de Guards custava o
mesmo que verificar código adversarialmente. Agora a decisão de custo está escrita no arquivo que o
runtime lê, e um teste impede que ela caia sem ninguém notar.

## Por quê

O trabalho dos três é de dificuldade muito diferente e o preço era o mesmo.

`mustard-guards` e `mustard-patterns` recebem no prompt de dispatch os fatos que o binário já
calculou — a lista de arquivos, os exemplares, o subprojeto — e dão forma a eles. `mustard-review`
faz o oposto: recebe um diff e tenta refutá-lo, e é o único dos três em que o raciocínio é o produto,
não o acabamento.

Nada disso podia ser expresso. `DispatchItem` (`apps/rt/src/commands/pipeline/dispatch_plan.rs:60`)
e `AdvanceItem` (`apps/rt/src/commands/pipeline/wave_advance.rs:79`) carregam papel, subprojeto,
tipo de agente e prompt, e nenhum campo de modelo ou de esforço. O campo `model` só aparece depois,
lido da invocação para telemetria e preço — nunca para decidir nada.

O frontmatter de subagente já aceitava as duas chaves. Elas simplesmente não estavam declaradas.

## O que mudou

| arquivo | antes | depois |
|---|---|---|
| `plugin/agents/mustard-guards.md` | herda a sessão | `model: sonnet` · `effort: low` |
| `plugin/agents/mustard-patterns.md` | herda a sessão | `model: sonnet` · `effort: low` |
| `plugin/agents/mustard-review.md` | herda a sessão | `model: inherit` · `effort: high` |

`inherit` está declarado de propósito em `mustard-review`, mesmo sendo o comportamento padrão de uma
chave ausente. Com os outros dois declarando um modelo, a ausência passaria a ser ambígua entre
"esqueceram" e "a sessão é a escolha" — e o teste não teria como cobrar os três sem exceção.

**Nota sobre o diff.** Este pull request carrega três commits, não um. Junto de
`feat(plugin): declara model e effort no frontmatter dos tres agentes` viajam dois commits que já
estavam em `dev` localmente e nunca haviam sido enviados: `chore: update project config version
stamp` e `chore: refresh the deterministic project census` — o censo determinístico do projeto é
regravado sozinho quando uma unidade abre sobre uma base atualizada. Esses dois respondem por quase
todas as 1129 inserções e 720 remoções que o provedor mostra. A mudança descrita aqui são 163 linhas
em 4 arquivos.

`apps/rt/tests/plugin_agents.rs` ganha a catraca: todo arquivo de `plugin/agents/` precisa declarar
as duas chaves, com valor que o runtime realmente resolve. O arquivo já existia com esse papel para
outra superfície (a tabela de placeholders do ref de prompt); esta é a segunda trava dele.

## Como validar

Num diretório descartável, sem tocar em nada seu:

```sh
D=$(mktemp -d) && [ -n "$D" ] && cd "$D"
git clone --branch feature/calibrar-custo-agentes-plugin --depth 20 \
  https://github.com/rubensrpj/mustard.git . 

# as três declarações
grep -A1 '^tools:' plugin/agents/*.md

# a catraca passa
cargo test -p mustard-rt --test plugin_agents

# e reprova quando a declaração cai
sed -i '/^model: /d' plugin/agents/mustard-guards.md
cargo test -p mustard-rt --test plugin_agents shipped_agents_declare_model_and_effort   # falha
```

## Testes

Os dois critérios foram provados VERMELHOS contra a árvore sem a mudança, com os três arquivos
retirados de lado, e depois confirmados verdes com ela de volta. Medido, não estimado:
39 suítes, 2171 testes, zero falhas em `mustard-rt`; a suíte `plugin_agents` sozinha tem 3.

| critério | o que garante | comando |
|---|---|---|
| AC-1 | todo agente declara `model` e `effort`, com valor do vocabulário aceito | `cargo test -p mustard-rt --test plugin_agents shipped_agents_declare_model_and_effort` |
| AC-2 | a calibração é a decidida: `review` herda a sessão com `high`, os outros dois rodam mais barato com `low` | `grep -qx 'model: inherit' plugin/agents/mustard-review.md && grep -qx 'effort: high' plugin/agents/mustard-review.md && grep -qx 'effort: low' plugin/agents/mustard-guards.md && grep -qx 'effort: low' plugin/agents/mustard-patterns.md` |
| AC-3 | rede de segurança: o build fecha verde | `cargo build -p mustard-rt` |

## Decisões que valem explicação

**A catraca confere o VALOR, não só a presença da chave.** `effort: fast` é aceito pelo arquivo e
ignorado em execução: o arquivo continua lendo como decisão deliberada para quem abre, e o efeito
não existe. Esse é exatamente o modo de falha que o cabeçalho daquele arquivo de teste descreve —
nada neste workspace deixa de compilar quando uma dessas chaves cai ou é digitada errada. Presença
sem vocabulário certificaria como conforme justamente o arquivo quebrado.

**O teste NÃO trava qual modelo cada agente usa.** Sonnet ou haiku para `guards` é decisão de
operação, para revisitar conforme custo e modelos mudam. Fixar isso no teste transformaria toda
reafinação em teste vermelho, e ensinaria o próximo autor a editar o guarda em vez de pensar.

**Sonnet, e não o mais barato da faixa, para `guards` e `patterns`.** O que esses dois escrevem fica
versionado e é cobrado depois: uma linha de Guards marcada `[critical]` vira recusa automática no
gate de edição, e um molde `{role}-pattern` é copiado por todo implementador seguinte. Um molde
fraco não custa na hora — custa em código que nasce na forma errada.

## Fora de escopo

**Os papéis que caem em agentes embutidos** (`explore` e `impl`) continuam herdando a sessão.
Declarar modelo para eles exigiria campo novo em `DispatchItem` e `AdvanceItem`, com a mudança
correspondente no que o orquestrador relaia — trabalho em Rust, não em frontmatter. Fica para
depois, se esta fase se pagar.

**Nenhuma medição de economia acompanha este pull request.** A mudança move onde a decisão de custo
é declarada; quanto ela poupa por execução só aparece na telemetria depois de rodar, e afirmar um
número agora seria estimativa vestida de medida.
