# Desenho: Porta única (roteia + confirma na dúvida)

> Decisão de entrada (travada): **sem comando para o trabalho.** O usuário descreve em linguagem natural; o orquestrador classifica, narra a leitura e confirma só na dúvida. `/mustard` sozinho = ajuda. Os comandos `/mustard:feature|bugfix|task|tactical-fix` continuam existindo como **override de poder**, mas deixam de ser anunciados como escolha do usuário.

## As 3 camadas (e a regra que mantém a IA sem confusão)

1. **USUÁRIO** — só descreve: *"adiciona importação de CSV de clientes"*, *"tá com erro ao importar"*, *"como funciona o cálculo de juros?"*.
2. **ORQUESTRADOR / roteador** — o `CLAUDE.md § Intent Routing` é a **FONTE ÚNICA** de "intenção → fluxo interno". Ele:
   - (a) **classifica** a intenção (nova funcionalidade / mudança / correção / investigação) + escopo grosso, usando o que já existe (`scope-classify` determinístico, e o roteador semântico do digest-validate quando o fluxo já abriu) — não é chute de LLM;
   - (b) **SEMPRE narra a leitura** — *"Tratando como uma correção de bug."* / *"Entendi como mudança pequena (caminho leve)."* — transparência + você pode interromper;
   - (c) **PERGUNTA só na ambiguidade genuína** — fork real (bugfix-vs-feature, light-vs-full no limite, pedido vago) → uma pergunta. Caso óbvio → segue (economia de roteamento);
   - (d) **despacha o fluxo interno** + emite a classificação como evento (`kind`) → alimenta o dashboard (liga no item #3).
3. **FLUXOS INTERNOS** (skills feature/bugfix/task/tactical-fix) — mantêm os procedimentos intactos; só a **descrição** muda de *"Use when the user asks to add/fix…"* (gatilho de usuário) para *"Internal flow — dispatched by the orchestrator router; not chosen directly by the user."* → param de se auto-anunciar como escolha. `/mustard:feature` etc. seguem invocáveis (override), só não anunciados.

**"Nunca fazer sem questionar" — interpretado são:** você **sempre vê** a classificação (auditável + interrompível); a IA **pergunta** quando é um fork real. Não gateia o caso óbvio — senão vira a burocracia que não queremos.

## Doc em duas audiências (a sua preocupação central)

- **User-facing** (ajuda do `/mustard` + topo do README/guia): *"Diga o que você quer em palavras suas. O mustard descobre se é uma nova funcionalidade, uma mudança, uma correção ou uma investigação, te mostra a leitura, e pergunta se estiver na dúvida. Você não escolhe comando."*
- **Internal** (`CLAUDE.md § Intent Routing` + os SKILLs dos fluxos): o roteador (intenção → fluxo + escopo) e os fluxos (marcados internos). **Não misturar as duas** — é exatamente o que evita a IA se confundir sobre "qual fluxo estou rodando".

## O que muda nos arquivos

1. **`CLAUDE.md § Intent Routing`** → reescrito como o roteador explícito: classifica → **narra** → **confirma-na-dúvida** → despacha → **emite `kind`**. Vira a fonte única (já é, só fica explícito).
2. **Descrições dos SKILLs** feature/bugfix/task/tactical-fix → "internal flow, dispatched by router" (deixam de auto-disparar como escolha do usuário). Procedimento interno inalterado.
3. **`/mustard` (ajuda)** → uma entrada magra: "descreva o que quer". (Discoverability sem reintroduzir "o que eu digito".)
4. **Gancho de telemetria** → o roteador emite a classificação (`pipeline.kind`), determinístico, como side-effect — é o mesmo dado que o dashboard (#3/#4) precisa. **#2 e #3 se encaixam: o roteador decide o tipo E o emite.**

## Riscos + mitigação

- **Orquestrador não engata** (nada dispara) → ele sempre lê a mensagem e o `CLAUDE.md` manda rotear; **validar end-to-end** que linguagem natural ainda roteia após neutralizar o auto-trigger dos skills. Se preciso, manter um auto-trigger FRACO de fallback pra não estrandar trabalho.
- **Classifica errado** → o "narra + confirma-na-dúvida" pega (você objeta antes de rodar).
- **Confusão de doc** → o split de duas audiências é a mitigação; regra explícita "doc-de-usuário fala da porta; doc-interna fala dos fluxos".
- **Over-confirm (burocracia)** → a regra é "confirma só no fork real"; caso óbvio segue narrando, sem perguntar.

## Como construir (seguindo o SDD que você exige)

Isto é uma feature de verdade (mexe no orquestrador + descrições dos skills + 1 gancho de telemetria). Então **especifico via o próprio pipeline** (`/feature`, escopo a detectar) e implemento sob os gates — dogfood do "Mustard guia e não pula processo". Ordem natural: **#2 (porta) + #3 (emite kind) juntos**, porque a classificação do roteador É o dado do dashboard; depois **#4 (a aba Atividade)** consome.

## Não-faz (escopo)
- Não renomeia comando (continuam como override oculto).
- Não remove os fluxos (feature/bugfix/task seguem inteiros internamente).
- Não mexe no dashboard ainda (isso é #4, depende do `kind` de #3).
