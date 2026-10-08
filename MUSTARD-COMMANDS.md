# Mustard — Comandos e Fluxo

Referência de tudo o que o Mustard publica: o fluxo, os comandos de barra, os textos que chegam à janela, os ganchos e os comandos `mustard-rt run`. Nomes de comando, opções e chaves ficam como são.

---

## O fluxo

Todo trabalho que muda arquivo passa por um fluxo só. Cada passo é uma chamada ao binário, e a resposta de cada chamada diz qual é o próximo passo: o modelo repassa essa resposta. O binário executa operações mecânicas: estado, recuperação, contexto, cálculos, validação e geração de páginas. O modelo interpreta, toma decisões que exigem raciocínio e implementa código. Operação mecânica ausente deve virar proposta de comando, não um script improvisado em cada conversa.

```mermaid
flowchart LR
    A["open"] --> B["grill"] --> C["plan"] --> D["aprovação<br/>(clique)"] --> E["round"] --> F["close"] --> G["pr-open"]
    G -.->|"só a pedido"| H["pr-merge"]
```

| Passo | Comando | O que faz |
|---|---|---|
| 1. Abertura | `mustard-rt run open --kind <tipo> --name "<nome>" --base <base>` | Cria a branch `<tipo>/<nome>` e a spec com o mesmo nome. O que falta volta como pergunta, com os candidatos. `--pending P-<n>` liga a spec à pendência de onde ela veio. |
| 2. Levantamento | `mustard-rt run grill --spec <spec>` | Grava o tipo de trabalho e os pontos da lista, abertos, com os fatos que ela traz. Sem `--kinds`, vale o tipo gravado e, na primeira vez, o do começo da branch; o pedido misto diz os tipos, como `--kinds feature,fix`. `--condensed` junta todos os pontos num bloco só, para um pedido que cabe numa frase. O assistente soma os fatos que conferiu com `write point` levando só `replaces` e `facts`; cada resposta vai pelo `answer`, que grava o item, fecha o ponto e devolve o próximo. |
| 3. Plano | `mustard-rt run plan --spec <spec>` | Confere o plano, prepara o pedido canônico e faz a pergunta de aprovação. Não gera nem sincroniza página externa; projeto e specs são acompanhados pelo painel local. |
| 4. Aprovação | nenhum | O clique em "Aprovar" é gravado pela testemunha. "Ajustar" não aprova. |
| 5. Ondas | `mustard-rt run round --spec <spec>` | Integra os retornos autorizados, executa build e provas pertinentes, comita e despacha dependentes liberados. Lint e suíte geral ficam para o fechamento. Perfis de tarefas e julgamentos complementares válidos são reutilizados; dependências, reservas e prioridade são calculadas localmente. Jev só julga candidatos ambíguos. `--report` recebe consumo/pausas; a entrega é gravada pelo agente com `write delivered`, com envio aberto. |
| 6. Fechamento | `mustard-rt run close --spec <spec>` | Executa lint, suíte geral e critérios sobre o código integrado, registra o resultado e reaproveita validação vigente. Mudança de conteúdo, comando ou ambiente invalida o recibo. Falha abre trabalho rastreável de conserto. A revisão final confere resumos, código, diff e critérios, sem alterar a obra. Só fecha com revisão aprovada e validação válida. `--report` recebe linhas da rodada; `--pending-later "P-<n>=<motivo>"` registra o destino de cada pendência. |
| 7. Pull request | `mustard-rt run pr-open --base <base> --head <branch> --spec <spec>` | Envia a branch ao servidor (o envio recusado para o comando, sem abrir nada), monta o título e o corpo a partir da spec e abre o pull request; o que já existe só tem o corpo reescrito. Com submódulo mexido pela spec, abre antes o de cada submódulo e o do principal como rascunho, que fica pronto quando eles entram (`pr-merge --root <submódulo>` ou o início da sessão). `--draft` abre como rascunho; `--fill` monta título e corpo pelos commits, para o repositório sem spec. |
| 8. Merge | `mustard-rt run pr-merge --pr <n>` | Só quando o usuário pede. Sem veredito aprovado, pergunta e não mexe em nada; `--confirm` é a resposta. |

A entrega da onda mora na spec: o agente a grava com `mustard-rt run write delivered`, e só com o envio da onda aberto — sem ele, a gravação é recusada com `no-open-send`, sem gravar nada. A gravação já confere o título do commit (até 60 caracteres), o resumo com cara de SHA, o arquivo que o projeto não conhece, a prova nova e a leitura do pedido (recusa `delivery-read-missing`, nomeando o item, se algum item que o pedido da onda lista não foi lido com `run read` de dentro da cópia), e grava a entrega como volta, escondida da leitura até a rodada assumi-la. A entrega traz `wave`, `text`, `files` e `commit` — o resumo de onde a mensagem do commit é montada, exigido quando há arquivo —, e ainda `proofs` (o critério e o comando de cada teste novo), `fixes` (as ondas que um conserto fecha), `replan` (a mudança, quando o plano da onda não funciona; ela só segue com o clique em "Aceitar"), `undone` (o código de cada tarefa da onda que o agente não fez, obrigatório com `replan` — `[]` quando fez todas —, e só com tarefa da própria onda; com a volta assumida, cada uma volta ao backlog sem a onda, a mudança aceita fica anotada na parte do agente dela, e a resposta da rodada avisa com `tasks-returned`) e `leftovers` (cada sobra com `title` e `detail`, que a rodada grava como tarefa da spec, no backlog; a sobra que só muda comentário, documentação ou texto de ajuda leva `cleanup` (`true`), e a rodada a segura até o fim da obra, junto das outras limpezas numa onda só; o `kind` de uma volta antiga é ignorado). A rodada assume a volta: grava a entrega oficial, com `replaces` apontando as voltas desde o último envio, e comita com o resumo dela. O relatório da rodada são as linhas que o orquestrador escreve, uma por linha, quantas forem: `<PAUSED>{…}</PAUSED>` da onda que pausou e `<USAGE>{…}</USAGE>` da onda cujo agente terminou. O resto do texto não é lido; um relatório sem nenhuma dessas marcas é recusado, e a linha `<DELIVERED>` no relatório também, sem gravar nada. O envio grava os itens que ficaram e, à parte, em `analysis`, o que o Jev tirou e o que pôs, cada um com a chance dele (`Jev p=0.03`). O veredito é do agente de teste dedicado, que o fechamento pede a toda obra, e ele o grava com `mustard-rt run write verdict`, só com o pedido de revisão aberto e depois de ler, de dentro da cópia, cada item que esse pedido lista (senão recusa `verdict-read-missing`, nomeando o item); a linha `<VERDICT>` no relatório é recusada, como a de entrega. O veredito traz `result` (`approved` ou `rejected`) e `final:true`; a aprovada é a única que vem sem `wave`, e a reprovada traz nele a onda que o conserto refaz. Quando o agente de uma onda termina, o orquestrador escreve a linha `<USAGE>{"wave":1}</USAGE>`, só com a onda: o consumo — o modelo, os passos e os tokens do agente, e os da conversa principal no ramo da spec — a rodada mede nos arquivos de conversa que a plataforma grava, e avisa a onda cujo arquivo não acha. Nenhum número digitado, na linha ou na entrega, vira consumo, e a entrega com o campo de consumo é recusada. A linha de consumo sozinha completa a onda que já voltou; a de uma onda de lote com envio aberto, sem entrega gravada e com o Claude Code dela fechado marca a onda cortada, e as tarefas dela voltam para o backlog. O fechamento lê o relatório da última rodada pela mesma porta.

---

## Comandos de barra

| Comando | O que faz |
|---|---|
| `/mustard:continue` | Retoma a spec pelo `resume` e repassa o próximo passo. É o botão de reserva: a retomada já acontece no início da sessão. |
| `/mustard:measure` | Roda o `measure` e repassa a frase do veredito: o gasto do Claude no projeto antes e depois da marca da versão. |
| `/mustard:pr` | Abre o pull request, revisa o de um colega ou faz o merge, só a pedido. |
| `/mustard-panel` | Abre projeto, specs, execução, consumo e dados da statusline nos Mods. Eventos atualizam a projeção; consulta a cada 2 s cobre mudanças externas. Sem turno de modelo. |
| `/mustard-pages` | `project` publica a visão geral sem exigir spec aberta; `spec` usa a spec da branch atual; `spec <nome>` escolhe outra; `report <arquivo.md>` publica análise/resumo pronto no mesmo layout. Sem destino configurado, entrega arquivos locais. Não inicia turno do modelo. |
| `/mustard:upsert` | Instala ou atualiza o Mustard no projeto, e diagnostica a instalação. |

Não há comando de entrada: um pedido que muda arquivo, dito na conversa, abre a spec.

---

## O que chega à janela

- **O mapa do início da sessão** (`.claude/mustard/session-map.md`, até 3 kB): o fluxo, quando uma spec abre e onde cada coisa mora. Entra no início da sessão, depois de `/clear` e da compactação.
- **A linha de cada mensagem** (até 100 caracteres): o idioma do texto e "texto simples". Depois de uma resposta com erro de escrita, ela leva mais uma frase curta com o erro, como "Na última resposta: frase com 29 palavras.", uma vez só.
- **O estilo de resposta** do plugin, um por idioma (`mustard:mustard-pt-BR`, `mustard:mustard-en-US`), escolhido pelo instalador na chave `outputStyle` do `.claude/settings.local.json`.
- **Os dois agentes**, em `.claude/agents/mustard/`, no idioma do texto: `wave` implementa uma onda, e `review` confere a obra integrada no final, um levantamento ou o pull request de um colega. Resumos orientam a investigação; as conclusões são conferidas no código. O modelo e o esforço de cada um vêm de `agents.model` e `agents.effort` do `mustard.json`; sem esses campos, a instalação os grava com `sonnet` e `xhigh`.

---

## Os ganchos

| Gancho | Quando | O que faz |
|---|---|---|
| `session_start_inject` | início da sessão | Coloca mapa, retomada e avisos até 3 kB, incluindo o comando do painel local. Não prepara páginas nem pede sincronização/publicação externa, inclusive de gasto. |
| `statusline_heal_observer` | início da sessão | Conserta a barra de status. |
| `prompt_entry` | cada mensagem | Grava a mensagem na spec atual; depois de uma resposta com erro de escrita, coloca a linha curta com o erro, uma vez. |
| `write_gate` | antes de arquivos e depois de buscas | Conserva as proteções de escrita, segredo e leitura. Grep/rg executam com seus argumentos originais; compactação posterior só usa resultados executados verificáveis, sem Jev de rotina e sem transformar resultado em instrução. |
| `command_guard` | antes de um comando | Recusa comando que apaga trabalho. |
| `subagent_inject` | antes de despachar um agente | Troca o bilhete `MUSTARD-WAVE: <spec> <n>` pelo pedido montado da onda. |
| `approval_witness` | depois de uma pergunta com opções | Grava o clique em "Aprovar" ou "Aceitar". |
| `end_of_turn_check` | fim da resposta | Confere a escrita: o erro achado vai na linha da mensagem seguinte, sem barrar. Barra quando a resposta sai noutro idioma que não o do projeto, para o assistente escrevê-la de novo, e quando a spec fecha ou entra no merge e a resposta não cita uma pendência aberta nascida nela. |
| `session_cleanup_observer` | fim da sessão | Solta a spec da sessão. |

---

## Os comandos `mustard-rt run`

Quase todos aceitam `--root <pasta>`, que diz de que pasta o repositório é lido; sem ela, vale a pasta atual. Não a aceitam `clean`, `upsert`, `doctor` e `statusline`, que trabalham sempre na pasta atual. Os que trabalham numa spec aceitam `--spec <spec>`; sem ela, vale a spec atual.

| Comando | O que faz |
|---|---|
| `read <bloco>` | Devolve um bloco da spec: `state`, `specification`, `agreed`, `waves`, `wave-<n>`, `criteria`, `review`, `progress`, `notes` ou `conversation`. `--term <termo>` filtra pelo termo ou pelo código do item. |
| `write <tipo>` | Grava um evento: `mustard-rt run write point --spec <spec> --json '{…}'`. `lesson` grava no banco de lições. `--copy` é compatibilidade: informa que a publicação automática foi retirada. Links/recibos históricos continuam legíveis. Quando obrigatório, `origin` recebe a última mensagem do usuário se estiver ausente. |
| `answer` | Grava a resposta de um ponto do levantamento e fecha o ponto numa chamada só: `mustard-rt run answer --point <número ou código> --type <tipo> --json '{…}'`. Sem `--point`, vale o primeiro ponto aberto. `--result <itens>` aponta itens já gravados, junto do item novo ou sozinhos; `--not-applicable --reason "…"` fecha o ponto sem item. O item passa pelas mesmas recusas do `write`, e o item recusado não fecha o ponto; sem `origin`, ele e o fechamento apontam a última mensagem do usuário. Devolve o item, o fechamento e o próximo ponto. |
| `resume` | A fase, o próximo passo em palavras e o comando que o faz. |
| `reopen` | `mustard-rt run reopen --reason "<motivo>"` reabre a spec para um pedido novo: a que ainda não fechou volta ao levantamento; a fechada ou com o pull request aberto volta à execução, já aprovada, na mesma branch, e o pedido novo entra pelo `write request`; a entregue e a descartada são recusadas. Com `--fix`, é a porta de conserto do pull request que o servidor reprovou: só com o vermelho relatado, abre a onda de conserto dentro da mesma spec e, com ela entregue e comitada pela rodada, empurra a branch para o servidor — sem reabrir a obra e sem spec nova. |
| `discard` | Descarta a spec em duas chamadas: a primeira mostra o que sai e devolve um código; a segunda, com `--confirm <código>`, faz. `--remote` apaga também a branch do servidor; `--delete` apaga a pasta da spec em vez de guardá-la. |
| `pending` | A lista de pendências. `--add --title "…" --detail "…"` acrescenta; `--close P-<n> --reason "…"` dá como entregue; `--remove` com `--id`, `--term` ou `--before <dia>` tira em duas chamadas, a segunda com `--confirm <código>`; `--drop P-<n>` é a mesma saída para um item; `--reopen P-<n>` traz de volta; `--stale` mostra as paradas há 30 dias e `--expire --keep P-<a>,P-<b>` tira as outras. |
| `index` | Refaz o índice das specs. |
| `scan` | Atualiza o mapa do projeto, lendo só o que mudou; nunca escreve no git. `--full` refaz o mapa de cada subprojeto; `--out <caminho>` grava o modelo em outro lugar. |
| `map <pergunta>` | Pergunta ao mapa: `examples` (`--file <arquivo>`), `importers --file`, `tests --file`, `slice --file <arquivo> --name <declaração>`, `users --name <declaração>` (quem usa a declaração, como `arquivo:linha:quem chama`; com `--file`, só a desse arquivo), `search "<padrão>" [<pasta>]` (o mesmo texto que o `Grep` recebe, com `-i` (`--ignore-case`), `-w` (`--word-regexp`), `-F` (`--fixed-strings`), `--glob` e `--type`; recuperação local e julgamento complementar de candidatos ambíguos, quando habilitado; busca literal pelo Grep/rg não chama Jev) e `summary`. |
| `page` | Gera HTML local no layout do Mustard a partir de markdown: `--body <pagina.md> --out <pagina.html>`, com `--title`, `--subtitle`, `--kind`. Não publica remotamente. |
| `panel` | Consulta projeto, specs, ondas, validação, etapas e consumo conhecido em JSON: `--root <projeto> --spec <spec>`. Não gera página nem chama scan, Jev ou modelo. |
| `publish` | Gera HTML/JSON/manifesto e publica pelo Cloudflare configurado. Sem seletor, usa a spec da branch atual; `--spec <spec>` escolhe uma, `--project` gera visão geral mesmo sem spec, `--document <arquivo.md>` publica relatório no layout comum. `--include-consumption` autoriza incluir consumo. Só por pedido explícito; retorna `published:false` enquanto não houver envio remoto confirmado. |
| `clean` | Lista as cópias descartáveis que os agentes deixaram no diretório temporário. `--dry-run` só lista, que é o padrão; `--apply` apaga as listadas; `--path <pasta>` apaga só aquela pasta. |
| `pr-review` | Sem número, lista os pull requests abertos da base; `--pr <n>` mostra o pedido de revisão. Não grava veredito: `--verdict` recusa na entrada, porque o veredito de cada onda é gravado pela rodada. |
| `upsert` | Instala ou atualiza. Na mesma chamada, tira as sobras do Mustard antigo nos `CLAUDE.md` e no `settings.json` da equipe, com as regras das Guards indo antes para um item só da lista de pendências, troca no lugar as regras de bloqueio da equipe escritas errado, e diz o que saiu e o que mudou; o arquivo sem marca só aparece na lista. Enquanto o `mustard.json` não tem `localFiles`, responde `localFilesFound`, os arquivos que o git ignora fora das pastas ignoradas; `--local-files <a,b>` grava a lista confirmada e `--prepare <comando>` o comando que prepara cada cópia, e o valor vazio grava que não há. |
| `doctor` | Diagnóstico só de leitura. `--check <nome>` roda uma conferência; `--residue` procura também referências mortas; `--format json` ou `--json` responde em JSON. |
| `spend` | Conta localmente conversas por dia/projeto, sem spec aberta. Deduplica mensagens, guarda dias fechados e recalcula hoje. Sem opções, devolve resumo e não prepara página ou lote externo. `--publish` gera/publica explicitamente um snapshot de todos os dias; `--republish` mantém compatibilidade com o pedido de republicação. `--url <endereço>` registra um endereço confirmado. Cloudflare Pages recebe o snapshot completo pelo binário se configurado; exportação local não confirma publicação. |
| `measure` | O gasto do Claude no projeto antes e depois da marca da versão: a primeira sessão de cada compilação do Mustard no projeto, que o início da sessão grava. Compara os dias contados (fechados, com 100 ações ou mais; o dia da marca fica fora) dos dois lados, pela mesma conta do `spend`, e só afirma com 5 de cada lado. Responde `project`, `mark`, `before`, `after`, `verdict` e o veredito numa frase em `text`; sem marca, diz que a medição começa na próxima sessão. `--since <instante>` (RFC 3339 ou `AAAA-MM-DD`) mede a partir dele. Só lê: não grava nada e não chama o Jev. |
| `statusline` | Projeção compacta de spec/ondas, comandos do painel, validação e métricas conhecidas; preserva links históricos. Só consulta estado, sem scan, Jev ou publicação. `--preview` mostra os temas. |

---

## A instalação

`mustard-rt run upsert` grava, escondidos do git do projeto:

- `.claude/settings.local.json`, `.claude/.gitignore` e `mustard.json`, que são da pessoa: o que existe fica, e só o que falta é acrescentado;
- o mapa `.claude/mustard/session-map.md` e os dois agentes em `.claude/agents/mustard/`, no idioma do `language.text`. Esses são textos do próprio Mustard: toda execução regrava o texto embarcado, então uma cópia editada volta em `updated`, e uma idêntica volta em `preserved`, porque não havia o que escrever.

O `.claude/settings.local.json` recebe as liberações do próprio Mustard, na instalação nova e na atualização: os comandos `mustard-rt run` e a pasta das cópias das ondas, que mora fora do projeto, em `permissions.additionalDirectories`. Não acrescenta permissão de publicação externa para o modelo. As liberações que a pessoa já tem ficam como estão.

Uma instalação antiga que declarava as três partes do roteador (`orchestrator.md`, `dispatch.md` e `material.md`) passa a declarar o mapa no lugar delas, e os três arquivos saem do disco. Uma instalação com o mapa de nome antigo, `mapa-inicio-sessao.md`, passa a declarar o `session-map.md` no `mustard.json`, sem mudar o resto dele, e o arquivo antigo sai do disco. Templates automáticos de projeto/spec não são mais distribuídos ou semeados; páginas históricas não voltam a ser publicadas pela instalação. Nada é commitado.

## Publicação externa nativa

O destino recomendado é **Cloudflare Pages Direct Upload**. Configure uma vez
um projeto Direct Upload no painel da sua conta (sem integração automática de
Git) e um token com permissão **Account → Cloudflare Pages → Edit** restrito à
conta escolhida. Disponibilize `CLOUDFLARE_API_TOKEN` ao processo do Claude Code
ou ao terminal do binário; não grave o token em `mustard.json`, na spec ou no Git.

Acrescente ao `mustard.json`, preservando as configurações existentes:

```json
{
  "publication": {
    "provider": "cloudflare-pages",
    "accountId": "0123456789abcdef0123456789abcdef",
    "projectName": "mustard-status"
  }
}
```

Os nomes acima são exemplos. `accountId` é o identificador real da conta,
e `projectName` é o projeto Direct Upload já criado. A instalação não cria
contas, projetos remotos ou credenciais.

O bloco `publication` é opcional e pertence ao `mustard.json` da raiz do projeto.
Instalação, atualização e `mustard config` preservam seus campos e as demais
configurações existentes. O binário informa separadamente arquivo ilegível,
destino inválido e token ausente, antes de tentar comunicação remota. O token
é lido exclusivamente de `CLOUDFLARE_API_TOKEN` no ambiente; um campo de token
no JSON não configura a autenticação.

- `mustard-rt run publish --project` ou `/mustard-pages project` publica o projeto
  sem exigir uma spec aberta.
- `mustard-rt run publish` ou `/mustard-pages spec` usa a spec da branch atual.
  `--spec <spec>` ou `/mustard-pages spec <spec>` seleciona outra. Consumo da spec só entra com `--include-consumption`.
- `mustard-rt run publish --document resumo.md` ou `/mustard-pages report resumo.md`
  publica o Markdown solicitado, com título inicial `# Título`, no layout comum.
  O conteúdo pode ser uma análise ou resumo para gestores. O modelo raciocina
  sobre o conteúdo quando necessário; formatação e envio são do binário.
- `mustard-rt run spend --publish` publica o consumo agregado da máquina. Esse
  pedido explícito inclui os nomes dos projetos e os valores por dia.
- Só `published:true` com `remote_url` confirma uma página pronta. Se `pending:true`,
  repita o comando para consultar a publicação aceita; isso não cria outra.
- Sem configuração/token ou em caso de erro, os arquivos continuam locais e
  `published:false`. Uma URL histórica não confirma o envio do snapshot atual.

Cada publicação envia somente `index.html` e `snapshot.json`, com o layout
comum do Mustard. O JSON é a base estática da publicação; não existe banco
remoto recebendo atualizações automáticas. O binário verifica os arquivos que
já existem no servidor antes de enviar e guarda o identificador aceito para
retomar a consulta. Não utiliza modelo, Jev, Python ou Wrangler para publicar.

O link retornado corresponde à versão publicada. Atualizar exige outro pedido
explícito. O painel local continua consultando o estado atual sem publicar.
Nenhum comando de publicação exclui páginas anteriores; retirar acesso ou
apagar recursos remotos requer operação separada na conta do usuário.

Protocolo: [Direct Upload](https://developers.cloudflare.com/pages/get-started/direct-upload/)
e [API de deployments](https://developers.cloudflare.com/api/resources/pages/subresources/projects/subresources/deployments/methods/create/).

Os Mods observam `tool.call` após a execução, `turn.complete`, `session.compact`
e `classic.SubagentStop` para antecipar consultas locais com coalescência de
200 ms. `session.measure` fornece contexto, custo informado e limites diretamente
do host. Observar um evento não acrescenta um novo recibo de entrega/validação:
o histórico canônico permanece no binário. Hooks clássicos continuam aplicando
as validações nativas. Fechar o painel cancela seus timers.

O nome de comando nos Mods é `/mustard-pages`: a interface atual aceita letras,
números, hífen e sublinhado, sem `:`. Isso permite execução direta sem transformar
a publicação mecânica em um comando Markdown que consome turno do modelo.
Relatórios atuais publicam texto/tabelas do Markdown; anexos em arquivos locais
não são enviados implicitamente. A leitura do arquivo é explícita e não inclui
outras conversas, código ou a spec inteira por conta própria.

## Teste da versão de desenvolvimento no Claude Code

O aceite do painel e dos eventos deve ser realizado numa sessão autenticada do
Claude Code. Na raiz do projeto de teste, prepare os arquivos locais com o CLI
do pacote em revisão e inicie o Claude com esse pacote:

```sh
mustard_review_plugin="/caminho/do/checkout/target/review-plugin"
"$mustard_review_plugin/bin/mustard" init --yes
PATH="$mustard_review_plugin/bin:$PATH" claude \
  --plugin-dir "$mustard_review_plugin" \
  --settings '{"enabledPlugins":{"mustard@inline":true}}'
```

Use o caminho real do pacote de revisão; o `mustard` instalado no sistema pode
pertencer à versão anterior. O PATH informado vale para o Claude e seus filhos.
O manifesto do Mustard declara `defaultEnabled:false`, portanto a configuração
`mustard@inline:true` habilita a cópia local somente nessa sessão, sem gravar
habilitação nas configurações pessoais.

No Claude, confira o plugin em `/plugin` e abra `/mustard-panel`. Peça uma
alteração pequena pelo fluxo normal: levantamento, aprovação do plano, ondas,
validação e revisão final. Observe a atualização do painel, os dados de consumo
e a statusline durante as ferramentas e após a retomada. A publicação externa
continua exigindo pedido explícito e pode ser validada separadamente.

Registre a versão retornada por `mustard-rt --version` e o resultado de cada
etapa. Uma sessão que não carregou `mustard@inline` ou usa outro executável não
comprova o comportamento desta entrega. Fonte:
[carregamento de plugins](https://code.claude.com/docs/en/plugins/loading).
