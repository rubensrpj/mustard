# Revisão da busca, orquestração, especializações e painel do Mustard

O Mustard já reúne uma base técnica útil, mas ainda não há uma comparação que demonstre economia líquida e qualidade superior até a conclusão de uma spec. Há defeitos concretos entre componentes que funcionam isoladamente: uma skill explícita impedia anexar especializações, a preparação das cópias presumia um único layout Node, resumos eram descontados da capacidade da onda e o Desktop recebia uma instalação diferente da usada no terminal.

Base examinada: `main` em `230d55ac5a09b48a08873ed5a0b542616103e353`, versão 0.2.20. Correções em `codex/review-agnostic-orchestration`. O projeto Florestal em execução permanece intacto. As afirmações retiradas pelo usuário em relatos anteriores não foram tratadas como fatos novos.

## Resultado por área

| Área | O que o código entrega | Limite relevante |
| --- | --- | --- |
| Scan e banco | Arquivos, linguagens, declarações e intervalos, contratos sintáticos, imports, testes, recursos textuais, histórico, comentários de revisão, estrutura de pastas e índices derivados. SQLite com FTS5 e atualização de arquivos alterados. | Tipos de retorno e relações estáticas não provam o resultado de executar uma função. Dependências dinâmicas podem permanecer desconhecidas. |
| Busca | Execução nativa de Grep/rg/Read com os argumentos fornecidos, cruzamento com fontes atuais, buscas por símbolos, grafo e referências precisas quando disponíveis. Retorno nativo preservado. | Uma consulta literal sem resultado não pode ganhar correspondências semânticas apresentadas como se fossem ocorrências literais. Comandos de shell não compreendidos conservam seu comportamento. |
| Modelos locais | Vetores gerais e `potion-code-16M-v2`, embutidos, incrementais e habilitados por padrão, salvo `ai.vectors:false`. | Similaridade ajuda a recuperar candidatos; não certifica o papel de um arquivo, fluxo de negócio ou correção da implementação. |
| Haiku e Luna | Preparação e recebimento de interpretações pela sessão nativa, com fontes e regras vinculadas, reaproveitamento e invalidação. Preferência por família atual e esforço médio; horizonte solicitado de 100 mil tokens. | O binário prepara a delegação; o host precisa executá-la. Modelo, esforço e entrada efetivos só podem ser confirmados quando o host os informa. A execução autenticada ainda precisa de validação. |
| Jev | Porta `JudgementProvider`, cache por evidência/pergunta/provedor e classificações tipadas de busca, contexto e planejamento. Há abstenção e proteções determinísticas. | Ativação não prova benefício. O modelo não pode substituir dependências, reservas de arquivo, aprovação de escopo ou testes. |
| Ondas | Backlog, dependências, cópias isoladas, revisão vinculada ao diff, provas nativas, recuperação e resumos de continuidade. | Fingerprint comprova qual diff foi aprovado; não comprova a qualidade da leitura do revisor. Tarefa que apenas relata testes continua diferente de teste executado pelo binário. |
| Agentes e skills | Catálogo de navegação interno; procedimentos com fontes atuais e revisão antes da exportação. Formatos próprios de Claude e Codex. | Catálogo, exportação e seleção são etapas distintas de descoberta no host, leitura e uso efetivo. Uma contagem de agentes gerados não mede utilidade. |
| RTK | Reescrita de comandos compatíveis e histórico de redução da saída. | Redução estimada da saída do shell não equivale à redução do consumo total da sessão ou da assinatura. |
| Mods | Estado local, definições, pedidos, entregas, provas, consumo, configurações, ideias, perfis e publicação explícita. | Teste da árvore de interface não comprova que o Desktop remoto a desenhou. |

Fontes principais do código: `apps/scan/src/main.rs`, `packages/core/src/io/project_map.rs`, `packages/core/src/io/knowledge/catalog.rs`, `packages/core/src/io/knowledge/precise.rs`, `packages/core/src/io/code_vectors.rs`, `apps/rt/src/shared/search_gateway.rs`, `apps/rt/src/shared/judgement.rs`, `packages/core/src/io/expertise/` e `plugin/hooks/register.js`.

## Defeitos corrigidos no candidato

| Defeito confirmado | Alteração | Prova |
| --- | --- | --- |
| `package.json` obrigava qualquer preparo a criar `node_modules`. | Reconhece instalações convencionais e Yarn Plug’n’Play; comandos personalizados mantêm seu contrato. O preparo não exige layout Node de um projeto de outra linguagem. | Testes executam preparo personalizado, mudança em `go.mod`, Yarn com `.pnp.cjs`, mudança de linker e substituição segura de symlink. |
| Cache de preparo não abrangia vários manifestos e configurações. | Acrescenta entradas de Go, Maven, Gradle, Ruby, Composer, workspaces Node e configurações Yarn/.NET. | Alterar manifesto ou configuração invalida o recibo; entradas iguais reutilizam o preparo. A lista é um conjunto de adaptadores de ferramentas, não uma regra de negócio. |
| Uma tentativa de reparo podia deixar um recibo anterior reutilizável. | Invalida o recibo antes de preparar novamente; registra layout e fingerprint apenas após sucesso. | Teste cria dependências durante uma tentativa que falha e confirma que a próxima tentativa realmente executa o comando. |
| Skill explícita suprimia o procedimento especializado selecionado. | Anexa procedimentos revisados junto das skills explícitas, sem duplicar o mesmo arquivo. Navegação nativa não vira skill exportada. | Uma onda recebe simultaneamente skill manual e especialização nos formatos Claude e Codex. O corpo não é copiado para o pedido. |
| Uma especialização cobrindo parte dos arquivos impedia sugerir preparação para o restante. | Calcula os arquivos sem procedimento atual, respeita skills manuais e preserva `--file` nos argumentos. Um único arquivo também pode justificar um procedimento reutilizável. | Teste cobre área parcialmente atendida e solicitação com texto acentuado dentro do limite real de bytes do CLI. |
| O horizonte de 150 mil descontava resumos do contexto disponível. | Usa contexto total para decidir se pode começar outra tarefa. O desconto permanece apenas na estimativa de crescimento do trabalho. | Cenários em português e inglês com 145 e 160 mil tokens retornam para testes mínimos e entrega quando não cabe outra tarefa. |
| Reabertura do painel podia conflitar com eventos de fechamento e chamadas concorrentes. | Serializa aberturas, não encerra o novo poller por um fechamento originado no próprio plugin e preserva a intenção de abertura no descarregamento. | Testes cobrem reabertura, registro de desenho por abertura e um único timer após pedidos concorrentes. A pintura real no Desktop continua sendo outra verificação. |
| Diagnóstico só reconhecia o primeiro desenho por interface. | Registra o primeiro desenho de cada abertura. | Duas aberturas produzem dois registros de desenho. |
| Consumo misturava escopos e texto demais. | Primeiro mostra subtotal e cobertura da spec, consultas comparáveis e atrito; histórico RTK e custo Jev ficam identificados como dados do projeto. Expansão de saída fica explícita. | Testes conferem cobertura incompleta, resposta maior, ausência de economia líquida inventada e navegação para detalhes. |
| Painel terminal acumulava caixas; valores complexos viravam `[object Object]`; páginas cortavam frases desnecessariamente. | Terminal usa apresentação compacta sem caixas aninhadas. Objetos preservam conteúdo legível; paginação prefere parágrafos, linhas e palavras. | Testes preservam Unicode e a reconstrução integral do texto. |
| Prévia da statusline usava o nome de uma spec do Florestal. | Exemplo neutro `example-spec`. | Teste existente da prévia atualizado. O nome antigo não era usado para rotear execução. |
| `doctor` não aceitava `--root`, exigindo mudar o diretório do processo para consultar outro projeto. | Aceita a raiz explícita e preserva o comportamento anterior quando ela não é fornecida. | Integração consulta um projeto a partir de outra pasta e confere que nenhuma instalação foi criada. |
| Recuperação do cliente Prisma só reconhecia três nomes de script e presumia npm quando não achava o gerenciador. | Reconhece comandos de geração declarados com nome arbitrário, usa a detecção comum do gerenciador, inclusive `bun.lock`, e não escolhe entre geradores ambíguos. | Testes cobrem nome com espaços, comandos sem efeito de geração, gerenciador desconhecido e múltiplos geradores. Não há regra do Florestal nesse adaptador. |

## Desktop Windows com SSH

A versão informada do Desktop é 2.37337.1. No host Linux, o processo da sessão SSH usa Claude Code 2.1.295; o CLI local usa 2.1.296. Mais importante: os argumentos do processo SSH incluem explicitamente uma pasta do Mustard em `~/.claude/remote/plugins/…`, cujo manifesto e marca dos binários são **0.1.68** e que não contém `hooks/register.js`.

Essa instalação fornecida ao processo é diferente da versão 0.2.20 examinada. Portanto, atualizar apenas o plugin do CLI Linux não demonstra atualização da cópia fornecida pelo Desktop. A ausência de Mods nessa cópia é uma incompatibilidade concreta; identificar o argumento não comprova sozinho qual módulo ganhou precedência nem que o painel foi desenhado.

O candidato acrescenta `mustard-rt run doctor --check session-plugins --json`. Em Linux, ele identifica diretórios de plugin explicitamente fornecidos a processos Claude no mesmo projeto, informa versão e presença do módulo Mods, sem gravar prompts, demais argumentos ou credenciais. A consulta também aparece no Diagnóstico do painel. Em sistemas sem esse observador, a resposta permanece sem observação, em vez de declarar instalação saudável.

O próximo teste do Desktop deve começar pela versão do plugin no gerenciador de plugins do cliente que abre a conexão SSH. Depois, uma nova sessão deve confirmar a pasta efetiva, `/mustard-panel --diagnose`, as interfaces conectadas e um desenho `desktop` após abrir e reabrir. Trocar arquivos de uma sessão de ondas ativa não faz parte desse procedimento.

A documentação oficial permite plugins em [sessões SSH do Desktop](https://code.claude.com/docs/en/desktop#ssh-sessions) e mostra Mods na [aba Code do Desktop](https://code.claude.com/docs/en/plugins/mods#where-mods-run). O caso é diferente do Remote Control móvel/web, cujo painel permanece no terminal de origem.

## Como aproveitar Jev, Haiku e Luna com critério

O caminho preferencial é: resolver contratos, símbolos, dependências e evidências por código; recuperar uma análise válida; fazer uma classificação curta somente quando sobrar uma alternativa relevante. A [documentação Jev](https://docs.typesafe.ai/introduction) recomenda julgamentos atômicos sobre um estado claro, compostos pela lógica da aplicação. [Confiança](https://docs.typesafe.ai/patterns/confidence-routing) orienta abstenção e encaminhamento, não transforma escolha em prova.

Jev é adequado para comparar procedimentos revisados, classificar uma tarefa ambígua, apontar provável interferência semântica entre tarefas já relacionadas e escolher contexto entre candidatos atuais. Não deve reconstruir o projeto inteiro, gerar skills, aprovar código ou decidir autorização de acesso. Dependência explícita e conflito de escrita são resolvidos antes dele.

O [exemplo oficial de sugestão de skills](https://docs.typesafe.ai/cookbooks/skill_suggestion) usa descrições, uma seleção e uma verificação curta, permite não escolher nada e preserva a decisão do agente. Seus resultados são do catálogo e harness avaliados pelos autores; não são resultados do Mustard. Uma avaliação própria deve incluir casos sem skill adequada, procedimentos com o mesmo escopo e solicitações em português. A [documentação de estado](https://docs.typesafe.ai/concepts/state) informa que idiomas diferentes do inglês podem ter menor acurácia.

Haiku e Luna servem para interpretar um conjunto pequeno de fontes que a busca já localizou: responsabilidade, contratos, padrões e dúvidas. A interpretação deve trazer referências e fingerprint, sem se tornar regra obrigatória. Depois de concluída, deve ser reaproveitada entre specs/hosts enquanto as fontes e regras permanecerem válidas. Preparar o recibo não equivale a executar o modelo; declarar preferência também não prova modelo/esforço efetivos.

Não há motivo para chamar Jev para uma leitura exata ou Haiku/Luna para enumerar arquivos, contar referências, executar testes e montar relatórios que o binário consegue gerar. O binário deve continuar responsável por essas operações.

## Agentes e skills que ajudam uma onda

O procedimento especializado precisa ensinar algo não óbvio sobre a tarefa e o projeto: ponto de entrada, contrato, padrão local, exemplo atual, limites e validação pertinente. Pastas parecidas ou nomes de arquivos não bastam. Uma especialização de banco ou React deve surgir das fontes reais, sem categorias de negócio fixadas no Mustard.

O fluxo deve distinguir descoberta nativa, procedimento candidato, revisão, exportação, descoberta no host, seleção e uso. O candidato corrige a anexação e a cobertura parcial; a preparação por modelo continua opcional. Não existe garantia de que o host gerará e usará um agente novo em toda onda, nem seria econômico impor isso.

Os [subagentes Claude](https://code.claude.com/docs/en/sub-agents) têm contexto próprio e podem receber o conteúdo completo de skills declaradas. As [skills](https://code.claude.com/docs/en/skills) favorecem carregamento progressivo. Os [subagentes Codex](https://learn.chatgpt.com/docs/agent-configuration/subagents) também têm custo próprio; a documentação recomenda escopos estreitos e cautela com escrita paralela. Portanto, o ganho deve vir de menos exploração repetida e menos retrabalho, e não do número de agentes criados.

Para uma alteração pequena e já compreendida, uma onda adicional pode custar mais que agregar a tarefa à mesma entrega. Para alteração independente com muita exploração, uma especialização isolada pode ajudar. As regras obrigatórias e a reserva da cópia continuam válidas nos dois casos. Selecionar uma skill por caminho deve ser avaliado também pelo propósito: dois procedimentos para os mesmos arquivos podem ser complementares, e não alternativas exclusivas.

## O que falta provar sobre o scan e a economia

A estrutura atual não precisa ser substituída por um banco vetorial externo para começar a funcionar bem. Ela combina índices lexicais, estrutura, grafos, vetores e referências precisas. O [SCIP](https://github.com/scip-code/scip/blob/main/docs/scip.md) permite vínculos exatos entre posições e símbolos; o importador atual contempla intervalos tipados e o formato anterior. A [técnica de mapa do Aider](https://aider.chat/docs/repomap.html) reforça a seleção de contexto por relevância e espaço disponível. O [Model2Vec](https://github.com/MinishLab/model2vec) é uma recuperação estática leve, sem a compreensão de fluxo de um modelo gerador.

A prova necessária é de tarefa concluída. Deve comparar consultas equivalentes e o trabalho posterior, registrando:

1. Entrada correta localizada e fontes atuais entregues, incluindo casos sem resposta.
2. Referências úteis e não pertinentes, corpos necessários completos e expansões seguintes.
3. Latência fria/quente e consultas que repetem exploração já resolvida.
4. Consumo observado de orquestrador, agentes e análises, separado por entrada nova, cache e saída.
5. Entregas verificadas, retornos impedidos, retrabalho e custo adicional Jev.

O benchmark não pode usar o próprio índice como verdade de referência. Precisa de fontes rotuladas separadamente, projetos e consultas não usados no ajuste e escopo equivalente entre os braços. O [RTK](https://github.com/rtk-ai/rtk/blob/develop/docs/guide/analytics/gain.md) mede redução de bytes da saída convertidos em tokens estimados; esse número não deve ser somado aos tokens do modelo como se fosse economia líquida.

A cobertura do gateway também precisa ser medida. Formas reconhecidas de busca e leitura têm tratamento nativo; isso não demonstra interceptação universal de qualquer script ou processo que leia arquivos. Bloquear comandos não compreendidos para aumentar uma contagem de adoção criaria atrito sem demonstrar ganho. Leituras exatas devem preservar permissões e rastreamento do host para edição, e o cruzamento deve aproveitar a mesma chamada.

Os exemplos antigos de Florestal e Sialia continuam úteis como regressões, mas poucas funções conhecidas com 100% de localização não provam uma busca geral excelente. Acrescentar grafo ou vetores sem aumentar acerto, reduzir leituras posteriores ou reduzir retrabalho não demonstra ganho. Esta revisão não atribui melhora de recuperação às correções do painel ou da preparação.

## Verificação e medição desta revisão

O ensaio reproduzível `scripts/bench-panel-layout.mjs` compara a mesma árvore sintética de Consumo, com os mesmos dados e largura. Os blocos `Text` passaram de 1.704 para 885 caracteres (**48,06% menos**) e de 42 para 27 blocos (**35,71% menos**). No terminal, caixas com borda passaram de 27 para zero. Labels/opções dos controles não entram nessa contagem. O ensaio não mede pintura no host, altura realmente ocupada, tokens do LLM ou economia da assinatura.

Resultado com hashes das fontes: [medição da árvore do painel](evidence/2026-10-13-panel-layout-review.json).

Validação local: 4.147 testes Rust distintos aprovados, dois testes antigos ignorados; 62 testes do plugin aprovados em cada host Claude Code 2.1.295 e 2.1.296; validação do plugin e Clippy sem avisos. A suíte completa e as reexecuções pertinentes estão discriminadas no [registro de validação](evidence/2026-10-13-revisao-agnostica-validation.json). O checksum esperado das traduções foi atualizado para o texto novo do horizonte e o núcleo foi reexecutado integralmente.

A aprovação local não substitui a execução autenticada de uma spec equivalente, a descoberta de agentes no host ou o teste visual Desktop/SSH. Não foi aberta uma PR durante a revisão.
