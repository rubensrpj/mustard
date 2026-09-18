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
| 3. Plano | `mustard-rt run plan --spec <spec>` | Monta o pedido de cada onda e confere o plano; refaz a página e manda publicá-la e fazer a pergunta de aprovação. |
| 4. Aprovação | nenhum | O clique em "Aprovar" é gravado pela testemunha. "Ajustar" não aprova. |
| 5. Ondas e revisão | `mustard-rt run round --spec <spec>` | Despacha as ondas que podem sair juntas. A onda com item do projeto todo ou sem dono só sai depois da análise antes do envio: a rodada devolve em `analysis` o pedido pronto para um agente com o modelo Sonnet. Com `--report '<as linhas do fim de cada agente>'`, grava o que cada onda entregou e o veredito da revisão, faz o commit da rodada e despacha a seguinte. |
| 6. Fechamento | `mustard-rt run close --spec <spec>` | Roda cada critério uma vez e fecha a spec. `--report '<as linhas do fim de cada agente>'` traz o relatório da última rodada, no mesmo formato da rodada. |
| 7. Pull request | `mustard-rt run pr-open --base <base> --head <branch> --spec <spec>` | Monta o título e o corpo a partir da spec e abre o pull request; o que já existe só tem o corpo reescrito. Com submódulo mexido pela spec, abre antes o de cada submódulo e o do principal como rascunho, que fica pronto quando eles entram (`pr-merge --root <submódulo>` ou o início da sessão). `--draft` abre como rascunho; `--fill` monta título e corpo pelos commits, para o repositório sem spec. |
| 8. Merge | `mustard-rt run pr-merge --pr <n>` | Só quando o usuário pede. Sem veredito aprovado, pergunta e não mexe em nada; `--confirm` é a resposta. |

O relatório da rodada não é um objeto só: são as linhas do fim dos agentes, como elas vieram — `<DELIVERED>{…}</DELIVERED>` do agente de onda, `<VERDICT>{…}</VERDICT>` do revisor e `<ANALYSIS>{…}</ANALYSIS>` do agente da análise antes do envio, uma por linha, quantas forem. O resto do texto não é lido, e um relatório sem nenhuma dessas marcas é recusado com `round-line-missing`, sem gravar nada. A análise traz `wave`, `removed` e `added`, cada item com o código e o motivo numa frase; o envio grava os itens que ficaram e, à parte, em `analysis`, o que saiu e o que entrou. A entrega traz `wave`, `text`, `files` e `commit` — o resumo de onde a mensagem do commit é montada, exigido quando há arquivo —, e ainda `proofs` (o critério e o comando de cada teste novo), `fixes` (as ondas que um conserto fecha) e `replan` (a mudança, quando o plano da onda não funciona; ela só segue com o clique em "Aceitar"). O veredito traz `wave`, `result` (`approved` ou `rejected`), `text`, `criteria` e `lessons`; o da revisão final do conjunto traz também `final`, e a aprovada é a única que vem sem `wave`. O fechamento lê o relatório da última rodada pela mesma porta.

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
| `write <tipo>` | Grava um evento: `mustard-rt run write point --spec <spec> --json '{…}'`. Com o tipo `lesson`, grava no banco de lições. A publicação da página do projeto sem `--spec` grava o endereço direto no índice das specs. |
| `resume` | A fase, o próximo passo em palavras e o comando que o faz. |
| `reopen` | `mustard-rt run reopen --reason "<motivo>"` leva a spec de volta ao levantamento. |
| `discard` | Descarta a spec em duas chamadas: a primeira mostra o que sai e devolve um código; a segunda, com `--confirm <código>`, faz. `--remote` apaga também a branch do servidor; `--delete` apaga a pasta da spec em vez de guardá-la. |
| `pending` | A lista de pendências. `--add --title "…" --detail "…"` acrescenta; `--close P-<n> --reason "…"` dá como entregue; `--remove` com `--id`, `--term` ou `--before <dia>` tira em duas chamadas, a segunda com `--confirm <código>`; `--drop P-<n>` é a mesma saída para um item; `--reopen P-<n>` traz de volta; `--stale` mostra as paradas há 30 dias e `--expire --keep P-<a>,P-<b>` tira as outras. |
| `index` | Refaz o índice das specs. |
| `scan` | Atualiza o mapa do projeto, lendo só o que mudou; nunca escreve no git. `--full` refaz o mapa de cada subprojeto; `--out <caminho>` grava o modelo em outro lugar. |
| `map <pergunta>` | Pergunta ao mapa: `examples` (`--file <arquivo>` ou `--task "<tarefa>"`), `importers --file`, `tests --file`, `search --query "<palavras>"`, `summary` e `skill --path <SKILL.md>`. |
| `page` | Gera uma página no layout do Mustard: `--body <pagina.md> --out <pagina.html>`, com `--title`, `--subtitle` e `--kind` opcionais. `--spec <spec>` refaz a página e o `.md` da spec. `--spec <spec> --owners [<donos.json>]` grava a lista dos itens combinados sem dono (`owners.html`), com o dono proposto e a regra de onde ele veio, para conferir antes de gravar; o arquivo traz o dono que o orquestrador dá. |
| `clean` | Lista as cópias descartáveis que os agentes deixaram no diretório temporário. `--dry-run` só lista, que é o padrão; `--apply` apaga as listadas; `--path <pasta>` apaga só aquela pasta. |
| `pr-review` | Sem número, lista os pull requests abertos da base; `--pr <n>` mostra o pedido de revisão. Não grava veredito: `--verdict` recusa na entrada, porque o veredito de cada onda é gravado pela rodada. |
| `upsert` | Instala ou atualiza. Na mesma chamada, tira as sobras do Mustard antigo nos `CLAUDE.md` e no `settings.json` da equipe, com as Guards virando lições antes, e diz o que saiu; o arquivo sem marca só aparece na lista. |
| `doctor` | Diagnóstico só de leitura. `--check <nome>` roda uma conferência; `--residue` procura também referências mortas; `--format json` ou `--json` responde em JSON. |
| `statusline` | A barra de status, chamada pelo Claude Code. `--preview` mostra cada tema numa linha. |

---

## A instalação

`mustard-rt run upsert` grava, escondidos do git do projeto:

- `.claude/settings.local.json`, `.claude/.gitignore` e `mustard.json`, que são da pessoa: o que existe fica, e só o que falta é acrescentado;
- o mapa `.claude/mustard/session-map.md`, os dois templates das páginas em `.claude/mustard/pages/` (`spec.html` e `project.html`) e os três agentes em `.claude/agents/mustard/`, no idioma do `language.text`. Esses são textos do próprio Mustard: toda execução regrava o texto embarcado, então uma cópia editada volta em `updated`, e uma idêntica volta em `preserved`, porque não havia o que escrever.

O `.claude/settings.local.json` recebe as liberações do próprio Mustard, na instalação nova e na atualização: os comandos `mustard-rt run` e a ferramenta `ArtifactData`, que grava no banco de dados das páginas publicadas. As liberações que a pessoa já tem ficam como estão.

Uma instalação antiga que declarava as três partes do roteador (`orchestrator.md`, `dispatch.md` e `material.md`) passa a declarar o mapa no lugar delas, e os três arquivos saem do disco. Uma instalação com o mapa de nome antigo, `mapa-inicio-sessao.md`, passa a declarar o `session-map.md` no `mustard.json`, sem mudar o resto dele, e o arquivo antigo sai do disco. Nada é commitado.
