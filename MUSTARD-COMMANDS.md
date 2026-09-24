# Mustard — Comandos e Fluxo

Referência de tudo o que o Mustard publica: o fluxo, os três comandos de barra, os textos que chegam à janela, os ganchos e os comandos `mustard-rt run`. Nomes de comando, opções e chaves ficam como são.

---

## O fluxo

Todo trabalho que muda arquivo passa por um fluxo só. Cada passo é uma chamada ao binário, e a resposta de cada chamada diz qual é o próximo passo: o modelo repassa essa resposta, em vez de decidir a ordem.

```mermaid
flowchart LR
    A["open"] --> B["grill"] --> C["plan"] --> D["aprovação<br/>(clique)"] --> E["round"] --> F["close"] --> G["pr-open"]
    G -.->|"só a pedido"| H["pr-merge"]
```

| Passo | Comando | O que faz |
|---|---|---|
| 1. Abertura | `mustard-rt run open --kind <tipo> --name "<nome>" --base <base>` | Cria a branch `<tipo>/<nome>` e a spec com o mesmo nome. O que falta volta como pergunta, com os candidatos. `--pending P-<n>` liga a spec à pendência de onde ela veio. |
| 2. Levantamento | `mustard-rt run grill --spec <spec> --kinds feature` | Grava o tipo de trabalho e monta a lista de pontos. `--condensed` junta todos os pontos num bloco só, para um pedido que cabe numa frase. Cada resposta é gravada com `write`, que devolve o próximo ponto. |
| 3. Plano | `mustard-rt run plan --spec <spec>` | Monta o pedido de cada onda e confere o plano; prepara a cópia da spec para o banco de dados da página e manda publicar a página que ainda não tem endereço, copiar os lotes com a ferramenta `ArtifactData`, gravar a cópia feita (`write copy`) e fazer a pergunta de aprovação. A rodada, o fechamento e o pedido que muda o plano mandam copiar do mesmo jeito o que entrou desde a última cópia; a entrega que a onda grava só entra na cópia depois que a rodada a assume. |
| 4. Aprovação | nenhum | O clique em "Aprovar" é gravado pela testemunha. "Ajustar" não aprova. |
| 5. Ondas | `mustard-rt run round --spec <spec>` | Despacha as ondas que podem sair juntas, sem pedir a revisão de onda nenhuma. A onda com item do projeto todo, item sem dono ou lição que casa com ela só sai depois da escolha antes do envio: a rodada devolve em `analysis` esses candidatos ao orquestrador, cada um com o título, e nenhum agente é aberto para isso. A onda grava a própria entrega com `mustard-rt run write delivered --spec <spec> --json '<a entrega>'`, só com o envio dela aberto; a rodada seguinte assume a entrega gravada, faz o commit da rodada, grava cada sobra como pendência da spec e despacha a seguinte. `--report '<as linhas do orquestrador>'` leva o consumo, a pausa e a escolha antes do envio. |
| 6. Fechamento | `mustard-rt run close --spec <spec>` | Roda cada critério uma vez e pede o agente de teste dedicado, mesmo com uma onda só; ele confere a obra inteira de uma vez, aponta e não conserta. Reprovado, o orquestrador monta o pedido do conserto, um agente conserta e o agente de teste confere só o conserto — até duas voltas; depois, a decisão é do usuário. Só fecha a spec com a linha dele aprovada. `--report '<as linhas>'` traz as linhas da última rodada e a do agente de teste, no mesmo formato da rodada; a entrega das ondas já está na spec. Na resposta, cada pendência aberta nascida na obra vem com a pergunta de destino e a linha da resposta: `--pending-later "P-<n>=<o motivo>"` grava o "fica para depois", que solta a pendência da obra e a passa ao projeto — uma por pendência, e vale também depois de a obra fechar. |
| 7. Pull request | `mustard-rt run pr-open --base <base> --head <branch> --spec <spec>` | Monta o título e o corpo a partir da spec e abre o pull request; o que já existe só tem o corpo reescrito. Com submódulo mexido pela spec, abre antes o de cada submódulo e o do principal como rascunho, que fica pronto quando eles entram (`pr-merge --root <submódulo>` ou o início da sessão). `--draft` abre como rascunho; `--fill` monta título e corpo pelos commits, para o repositório sem spec. |
| 8. Merge | `mustard-rt run pr-merge --pr <n>` | Só quando o usuário pede. Sem veredito aprovado, pergunta e não mexe em nada; `--confirm` é a resposta. |

A entrega da onda mora na spec: o agente a grava com `mustard-rt run write delivered`, e só com o envio da onda aberto — sem ele, a gravação é recusada com `no-open-send`, sem gravar nada. A gravação já confere o título do commit (até 60 caracteres), o resumo com cara de SHA, o arquivo que o projeto não conhece e a prova nova, e grava a entrega como volta, escondida da leitura até a rodada assumi-la. A entrega traz `wave`, `text`, `files` e `commit` — o resumo de onde a mensagem do commit é montada, exigido quando há arquivo —, e ainda `proofs` (o critério e o comando de cada teste novo), `fixes` (as ondas que um conserto fecha), `replan` (a mudança, quando o plano da onda não funciona; ela só segue com o clique em "Aceitar") e `leftovers` (cada sobra com `title` e `detail`, que a rodada grava como pendência da spec). A rodada assume a volta: grava a entrega oficial, com `replaces` apontando as voltas desde o último envio, e comita com o resumo dela. O relatório da rodada são as linhas que o orquestrador escreve, uma por linha, quantas forem: `<ANALYSIS>{…}</ANALYSIS>` da escolha antes do envio, `<PAUSED>{…}</PAUSED>` da onda que pausou e `<USAGE>{…}</USAGE>` do consumo. O resto do texto não é lido; um relatório sem nenhuma dessas marcas é recusado, e a linha `<DELIVERED>` no relatório também, sem gravar nada. A escolha traz `wave`, `removed` e `added`, cada item com o código em `item` e o motivo numa frase; a lição sai pelo número do banco em `lesson`, e não entra por escolha. O envio grava os itens que ficaram e, à parte, em `analysis`, o que saiu e o que entrou. O veredito é do agente de teste dedicado, que o fechamento pede a toda obra, e ele o grava com `mustard-rt run write verdict`, só com o pedido de revisão aberto; a linha `<VERDICT>` no relatório é recusada, como a de entrega. O veredito traz `result` (`approved` ou `rejected`) e `final:true`; a aprovada é a única que vem sem `wave`, e a reprovada traz nele a onda que o conserto refaz. O consumo de cada onda vem numa linha à parte, `<USAGE>{"wave":1,"model":"…","steps":…,"tokens":…,"caller_steps":…,"caller_tokens":…}</USAGE>`, que só o orquestrador escreve com o número que a plataforma lhe entrega quando o agente termina; nenhum valor digitado pelo agente na entrega vira consumo, e a entrega com o campo de consumo é recusada. A linha de consumo sozinha completa a onda que já voltou; a de uma onda de lote com envio aberto, sem entrega gravada e com o Claude Code dela fechado marca a onda cortada, e as tarefas dela voltam para o backlog. O fechamento lê o relatório da última rodada pela mesma porta.

---

## Comandos de barra

| Comando | O que faz |
|---|---|
| `/mustard:continue` | Retoma a spec pelo `resume` e repassa o próximo passo. É o botão de reserva: a retomada já acontece no início da sessão. |
| `/mustard:pr` | Abre o pull request, revisa o de um colega ou faz o merge, só a pedido. |
| `/mustard:upsert` | Instala ou atualiza o Mustard no projeto, e diagnostica a instalação. |

Não há comando de entrada: um pedido que muda arquivo, dito na conversa, abre a spec.

---

## O que chega à janela

- **O mapa do início da sessão** (`.claude/mustard/session-map.md`, até 3 kB): o fluxo, quando uma spec abre e onde cada coisa mora. Entra no início da sessão, depois de `/clear` e da compactação.
- **A linha de cada mensagem** (até 100 caracteres): o idioma do texto e "texto simples". Depois de uma resposta com erro de escrita, ela leva mais uma frase curta com o erro, como "Na última resposta: frase com 29 palavras.", uma vez só.
- **O estilo de resposta** do plugin, um por idioma (`mustard:mustard-pt-BR`, `mustard:mustard-en-US`), escolhido pelo instalador na chave `outputStyle` do `.claude/settings.local.json`.
- **Os três agentes**, em `.claude/agents/mustard/`, no idioma do texto: `wave` implementa uma onda, `review` confere uma onda, um levantamento ou o pull request de um colega, e `skill` escreve uma skill a partir dos exemplos que o binário escolhe.

---

## Os ganchos

| Gancho | Quando | O que faz |
|---|---|---|
| `session_start_inject` | início da sessão | Coloca o mapa, a linha de retomada e os avisos, até 3 kB. Num projeto sem a página do projeto publicada, manda publicar o template dela e gravar o endereço. |
| `statusline_heal_observer` | início da sessão | Conserta a barra de status. |
| `prompt_entry` | cada mensagem | Coloca a linha curta e grava a mensagem na spec atual. |
| `write_gate` | antes de escrever | Recusa escrita sem spec aprovada, numa base, em arquivo de segredo e nos `spec.*`. |
| `command_guard` | antes de um comando | Recusa comando que apaga trabalho. |
| `subagent_inject` | antes de despachar um agente | Troca o bilhete `MUSTARD-WAVE: <spec> <n>` pelo pedido montado da onda. |
| `approval_witness` | depois de uma pergunta com opções | Grava o clique em "Aprovar" ou "Aceitar". |
| `end_of_turn_check` | fim da resposta | Confere a escrita sem barrar: o erro achado vai na linha da mensagem seguinte. Barra só quando a spec fecha ou entra no merge e a resposta não cita uma pendência aberta nascida nela. |
| `session_cleanup_observer` | fim da sessão | Solta a spec da sessão. |

---

## Os comandos `mustard-rt run`

Quase todos aceitam `--root <pasta>`, que diz de que pasta o repositório é lido; sem ela, vale a pasta atual. Não a aceitam `clean`, `upsert`, `doctor` e `statusline`, que trabalham sempre na pasta atual. Os que trabalham numa spec aceitam `--spec <spec>`; sem ela, vale a spec atual.

| Comando | O que faz |
|---|---|
| `read <bloco>` | Devolve um bloco da spec: `state`, `specification`, `agreed`, `waves`, `wave-<n>`, `criteria`, `review`, `progress`, `notes` ou `conversation`. `--term <termo>` filtra pelo termo ou pelo código do item. |
| `write <tipo>` | Grava um evento: `mustard-rt run write point --spec <spec> --json '{…}'`. Com o tipo `lesson`, grava no banco de lições. A publicação da página do projeto sem `--spec` grava o endereço direto no índice das specs. Com `--copy`, a gravação prepara a cópia da página: vai só na última gravação de um pedido do usuário que muda o plano, ou no próprio pedido quando ele não gera outra. |
| `resume` | A fase, o próximo passo em palavras e o comando que o faz. |
| `reopen` | `mustard-rt run reopen --reason "<motivo>"` leva a spec de volta ao levantamento. Numa obra já fechada cujo pull request o servidor reprovou, é a porta de conserto: abre a onda de conserto dentro da mesma spec e, com ela entregue e comitada pela rodada, empurra a branch para o servidor — sem reabrir a obra e sem spec nova. |
| `discard` | Descarta a spec em duas chamadas: a primeira mostra o que sai e devolve um código; a segunda, com `--confirm <código>`, faz. `--remote` apaga também a branch do servidor; `--delete` apaga a pasta da spec em vez de guardá-la. |
| `pending` | A lista de pendências. `--add --title "…" --detail "…"` acrescenta; `--close P-<n> --reason "…"` dá como entregue; `--remove` com `--id`, `--term` ou `--before <dia>` tira em duas chamadas, a segunda com `--confirm <código>`; `--drop P-<n>` é a mesma saída para um item; `--reopen P-<n>` traz de volta; `--stale` mostra as paradas há 30 dias e `--expire --keep P-<a>,P-<b>` tira as outras. |
| `index` | Refaz o índice das specs. |
| `scan` | Atualiza o mapa do projeto, lendo só o que mudou; nunca escreve no git. `--full` refaz o mapa de cada subprojeto; `--out <caminho>` grava o modelo em outro lugar. |
| `map <pergunta>` | Pergunta ao mapa: `examples` (`--file <arquivo>` ou `--task "<tarefa>"`), `importers --file`, `tests --file`, `slice --file <arquivo> --name <declaração>`, `users --name <declaração>` (quem usa a declaração, como `arquivo:linha:quem chama`; com `--file`, só a desse arquivo), `search --query "<palavras>"`, `summary` e `skill --path <SKILL.md>`. |
| `page` | Gera uma página no layout do Mustard a partir de um arquivo markdown: `--body <pagina.md> --out <pagina.html>`, com `--title`, `--subtitle` e `--kind` opcionais. A página da spec e a do projeto são templates que leem um banco de dados; o comando que as refazia saiu. |
| `clean` | Lista as cópias descartáveis que os agentes deixaram no diretório temporário. `--dry-run` só lista, que é o padrão; `--apply` apaga as listadas; `--path <pasta>` apaga só aquela pasta. |
| `pr-review` | Sem número, lista os pull requests abertos da base; `--pr <n>` mostra o pedido de revisão. Não grava veredito: `--verdict` recusa na entrada, porque o veredito de cada onda é gravado pela rodada. |
| `upsert` | Instala ou atualiza. Na mesma chamada, tira as sobras do Mustard antigo nos `CLAUDE.md` e no `settings.json` da equipe, com as regras das Guards indo antes para um item só da lista de pendências, e diz o que saiu; o arquivo sem marca só aparece na lista. |
| `doctor` | Diagnóstico só de leitura. `--check <nome>` roda uma conferência; `--residue` procura também referências mortas; `--format json` ou `--json` responde em JSON. |
| `statusline` | A barra de status, chamada pelo Claude Code. `--preview` mostra cada tema numa linha. |

---

## A instalação

`mustard-rt run upsert` grava, escondidos do git do projeto:

- `.claude/settings.local.json`, `.claude/.gitignore` e `mustard.json`, que são da pessoa: o que existe fica, e só o que falta é acrescentado;
- o mapa `.claude/mustard/session-map.md`, os dois templates das páginas em `.claude/mustard/pages/` (`spec.html` e `project.html`) e os três agentes em `.claude/agents/mustard/`, no idioma do `language.text`. Esses são textos do próprio Mustard: toda execução regrava o texto embarcado, então uma cópia editada volta em `updated`, e uma idêntica volta em `preserved`, porque não havia o que escrever.

O `.claude/settings.local.json` recebe as liberações do próprio Mustard, na instalação nova e na atualização: os comandos `mustard-rt run` e a ferramenta `ArtifactData`, que grava no banco de dados das páginas publicadas. As liberações que a pessoa já tem ficam como estão.

Uma instalação antiga que declarava as três partes do roteador (`orchestrator.md`, `dispatch.md` e `material.md`) passa a declarar o mapa no lugar delas, e os três arquivos saem do disco. Uma instalação com o mapa de nome antigo, `mapa-inicio-sessao.md`, passa a declarar o `session-map.md` no `mustard.json`, sem mudar o resto dele, e o arquivo antigo sai do disco. Nada é commitado.
