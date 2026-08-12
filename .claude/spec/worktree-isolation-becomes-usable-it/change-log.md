# Change Log — worktree-isolation-becomes-usable-it

_Solicitações registradas automaticamente durante o pipeline (mid-spec). O `spec.md` (narrativa congelada) NÃO é alterado; dobre o que muda comportamento em `## Acceptance Criteria` e rode o QA de novo._

- **2026-08-12T02:51:07.691Z** _(Plan)_ — ar
- **2026-08-12T07:03:37.722Z** _(Execute)_ — Segue
- **2026-08-12T11:15:07.084Z** _(QaReview)_ — não entendi, em resumo, não conseguiu fazer funcionar o que conversamos
- **2026-08-12T11:18:17.054Z** _(QaReview)_ — o que você está fazendo, gerando um link do node_module, de quem?
- **2026-08-12T11:51:41.982Z** _(QaReview)_ — o processo precisa ser simples, sem atrito, como o próprio claude faz, o meu questionamento é que ao criar um worktree, alguns arquivos de configuração não são copiados e está certo, só deveria ser avisado
- **2026-08-12T11:55:02.178Z** _(QaReview)_ — sim, mas não entendi o critico 2
- **2026-08-12T12:09:02.585Z** _(QaReview)_ — confesso que não sei entendo tanta burocracia. tudo deve começar em dev, até ai ok pra você? Eu peço alguma coisa, feature, bugfix, qualquer coisa, isso possivelmente vai gerar uma spec aqui começa o problema. Isso deve gerar uma branch nova com nome da branche dependendo do contexto da conversa, aqui mora um dos problemas a spec fica em dev ou dentro do branch? como o mustard está controlando essa questão da spec?
- **2026-08-12T12:11:09.151Z** _(QaReview)_ — confesso que não sei entendo tanta burocracia. tudo deve começar em dev, até ai ok pra você? Eu peço alguma coisa, feature, bugfix, qualquer coisa, isso possivelmente vai gerar uma spec aqui começa o problema. Isso deve gerar uma branch nova com nome da branche dependendo do contexto da conversa, aqui mora um dos problemas a spec fica em dev ou dentro do branch? como o mustard está controlando essa questão da spec? O comando spec não faz mas sentido aqui, já que cada spec é uma branch então spec já seria aprovação e inicio imediato só deveria perguntar se vai seguir com o modelo e effort após o passo acima ai sim seria o caso de criar um worktree e questionar se vai copiar os arquivos de configuração que poderia ficar configurado no mustard.json para uma próxima vez pegando do dev essa configuração
- **2026-08-12T15:37:21.900Z** _(QaReview)_ — ou seja voltamos ao inicio, não faz sentido então ter o worktree
- **2026-08-12T15:39:23.438Z** _(QaReview)_ — acho que deveria sair tudo de worktree não faz sentido mais
- **2026-08-12T15:40:39.562Z** _(QaReview)_ — **Instruction:** Remover TODA a maquinaria de worktree que o mustard adiciona para isolar unidade de trabalho. Sai: a secao worktree do mustard.json (carry/link), carry_environment, copy_tree, link_dir e a junction do Windows, o desvio do portao de escrita para cortar worktree, e a prosa que ensina esse arranjo. No lugar, quando o checkout ja segura o branch de OUTRA unidade com trabalho nao commitado, o portao RECUSA e nomeia o que fazer (commitar ou guardar antes de abrir a segunda unidade) em vez de desviar. Motivo: o processo tem de ser simples e sem atrito; copiar ou ligar ambiente para dentro de um worktree e maquina demais para o que se pediu, e a junction destroi o diretorio do checkout principal quando o worktree e removido (reproduzido com e sem --force). FICA de pe o coletor de worktrees orfaos: ele nao cria nada, apenas recolhe os que o Claude Code corta por conta propria, que aparecem no repositorio com ou sem o mustard.
- **2026-08-12T15:42:46.008Z** _(QaReview)_ — vc usa girias e termos que não dá pra entender
- **2026-08-12T15:47:06.687Z** _(QaReview)_ — sim
- **2026-08-12T16:51:09.378Z** _(QaReview)_ — uma melhoria, hoje os branches deveriam ser criados assim: fix/... - feature/... - hotfix/
- **2026-08-12T17:03:39.225Z** _(QaReview)_ — fix e feature a partir da develop ou dev deacordo com mustard.json e hotfix de qas ou produção/main
- **2026-08-12T17:09:02.005Z** _(QaReview)_ — unico ponto que não entendi é sobre hotfix que pode sair de qas ou main/produção como será identificado?
- **2026-08-12T17:20:18.044Z** _(QaReview)_ — e se fosse questionado ao usuário antes de decidir como opção?
- **2026-08-12T17:22:03.791Z** _(QaReview)_ — mas ai terei que pedir ou inicio a conversa com o termo que eu quero feature, fix, hotfix...?
- **2026-08-12T17:45:22.901Z** _(QaReview)_ — mas deveria ser algo que sempre deveria ser questinado ao usuário, porém, trazendo a opção padrão de acordo com o cenário, mas deixando ele escolher qualquer um e de qual branch será iniciado
- **2026-08-12T17:51:13.272Z** _(QaReview)_ — isso, aprovado
- **2026-08-12T18:36:11.134Z** _(QaReview)_ — sim
