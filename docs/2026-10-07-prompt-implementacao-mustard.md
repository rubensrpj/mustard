# Prompt de implementação do Mustard — validação leve, busca, scan e Jev

Preparado em 07/10/2026. Checkout analisado: branch `feature/validacao-leve-por-trecho`, commit `18ef6eed4d2e1d367a1f0ce111178bf8d51d7cc8` (`18ef6eed`). Claude Code implementa e testa; Codex fará a validação independente dos commits entregues.

Atualização de execução em 08/10: o usuário autorizou o Codex a implementar o plano completo. O checkout de implementação é `codex/mustard-plano-completo`, isolado da branch original. Conferência pelo próprio executor não constitui revisão independente. O registro vigente de implementação, provas e pendências está em `2026-10-08-implementacao-plano-completo.md`.

Extensão autorizada em 08/10: concluir as verificações locais sem depender de uma sessão real do Claude e aplicar a pesquisa de melhoria ao scan. O contrato complementar está em [Scan como oráculo](2026-10-08-scan-oraculo.md): evidência por conteúdo, interpretações com várias fontes, invalidação em worktrees, recuperação e exportação Markdown nativas. Esse complemento registra também o acerto insuficiente do piloto e as extensões ainda não implementadas; não amplia o suporte operacional ao Codex nem autoriza chamadas pagas ou publicação automática.

Continuação de 09/10: catálogo indexado e hidratação seletiva, busca completa antes do corte, frequências globais, evidências de arquivos sem declarações, referências explícitas documentação↔código, grupos estruturais e auditoria nativa foram implementados na branch isolada. O complemento do scan registra a comparação real com ckg e os limites. Não interpretar essas entregas como integração de todos os motores pesquisados, importação SCIP, compreensão integral de negócio ou economia faturada comprovada. A política continua sem IA auxiliar/Jev por padrão.

Etapa seguinte de 09/10: identificadores do corpo e textos completos passam a participar da recuperação, com pequenos trechos correspondentes e seleção ampla combinada. Pacotes versão 3 exigem novo scan. Florestal: segunda lista passou de 13/16 para 15/16 arquivos e de 7/13 para 10/13 símbolos, com respostas maiores. Avaliação externa RepoQA adaptada: arquivos permaneceram em 31/60; símbolos passaram de 7/60 para 11/60, com regressões individuais e lacunas de suporte. Suíte com 3.854 testes aprovados. Não há aceite de oráculo geral ou economia total comprovada. Os registros atuais estão nos documentos de implementação e do scan.

Continuação autorizada de 09/10: executar as cinco frentes de seleção arquivo→responsabilidade, contexto compacto, cobertura, relatórios/reuso e medição. O piloto Jev pago foi autorizado; manter inferência opcional, finalidade explícita, cache e contagem física. Formatos de `--coverage`, `--topics` e `--evaluate`, resultados e pendências atuais estão na seção inicial do complemento do scan. Esse comando de avaliação não executa nem aceita uma spec real. A seleção nova não foi promovida a padrão: uma regressão na amostra externa a mantém atrás de `--responsibility`; consultas comuns permanecem nativas mesmo com Jev configurado.

Este é o documento de entrada para execução. Consolida o código dessa base, o plano de 05/10 e o resumo “Validação leve por trecho — o que definimos”. A aprovação do item 16 foi dada pelo usuário nesta conversa em 07/10: o revisor final pode partir dos resumos das ondas para orientar sua investigação, confirmando no código. O resumo dizia que a spec estava em levantamento, sem onda entregue; esse estado deve ser conferido no começo da execução.

O documento de 05/10 fica como memória da análise e das medições históricas. Este documento prevalece sobre suas instruções divergentes, especialmente sobre validação por rodada. Não carregar toda a memória histórica em cada onda: preparar o contexto pertinente pelo fluxo vigente do Mustard.

## 1. Instrução ao executor e escopo

Implemente as entregas deste documento na ordem de suas dependências, através das specs, tarefas e ondas do Mustard. Comece pela trilha A, de validação leve. Aproveite a spec em levantamento quando for a mesma obra, sem duplicá-la. Registre critérios observáveis, contratos afetados e provas antes de editar. Decisões de produto e aprovação de plano seguem as portas vigentes; detalhes rotineiros de implementação podem ser resolvidos e registrados pelo executor.

**Responsabilidade do binário, conforme decisão de 08/10:** operações com regra objetiva pertencem ao Mustard: buscas, extração, referências, contexto, despacho, estado, cálculos, validação e geração de arquivos/páginas. LLM fica com implementação e decisões que exigem raciocínio. Jev fica atrás da interface de julgamento, somente quando a ambiguidade não foi resolvida localmente. Não abrir turno do modelo para formatar relatórios, combinar JSON, calcular métricas ou improvisar transporte externo. Operação mecânica ausente deve ser registrada como melhoria de comando nativo. Scripts de desenvolvimento usados para verificar o produto não são dependências do binário instalado.

A trilha A contém os 21 itens do resumo, incluindo o item 16 agora aprovado. As trilhas B–E conservam as entregas 1–12 do plano anterior, atualizadas para o fluxo novo. Elas são trabalhos próprios: não colocar painel, migração externa, política de modelos e toda a otimização do Jev dentro da spec de validação leve. A dependência técnica entre trilhas não autoriza fundir specs ou pendências. A trilha B continua P0 e pode avançar em trabalho separado assim que sua base de comparação estiver registrada; não precisa esperar todos os itens A para começar. Registrar a ordem efetiva e manter o núcleo independente do painel/publicação.

Antes de editar:

1. Registrar branch, HEAD e alterações locais. Confrontar o delta desde `18ef6eed`; preservar mudanças concorrentes e identificar requisitos já atendidos. Outra branch exige comparação, não apenas troca do nome neste documento.
2. Conferir regras da raiz e das áreas afetadas. O código prevalece sobre um README defasado. Skills locais com exemplos antigos precisam ser confrontadas com o código antes de uso.
3. Confirmar que este documento está disponível no checkout de execução. Na preparação ele ainda não está versionado; outro clone/worktree não o recebe por um checkout de commit.
4. Identificar configuração efetiva de comandos, modelo/esforço, instalação, árvore de execução e métricas disponíveis, sem imprimir credenciais.
5. Para cada bloco, registrar o que já existe, o que falta e a prova que distinguirá os dois. Fazer commits coesos e entregar os intervalos para revisão independente.

Não estão incluídos: implementação/fine-tuning de outro provedor, ChatGPT, Cloud Sessions, experimentos da entrega 13 antiga, automação de GitHub/release, publicação do pacote, republicação da página de gasto existente, decisão de apagar linhas antigas dessa página ou fusão das pendências de gasto com outras specs. Cópias de onda já nascerem com dependências compiladas continuam fora desta obra: o desenho precisa valer para qualquer linguagem. O tratamento vigente de `leftovers` permanece, conforme explicado na seção 4.

**Preparação para Codex — esclarecimento de 08/10/2026:** a integração com o Codex será feita futuramente, não nesta versão. Separar o estado e as decisões nativas da spec/ondas, a recuperação pelo scan e o julgamento por interface dos adaptadores do Claude Code. O painel consome uma projeção somente leitura, independente da apresentação por Mods; a porta de julgamento não depende do cliente que conduz a obra. Não implementar hooks, instalação, contabilização ou apresentação do Codex agora. Na etapa futura, conferir os contratos oficiais disponíveis e provar cada capacidade, preservando autorização, autoria e revisão; não assumir equivalência entre os clientes. Esta preparação arquitetural não constitui suporte operacional ao Codex.

## 2. O que a análise do código confirmou

Foi feita inspeção de código e do delta entre `05e1b1f2` e `18ef6eed`: 68 arquivos alterados. Esta preparação não executou testes do produto, não fez chamadas pagas ao Jev e não reproduziu as medições de 06/10. Testes mencionados abaixo são evidências existentes a executar/adaptar, não resultados desta auditoria.

| Ponto confirmado na base | Consequência para implementar |
| --- | --- |
| `round/report.rs::take_returns` chama `checks::ensure_checks_pass`; esta executa build, lint e suíte antes do commit. `close.rs::machine` volta a executar lint, suíte e provas | O contrato novo ainda não está implementado. Separar a política de execução por fase; reaproveitar o executor existente |
| `qa_run::ProofRun` já informa `ms`; `close` grava `criterion_run`; o laço de comandos de `checks` não persiste uma medição por etapa | Instrumentar o processo completo, inclusive falhas, sem criar outro executor de shell |
| `removed_check::dead_code` combina `lost_last_use` e `never_used`; já reconhece algumas rotas, contratos e usos textuais | Separar finalidades e fase. Não mover a detecção de último uso junto com a de código novo sem uso |
| `report::check_return` exige `open_sends`; `fixes::fix_file` depende de uma volta ainda não assumida | Conserto de falha detectada no fechamento precisa de autorização/despacho próprio. Retirar a exigência de envio aberto globalmente permitiria entregas avulsas |
| `answer::run_entered_round` monta o backlog pela interseção da leitura de entrada com a leitura sob trava | Tarefa liberada pelo commit da própria chamada espera outra rodada. Corrigir sem reintroduzir despacho duplicado nem reabsorver imediatamente tarefas cortadas |
| `queue::open_sends` e `unanswered_sends` já filtram ondas planejadas; `close` também assume voltas e recusa uma volta segurada | Há proteção parcial para ondas retiradas. Conferir a seleção de retornos, reprovações e bloqueios do fechamento, não só a fila de despacho |
| `dag::chain_dependents` já inclui dependentes quando dividem arquivo e satisfazem dependências/ocupação. `pack_by_kind` ainda separa por tipo | Evoluir agrupamento com produtor/consumidor elegível, sem recriar a inclusão que já existe |
| `round/usage.rs` já usa `transcript::wave_agent_file`, busca outras sessões do mesmo projeto e reúne pedaços após `/clear` | Ampliar/corrigir identificação e tentativas; não escrever um segundo leitor de conversas. O campo de consumo do condutor é repetido por onda e não pode ser somado como uso independente |
| `close::remove_build_output` apaga pastas declaradas e seguras sem medir seu tamanho | Implementar retenção pelo teto aprovado, mantendo verificações de caminho, Git e concorrência |
| `development_build::build_command` recompila os três programas em `--release --locked`; há seleção dos arquivos que pedem recompilação | Investigar o custo específico do Mustard, preservando atualização dos programas realmente afetados e identificação do commit compilado |
| `bash/waiting.rs` barra `while/until` com `ps/pgrep/pidof` e reescreve comandos Cargo | O detector é sintático e parte da reescrita é específica de ferramenta. Testar espera real e laços legítimos; a política genérica usa comandos declarados |
| `pr_azure::pat_from` já produz `azure-credential-missing`, indicando variável e cofre Git | O requisito é preservar e apresentar esse diagnóstico na porta chamada pelo usuário; não reinventar autenticação nem fazer teste remoto |
| O pedido de revisão final já lista entregas/itens; `review.md` não orienta explicitamente a usar seus resumos como mapa da investigação | Implementar a orientação aprovada e verificar leitura/independência; não usar o resumo como prova do resultado |
| O pedido de onda ainda monta `project_rules: Vec::new()` e os agentes distribuídos usam `omitClaudeMd: true` | Regras obrigatórias precisam chegar ao executor, além do revisor. Esse problema permanece na preparação de contexto |
| Scan, Jev, busca e DAG em `shared` não tiveram mudança no intervalo inspecionado; templates de onda e estilos tiveram | As oportunidades de busca/Jev continuam pertinentes. Instruções devem ser corrigidas usando os templates atuais |

Localização dos contratos: caminhos desta seção são relativos ao repositório, e os símbolos identificam a implementação. Resolver novamente símbolos/linhas no checkout escolhido. Os pontos de entrada são `apps/rt/src/commands/flow/{round.rs,close.rs}`, `round/{answer,report,checks,fixes,queue,commit,removed_check,usage}.rs`, `commands/review/qa_run/{mod,runner}.rs`, `shared/{dag,development_build,pr_azure}.rs` e `packages/core/src/{domain/wave_prompt,io/wave_prompt,io/transcript}.rs`.

## 3. Decisões que prevalecem

### 3.1. Validação por trecho e por spec

“Trecho” é a entrega que a rodada integra; uma rodada pode reunir várias ondas num commit. Executar o build declarado uma vez sobre o conteúdo integrado e cada prova pertinente uma vez, mantendo sua atribuição às ondas. Não compilar o mesmo conjunto novamente por onda apenas para medir.

Por trecho: build e provas dos critérios que a entrega consegue completar; conferências de autorização, leitura, integridade dos retornos, integração/Git e importações contra regras fortes continuam no runtime. O aviso de remoção do último uso continua por trecho. Lint, suíte integral e detecção de código novo sem uso ficam na validação final sobre o conjunto integrado.

Critério ainda dependente de outra tarefa não vira verde nem é cobrado como concluído cedo. Conservar a resolução vigente de provas novas, `covers`, `undone` e tarefas ainda não entregues. Se o comando que o projeto declarou como build/prova também executa lint ou suíte, executar o comando exato e apontar a composição; não editar comandos pessoais nem afrouxar um critério para aparentar ganho.

“Uma vez na entrega” significa uma validação completa por versão do código relevante, com nova execução quando um conserto altera essa versão. Chamadas repetidas de `close`, ou a chegada de um veredito sem mudança do código, não devem refazer uma validação completa já válida. A chave precisa incluir árvore/conteúdo, comandos/configuração e condições relevantes de execução; apenas HEAD é insuficiente se existem mudanças locais.

Falha final abre trabalho de conserto rastreável, preserva a spec em execução e invalida a validação afetada. A falha aponta evidências e ondas relacionadas; quando não houver atribuição confiável, dizer isso. Depois do conserto, validar a integração resultante e obter a revisão exigida. Não criar ciclo infinito de retries; preservar as portas de recusa e decisão do usuário existentes.

### 3.2. Linguagem, arquitetura, limites e disco

Os comandos vêm da configuração/detecção do projeto, nunca de uma lista de linguagens da rodada. A prova de contratos genéricos precisa exercitar também um projeto temporário não Rust. Adapters do scan podem conhecer sintaxe de linguagem; política de validação/orquestração não deve depender dela.

SOLID: separar política, execução de comandos, resultados, persistência e apresentação. Reusar peças existentes, sem reescrever a rodada inteira. Interface nova precisa de consumidores reais ou substituição útil em teste. A porta do Jev tem ambos: busca e ondas, além do provedor falso. O executor compartilhado já existe em `qa_run`; ampliar seu contrato quando necessário, em vez de duplicá-lo entre rodada e fechamento.

Preservar os máximos aprovados: build do trecho, 600 s; prova, 120 s ou 600 s quando requer compilação; lint e suíte final, 3.600 s cada. Vincular build ao papel do comando e à configuração da raiz em execução, sem reservar 600 s apenas para Cargo. Preservar overrides aplicáveis e informar a classificação efetiva. Não confundir timeout do Bash/hook com timeout do processo que o runtime executa.

O teto de retenção é 15 GB por pasta declarada em `buildOutput`, no checkout principal. Para implementação, usar e documentar 15.000.000.000 bytes: até esse valor conserva; acima apaga no fechamento, sob trava. Medir sem seguir links para fora; tamanho/erro desconhecido conserva e avisa. Não alocar 15 GB nos testes: injetar medidor com casos abaixo, igual e acima, e fazer integração com pastas pequenas reais.

Essa política não exige conservar worktrees de ondas encerradas. `remove_spec_copies` continua guardando código antes de remover cópias; a pasta principal retida pode beneficiar a próxima spec. Retenção no principal não prova que uma cópia nova evitará compilação fria. Não introduzir cópia de dependências compiladas, compartilhamento indiscriminado de `target` ou remoção de cache de processo ativo como parte implícita desse item.

### 3.3. Regras de estado e compatibilidade

Preservar escrita atômica, autoria, locks, fase, aprovação testemunhada do usuário, leitura dos itens obrigatórios, pedido canônico, `sent_tasks`, propriedade vigente, `join_unmet`, `undone`, prioridade explícita, exclusão por arquivos e conserto por versão. `answer` continua registrando resposta e fechamento do ponto de forma atômica. Texto opcional sem valor é omitido.

Manter o resumo vigente em Estado, Feito, Decidido, Fatos e Dúvidas, substituído pela retomada nova. Não reabrir decisões já registradas para a mesma situação; alterações pertinentes invalidam a evidência anterior. Contexto de 150 mil é alvo de composição/continuação, sem interromper tarefa em andamento. Conserto autorizado pode reabrir ferramentas acima do alvo; entrega nova fecha a trava, e trecho antigo não a reabre.

Não restaurar gates removidos por quantidade de linhas/testes. Não escrever `.git/config` no instalador, sobrescrever configuração pessoal, gravar sob `~/.claude`, trocar modelo explícito do usuário ou alterar instalação em uso durante testes. A prova de instalação usa o binário compilado do checkout em revisão, em pasta temporária vazia.

## 4. Trilha A — validação leve por trecho

Os números A01–A21 correspondem aos itens do resumo. “Parcial” significa caminho já encontrado, com comportamento restante a provar. Relatos do resumo são requisitos/casos a reproduzir; não foram tratados como bugs reproduzidos nesta análise.

| Item e prioridade | Implementação/prova esperada | Entrada principal e estado da análise |
| --- | --- | --- |
| A01 · nível 1 · build/prova por trecho | Retirar lint/suíte da rodada; mover código novo sem uso ao fechamento; manter aviso de último uso, gates estruturais e rollback. Rodada com lint/suíte configurados para falhar comita se build/prova passam; fechamento recusa corretamente | `round/checks.rs`, `report.rs`, `removed_check.rs`, `close.rs`; divergência confirmada |
| A02 · nível 1 · tempo por etapa | Registrar espera por lock, junção, formatação, build, provas, conferências, commit, scan, recompilação própria e despacho; fechamento registra lint/suíte/revisão. Cada etapa executada informa duração, resultado, versão e tentativa; falhas também persistem. Não somar duração inclusiva com suas subetapas | `ProofRun.ms`, `report.rs`, `answer.rs`, `close.rs`; medição parcial |
| A03 · nível 1 · conserto após entrega | Falha final cria/despacha onda de conserto com ligação à entrega original, arquivos, obrigações e evidência vigente. O agente grava retorno pelo envio desse conserto. Retorno sem envio/autorização continua recusado; dupla chamada não duplica conserto | `check_return`, `fix_file`, `queue`, `close`; exigência de envio aberto confirmada |
| A04 · nível 1 · onda retirada | Volta/reprovação de onda removida não bloqueia fechamento nem entra num commit alheio. Histórico, tarefas reassumidas, código pendente e processo ativo preservam tratamento próprio. Não apagar eventos como correção | `returned_waves`, `queue`, `close`; filtros parciais |
| A05 · nível 1 · despacho na mesma chamada | Após assumir/commit, reler sob lock e despachar dependente que esse commit liberou. Distinguir de tarefa recém-devolvida por corte/replanejamento. Duas rodadas concorrentes assumem e despacham uma vez | `run_entered_round`, `dispatch_backlog`, `log_on_entry`; espera adicional confirmada |
| A06 · nível 2 · código sem uso | Identificar usos externos, contratos, rotas, registros/reflexão quando demonstráveis, reexports e mapa parcial. Não desmontar arquitetura para satisfazer heurística. Bloqueio exige evidência suficiente; incerteza aparece como aviso/investigação. Código comprovadamente novo sem uso recusa no final, com origem rastreável | `removed_check`, scan/grafo; proteções existentes, falsos alarmes relatados |
| A07 · nível 2 · espera real | Recusar laço que de fato fica esperando outro processo; permitir consulta isolada, texto entre aspas, laço finito que só lista processos e tarefas legítimas. Correção não libera travas de autoria/Git ou agente antigo substituído | `hooks/bash/waiting.rs`, lexer e `flow/stuck`; detector sintático confirmado |
| A08 · nível 2 · autoria de tarefas | Agente de onda não grava/cria/reformula tarefa ou pedido de escopo diretamente na spec sem a porta autorizada. Testar pelo contexto real de onda, não pela palavra `task` no JSON. Condutor conserva o fluxo aprovado; programa conserva gravação de `leftovers`, `join_unmet` e retorno ao backlog | `spec_events/write.rs`, contexto/autoria/approval; regra de onda binária já existe, não equivale a proibir tarefa solta |
| A09 · nível 3 · produtor e consumidor | Colocar tarefa produtora e uso elegível na mesma onda, em ordem de dependência, mesmo quando o tipo de trabalho difere. Recalcular custo da onda completa e justificar divisão quando houver impedimento. Não agrupar toda relação do grafo automaticamente | `dag::chain_dependents`, `pack_by_kind`, backlog; inclusão parcial já existe |
| A10 · nível 3 · recompilação do Mustard | Medir e reduzir o build próprio pós-commit: pacotes realmente afetados, perfil incremental e caminho estável compatível com execução. Só trocar perfil após provar que todos os programas escolhidos executam e identificam a base nova. Não afetar o build dos projetos atendidos | `development_build`, `build_development_version`, lançamento de binários; `--release` confirmado |
| A11 · nível 3 · retenção de build | Aplicar a regra de 15 GB da seção 3.2; proteger raiz, paths externos, symlinks, arquivos versionados e builds ativos. Informar tamanho, unidade, decisão e erro. Não remover cache abaixo do teto | `close::remove_build_output`; remoção sem tamanho confirmada |
| A12 · nível 4 · resposta verdadeira | Rodada/retomada distinguem retorno recebido, integrado, commit, validação leve, validação final, revisão, bloqueio e próximo comando. Falha abre a resposta pelo que falhou. Repetir close não diz que executou o que só reutilizou | `round/answer.rs`, `resume.rs`, `close.rs`, i18n; evolução recente a preservar |
| A13 · nível 4 · título do commit | Título representa o conjunto efetivamente comitado; corpo conserva resumo/ondas. Evitar usar apenas a primeira onda para sugerir escopo estreito. Cumprir teto e modelo sem chamada Jev para escrever mensagem | `commit_message`, `shorten_to`; primeira onda atualmente dá a frase |
| A14 · nível 4 · escrita do domínio | Aceitar notação legítima, incluindo mês/ano como `10/2026`, sem confundir com caminho ou sigla. Testar medidor e portas que o consomem; preservar idioma e regras obrigatórias. Não criar exceção textual só para esta spec | `domain/clarity`, `clarity_check`, escrita de itens; datas completas já têm casos existentes |
| A15 · nível 4 · Azure sem credencial | Propagar `azure-credential-missing` até resposta de estado/ação, explicando fontes suportadas e próximo passo. Não reduzir a “PR ilegível”; não imprimir PAT, cofre ou header. Provar com transporte/cofre falso | `pr_azure::pat_from`, `pr_provider`, `branch_state` e portas de PR; erro do adaptador já existe |
| A16 · nível 4 · resumos na revisão · aprovado em 07/10 | Revisor lê resumos vigentes, liga ondas a arquivos/critério/commit e usa isso para escolher por onde investigar. Confere diff e código, critérios e integrações; pode expandir. Resumo ausente/errado não exclui área da revisão. Conserto revisa delta e impacto correspondente | `review.md` pt-BR/en-US, pedido final em `wave_prompt`; dados já disponíveis, orientação pendente |
| A17 · nível 5 · consumo de agentes | Identificar todos os agentes/tentativas da onda, sessões e pedaços; deduplicar mensagens e requests. Somar consumo próprio de tentativas distintas e contabilizar condutor uma vez. Conversa não encontrada aparece como desconhecida com diagnóstico; não como zero | `round/usage.rs`, `io/transcript.rs`; busca entre sessões/dedup parcial existente |
| A18 · nível 5 · teste sem critério | Avisar no trecho sobre teste novo sem vínculo demonstrável com critério/comportamento, com arquivo/linha e motivo. Teste de infraestrutura/regressão legítimo não vira erro só por faltar ID. Aviso sem gate de quantidade e sem Jev rotineiro | Diff, vínculos do scan e retorno da rodada; relato exige fixture/prova |
| A19 · nível 5 · arquivo fora da tarefa | Comparar diff real com escopo/padrões declarados e avisar caminho, onda e motivo. Distinguir arquivo desconhecido, ampliação necessária e outra área; não converter aviso em proibição total de mudança pertinente | `copy_check`, `unknown_file`, `task_files`, relatório; parte dos gates já existe |
| A20 · nível 5 · gasto de testes | Fixtures de uso/custo escrevem em diretório próprio/temporário, sem contaminar logs reais ou páginas. Verificar isolamento por processo, concorrência e cleanup. Caminho de teste não aparece no produto instalado | Helpers de teste de runtime/core e registro de chamadas; diagnóstico exato a reproduzir |
| A21 · nível 5 · timeout Windows | Corrigir regressão de deadline de 1 s com sincronização/processo controlado; provar timeout/cancelamento sem depender da máquina responder num instante. Preservar a semântica e não ocultar flake desativando o caso | `qa_run/runner.rs` e execução de processos; caso existente localizado, flake não reproduzido |

Consertos A03 e autoria A08 são complementares: criar uma onda de conserto autorizada pelo runtime não permite ao agente editar o plano por conta própria. A regra vigente de `leftovers` continua: o agente relata a sobra no retorno; a rodada grava o trabalho pelas portas e autorizações atuais. Não alterar esse mecanismo para cumprir A08.

### Ordem das ondas da trilha A

| Bloco | Itens | Dependência e saída |
| --- | --- | --- |
| A.I · contrato e medição | A02 + A01 | Medir o caminho anterior antes de mudá-lo; política por fase e textos correspondentes no mesmo bloco revisável |
| A.II · conserto e avanço | A03, A04, A05, A08 | Usa o contrato A.I; proteção de estado e concorrência antes de mudar despacho |
| A.III · confiabilidade | A06, A07, A09 | Reusa A.I/A.II; falso positivo, laço legítimo e consumidor sem bloqueio artificial |
| A.IV · tempo/disco | A10, A11 | Usa métricas A02; intervenção A10 só no próprio Mustard |
| A.V · instrução e diagnóstico | A12–A16 | Textos do contrato A01 não esperam este bloco; aqui completar diagnósticos, commit e revisão guiada |
| A.VI · qualidade da medição | A17–A21 | Parte do isolamento A20 deve estar pronta antes dos testes que gravam custo; apresentar dados finais sem contaminação |

Não esperar terminar A.V para retirar dos templates a afirmação de que a rodada executa lint/suíte: essa paridade acompanha A.I. Respeitar a urgência dos níveis do resumo; agrupar apenas tarefas que compartilham mudança/prova sem tornar a onda excessiva.

## 5. Trilha B — corrigir busca e uso do Jev

Prioridade P0 do plano anterior, entregas 1 e 2. Preserva a análise histórica: em 05/10 a spec `levantamento-sem-script` registrou 106.668.768 tokens de entrada nas buscas, e 99,87% do custo Jev registrado daquela spec veio dessa finalidade. Isso não é fatura completa nem prova do gasto da branch atual. O ganho principal depende da recuperação e do estado enviado; reduzir apenas a resposta ao Claude não resolve a entrada paga.

### B1. Contrato da busca e medição

O comando original é autoridade das ocorrências. Preservar flags, arquivos novos, arquivos ignorados conforme a ferramenta, regex, stdout/stderr, status, interrupção, pipes, redirecionamentos e efeitos. Sugestões do mapa têm identidade própria; regex sem ocorrência não ganha hit fictício. Mostrar janelas centradas nas ocorrências e intervalos realmente entregues, corrigindo a régua de cobertura.

Transformar resultado apenas para consultas reconhecidas, preferencialmente depois da execução. JSON, contagem, listagem, formato desconhecido, saída truncada sem identificação confiável e comandos compostos passam sem compactação no primeiro contrato. Na versão efetiva do Claude, comprovar que `PostToolUse.updatedToolOutput` foi aceito, com formato compatível; `additionalContext` apenas acrescenta contexto e não substitui a saída. Fonte: [Hooks reference](https://code.claude.com/docs/en/hooks).

Entradas: `hooks/bash/reading.rs`, `hooks/write/write_gate.rs`, `shared/word_search.rs`, `word_search/ruler.rs`, `hook_output.rs`, contrato de hooks no core. Provas: pipe/redirecionamento executam; flags desconhecidas preservam saída; hit no fim da função longa é entregue; ausência literal fica ausente; falha da transformação/Jev não impede a ferramenta original.

### B2. Serviço de julgamento por interface

Partir de `MapFilter`, já usado por interface na busca; desacoplar também `JevFilter`, quadros de tarefas e seleção de itens. Separar recuperação, política por finalidade, cache/coordenação/medição e transporte. A API final deve atender consumidores reais, sem inventar framework de plugins de provedor.

Desenho indicativo, adaptar às camadas reais:

```rust
trait JudgementProvider: Send + Sync {
    fn capabilities(&self) -> ProviderCapabilities;
    fn evaluate(&self, request: &JudgementRequest)
        -> Result<JudgementResponse, ProviderError>;
}

struct JudgementService {
    provider: std::sync::Arc<dyn JudgementProvider>,
}
```

O pedido identifica finalidade, evidências/versionamento e perguntas. A resposta conserva tipos, distribuição/confiança disponível e uso/tentativas; capacidade ausente ou resposta inválida tem fallback explícito. Jev é o único adaptador real inicial. Provedor falso verifica o contrato sem rede. Outro provedor futuro entra pelo adaptador e calibração, sem tipos Jev nos consumidores; não implementar Laya nesta execução.

Choice escolhe categoria, Score avalia rubrica ordinal e Noul avalia proposição binária. Perguntas precisam ser atômicas; o runtime combina as respostas. Perguntas independentes sobre o mesmo estado vão juntas; se a segunda depende de buscar evidência/alternativas com a primeira resposta, o segundo pedido é legítimo. Não mandar uma chamada por pergunta nem ampliar o estado com todo o repositório apenas para agrupar. Fontes: [Introdução](https://docs.typesafe.ai/introduction), [Primitivas](https://docs.typesafe.ai/primitives).

Choice/Score têm distribuição e confiança; Noul não tem campo separado de confiança. Valores de Choices com universos distintos não formam ranking global automaticamente. Não normalizar pertinências independentes para fazê-las parecer a distribuição do `MapFilter::Scored`. Calibrar limiares por decisão/corpus; confiança ausente permanece desconhecida. Fonte: [Confidence](https://docs.typesafe.ai/confidence).

### B3. Recuperar, reutilizar e chamar com critério

1. Fazer recuperação local por `rg`, nomes, paths, relações e escopo. Unir candidatos textuais e estruturais com origem. Ler corpos/documentação/histórico pertinentes da worktree correta; a quantidade vem da pergunta/cobertura, sem top-K arbitrário como solução.
2. Chamar Jev quando houver ambiguidade semântica e resposta com efeito possível na próxima ação. Resultado literal exato, dependência explícita ou item obrigatório não exigem julgamento. Evidência insuficiente aciona expansão/fallback, não conclusão negativa.
3. Preparar estado suficiente, estruturado e pertinente. Não carregar toda a conversa, grafo e histórico. Condições necessárias à decisão ficam explícitas. Fonte: [State](https://docs.typesafe.ai/concepts/state).
4. Reusar julgamento válido entre agentes/processos. Chave inclui finalidade, questão/rubrica, escopo/flags, árvore e conteúdo relevante, versão da tarefa/decisão, provedor/modelo e conjunto de opções quando aplicável. Mudança de agente sozinha não invalida; mudança pertinente invalida.
5. Coordenar pedidos iguais em andamento entre processos; não confiar apenas em mutex da mesma execução. Falha transitória não entra no cache como “irrelevante”. Agrupar perguntas do mesmo estado e medir o efeito da divisão em lotes.
6. Registrar chamadas físicas/lotes/tentativas, entrada, saída, uso desconhecido, falhas parciais e reuso. Cache hit referencia o pedido original; não cobra de novo nem replica custo por consumidor. Falha de um lote conserva uso conhecido dos outros. Não transformar ausência de `usage` em zero.
7. Separar política de busca das demais finalidades; a configuração atual compartilha habilitação com ondas. Preservar configurações explícitas e fallback. Os controles financeiros existentes não substituem recuperação e decisão de utilidade; não criar tetos financeiros novos como solução.

Reutilização e agrupamento são decisões de arquitetura do Mustard, a provar com seu corpus. A orientação oficial favorece composição no código e perguntas paralelas sobre o mesmo estado. Fonte: [Patterns](https://docs.typesafe.ai/patterns).

## 6. Trilha C — scan, levantamento, afinidade e contexto

Corresponde às entregas 3–6 antigas; usa contratos B1/B2 e a orquestração da trilha A. O scan permanece importante desde o levantamento até revisão/retomada. Ele prepara estrutura; conteúdo atual e validação confirmam comportamento.

| Entrega | Implementação e critério |
| --- | --- |
| C1 · scan confiável | Diagnóstico de cobertura/parse parcial, origem e confiança de relações; testes associados são candidatos, não prova de cobertura. Incremental/cache equivalem ao scan completo nas mudanças pertinentes. Arquivo novo, linguagem não reconhecida ou mapa incompleto preservam busca textual. Entradas: `apps/scan/src/{extract,graph,testmap,quality,main}.rs` |
| C2 · levantamento com fatos | Ampliar evidências além da pergunta de dependentes: fluxo, contratos, consumidores e validação. Separar fato confirmado, hipótese, lacuna e decisão de produto. Reusar `grill`, fontes do `survey`, descarte de fatos inválidos e `answer`; versionar conteúdo e árvore. Jev julga pertinência só quando útil, sem responder pelo usuário nem encerrar lacuna incerta |
| C3 · afinidade de ondas | Além de A09, avaliar afinidade de fluxo, leitura compartilhada e interferência em dimensões separadas. Gerar relações candidatas localmente antes de julgamento; evitar todos os pares indiscriminadamente. Reusar tipo/tamanho intrínseco por versão de tarefa, julgar interferência só com ondas pertinentes e recalcular após incorporar dependentes. Preservar prioridade, reservas, vagas, limpeza por último e resumo de retomada |
| C4 · contexto preparado | Preparar objetivo/itens/decisões completos, regras obrigatórias, trechos atuais, consumidores/testes candidatos, exemplo pertinente e lições, com fontes e expansão. Entrar pelo pedido canônico e leitura registrada; título/comando de despacho permanecem. Jev seleciona apenas material complementar ambíguo. Conferir worktree antes do envio, invalidar apenas partes afetadas e preservar todos os registros de leitura |

O crescimento da onda completa precisa ser comparado com uso real. Tamanho ordinal de Jev não é quantidade comprovada de tokens. Compartilhar material no disco não prova cache de prompt nem economia entre agentes. Ajustar espera de lotes pequenos ou calibração só com comparação de latência, contexto, conflitos e qualidade.

### Onde usar Jev e onde executar localmente

| Finalidade | Recuperação/regra local | Julgamento possível e condição |
| --- | --- | --- |
| Busca conceitual faltante | `rg`, símbolos, escopo e relações | Pertinência de candidatos ambíguos; dispensar em localização exata |
| Fatos para pergunta de levantamento | Fontes atuais, decisões/specs anteriores | Pertinência da evidência a um ponto aberto; decisão de produto permanece com usuário |
| Perfil de tarefa | Versão completa da tarefa e métricas já conhecidas | Tipo/tamanho intrínseco se útil; reusar sem reavaliar todo backlog a cada rodada |
| Afinidade/interferência | Dependências, arquivos ocupados, produtor/consumidor | Julgar só relações plausíveis não determinadas; prioridade explícita não é pergunta |
| Itens complementares do pedido | `covers`, autoria, `every_wave`, vínculos explícitos | Item ambíguo governa algo que a tarefa muda/testa? Obrigatórios entram sem chamada |
| Exemplos/skills | Padrão, tarefa, localização e aplicabilidade declarada | Desempatar candidatos pertinentes quando isso mudar a execução; não julgar skill declarada por rotina |
| Conserto/revisão | Diff, falha vigente, critérios e resumos | Material complementar ambíguo; nunca aprovar código, teste, autoria ou “sem uso” por probabilidade |
| UI, gasto, publicação e instruções | Projeções, aritmética, eventos e paridade | Nenhuma chamada de rotina. Jev não gera prosa de relatório/prompt |

Lições automáticas e pontuação semântica de toda resposta continuam experimentos futuros. Não expandir usos antes de corrigir as buscas que explicavam o maior custo histórico.

## 7. Trilha D — estado local, painel e publicação explícita

Corresponde às entregas 7–10 antigas. Manter como trabalho próprio, sem mudar a publicação nesta spec de validação leve.

### D1. Projeção e métricas

Uma porta Rust de consulta fornece estado coerente por projeto/worktree/spec/sessão, versão e horário. Reusar os eventos, sem um segundo histórico. Separar recebido, integrado/comitado, validação leve, validação final válida, revisão, conserto, espera por processo e decisão. Os resumos A16 orientam investigação; não são um novo banco de verdades.

Integrar A02/A17 e uso Jev: tempos e tokens são medidas distintas; input/output/cache, provedor/modelo, custo medido/estimado e origem têm semântica explícita. Uso do condutor compartilhado conta uma vez. Métrica faltante é desconhecida. Checkpoint intermediário não simula `write step` de término de tarefa. Encerramento persiste pendências sem marcar conclusão falsa nem abrir nova onda.

### D2. Painel e statusline

O painel local mostra Projeto, Specs, Execução e Consumo, com atualização por eventos/consulta. Statusline é projeção compacta do mesmo estado. Renderização não roda scan/Jev nem inicia turno do Claude. Hooks mantêm fallback essencial.

Em 08/10, a documentação oficial de [Mods](https://code.claude.com/docs/en/plugins/mods) e [interface](https://code.claude.com/docs/en/plugins/mods/interface) confirma panes, comandos imediatos, eventos e componentes nativos a partir do CLI 2.1.287. O ambiente tem CLI 2.1.292. O adaptador em `plugin/hooks/register.js` foi exercitado com o kit oficial: abertura, consulta, atualização, fechamento, falha e exportação sem turno do modelo. Esse teste não substitui aceite visual em uma sessão real do usuário.

`/mustard-panel` é registrado pelo adaptador Mods; não é URL de navegador. Projeto, specs, execução e consumo aparecem no mesmo painel. Preservar a identidade mostarda/carvão e melhorar hierarquia, cartões, navegação e legibilidade. O snapshot externo reutiliza o layout compartilhado, com tema claro/escuro e adaptação ao celular. O primeiro adaptador observa/oferece comandos; migração geral da interceptação exige experimento próprio e equivalência.

Observar eventos Mods reais (`tool.call`, `turn.complete`, `session.compact`, `classic.SubagentStop`) e coalescer consultas; manter o poll como cobertura para alterações fora do host. Capturar `session.measure` diretamente. Nunca contar entrega/validação de novo só porque um evento foi observado, nem substituir os gates nativos dos hooks clássicos sem provar equivalência. Os botões chamam a mesma função local do comando: chamadas a comandos do próprio plugin podem não voltar ao handler dele.

### D3. Transporte e publicação externa

A recomendação de 08/10 é **Cloudflare Pages Direct Upload**, com HTML/JSON estáticos gerados e enviados pelo binário. Configuração tipada por projeto (`publication.provider/accountId/projectName`) e token somente no ambiente (`CLOUDFLARE_API_TOKEN`). O usuário cria/configura o projeto Direct Upload; não criar recursos remotos nem publicar durante implementação/validação sem pedido específico. Não usar ArtifactData nem modelo/Python/Wrangler como executor do envio.

Verificar o contrato nas fontes oficiais, enviar somente assets ausentes, criar deployment, persistir identificador aceito e consultar prontidão. Uma repetição do mesmo conteúdo consulta o deployment aceito; conteúdo diferente só é publicado mediante novo pedido. Não fazer retries cegos do POST de criação. Erros não confirmam URL/cópia. Testes de protocolo/local HTTP são necessários, mas não substituem o aceite autenticado na conta do usuário. Cloudflare Pages exige esse aceite real antes de afirmar publicação operacional comprovada.
O comando direto nos Mods é `/mustard-pages project`, `/mustard-pages spec [nome]` ou `/mustard-pages report arquivo.md`. Sem nome, `spec` resolve a branch atual no binário; `project` independe de spec aberta. Relatórios Markdown solicitados seguem o mesmo layout de `run page`: raciocínio/texto podem exigir o modelo, mas conversão e envio são nativos. Apenas o documento explicitamente solicitado entra na publicação. O suporte inicial não envia anexos locais implicitamente.

Depois da prova, **Publicar acompanhamento** e comando equivalente geram HTML/JSON e publicam o snapshot para a spec selecionada; **Atualizar publicação** envia outro snapshot explicitamente. Entre essas ações não há envio remoto automático. Painel local continua em tempo real. Página externa mostra versão/horário do estado; consumo só se incluído para compartilhar. Nas projeções de projeto/spec, não enviar código, conversa integral, segredo ou caminhos absolutos. Relatório solicitado pode conter análise em texto; bloquear aparência de segredo e nunca acrescentar outras fontes implicitamente.

Remover gatilhos automáticos de projeto/spec e dependência de publicação para aprovação/conclusão, mantendo histórico existente. O JSON publicado é uma projeção estática, não fonte do estado local. Falha externa não muda resultado da tarefa. `run page` hoje gera HTML local e, isoladamente, não comprova publicação nem criação de banco.

Abrir/copiar link exige sucesso e identificador persistido. Retirar acesso externo é outra operação e só entra com suporte comprovado. A decisão posterior de 08/10 vale para **todas** as páginas: início de sessão, plano, rodada e fechamento não geram publicação externa, inclusive de gasto. `run spend` mede localmente; `--publish`/`--republish` são pedidos explícitos de geração/envio. Preservar histórico existente, sem apagar dados remotos. Republicação da página de gasto já publicada e retirada de acesso continuam ações externas separadas.

## 8. Trilha E — instruções, modelos e distribuição

### E1. Markdown e contexto por finalidade

Textos do comportamento acompanham a entrega que muda o código. Auditar pt-BR/en-US em `packages/core/templates/{agents,mustard}`, comandos/estilos em `plugin`, prompts/i18n e documentação operacional. Não apenas editar o Markdown instalado da máquina do autor: a distribuição é a fonte; atualização conserva modelo/configuração pessoais.

- Em A01, trocar “lint/suíte da rodada antes do commit” por validação final; tarefas de remoção/configuração usam prova pertinente e suíte final, sem criar teste espelho. Em A16, orientar revisão pelos resumos com confirmação no código.
- Em B/C, usar evidência preparada antes de `map search`/Grep duplicados. `summary`, `users` e `tests` são consultas por necessidade; mapa sugere testes e histórico, não demonstra cobertura/intenção. Não repetir descoberta já entregue e atual.
- Entregar ao executor regras da raiz/diretórios afetados mesmo com `omitClaudeMd`. Requisitos e regras obrigatórios não passam por corte probabilístico.
- Permitir releitura dirigida quando conteúdo mudou, houve edição concorrente ou uma prova exige conferir o resultado. Resumo/referência não substitui conteúdo obrigatório nem registro de leitura.
- Compor instruções por finalidade: levantamento, onda, conserto e revisão final. Evitar fazer toda resposta de status seguir explicação longa ou gerar página; conservar linguagem/preferências aprovadas e espaço para evidência técnica quando solicitado.
- Em D, trocar publicação automática/páginas como requisito por painel local e ações externas explícitas. Atualizar testes existentes que hoje exigem publicação no início da sessão.
- Conferir skills locais `refazer-gancho`, `add-hook-rule` e `add-run-command` contra registro/enums/paths/autoria atuais; a auditoria histórica encontrou lista fechada de hooks e exemplos obsoletos. Não apagar hooks atuais para satisfazer receita antiga.

O resumo exige texto testado contra código. Provas devem comparar contrato executado/saída e instrução correspondente, com casos que falham se só um dos lados mudar. Uma lista de substrings, contagem de palavras ou teste que confirma a cópia da mesma frase não prova paridade. Reusar `plugin_prose_matches_shipped_behaviour`, `plugin_agents`, provas de pedido e i18n, acrescentando casos comportamentais pertinentes.

### E2. Modelos e calibração

Entrega 11 antiga. Registrar modelo efetivo, alias, esforço e configuração. Comparar políticas por papel/tarefa com busca e validação mantidas constantes, preservando escolhas explícitas. Versões/capacidades são as disponíveis no ambiente; não fixar implementação em nomes anunciados sem verificar. Recalibrar onda/contexto com custo, tempo, qualidade e consertos reais. Nenhuma troca automática de todos os agentes para Opus.

### E3. Distribuição e atualização

Entrega 12 antiga. Compilar CLI/runtime/scan afetados; instalar o CLI desse checkout numa pasta temporária vazia (`<checkout>/target/debug/mustard init --yes`). Provar caminho do usuário, update, configuração pessoal preservada, idiomas, comandos e fallback sem Jev/sem adaptador visual. Fonte empacotada deve corresponder ao executável instalado.

Preparar documentação/artefato de distribuição com comportamento comprovado. Publicar marketplace/pacote e migrar instalação em uso não são consequências automáticas desta entrega.

## 9. Provas de aceite e medição

### 9.1. Referência e interpretação honesta

O resumo trouxe medidas de 06/10: rodada com commit, 434–592 s; suíte, 277 s; recompilação própria, 192 s; cópia fria, cerca de 330 s; 25 recusas (14 teste, 6 build, 5 lint); 13–27 GB por cópia, cinco acima de 100 GB. Registrar como observações fornecidas pelo usuário, sem afirmar que foram reproduzidas nesta preparação. Tempos podem representar etapas sobrepostas ou execuções diferentes; não somar como uma única rodada.

Comparar mesma spec/casos, commit/configuração e máquina, distinguindo build frio e incremental. Medir também espera, retries, revisões e fechamento. Transferir uma suíte para o final reduz repetição; não elimina seu custo. Retenção do build principal e mudança do perfil do Mustard não garantem acelerar cópia fria. Não prometer percentual antes da medição.

### 9.2. Contratos da trilha A

1. Rodada: comandos declarados deixam marcas distintas; build/prova passam e lint/suíte configurados para falhar não são executados antes do commit. Build/prova que falham recusam e preservam disco/índice/estado corretamente. Executar também num fixture não Rust.
2. Fechamento: lint/suíte executam sobre integração completa; código novo comprovadamente sem uso recusa com evidências relacionadas à onda. Comandos ausentes recebem diagnóstico, sem fingir validação. Repetição sem mudança reutiliza resultado válido; conserto ou comando/árvore pertinente diferente invalida.
3. Conserto final: falha cria trabalho rastreável; retorno pelo envio autorizado é aceito; retorno avulso, trecho antigo e segundo agente indevido continuam recusados. Duas chamadas não duplicam eventos/commits/ondas.
4. Retirada/reassunção: onda removida não impede fechar; tarefa reassumida fica com a dona vigente. Código não integrado e processo em andamento não são descartados por uma limpeza de eventos.
5. Despacho: commit libera dependente na mesma chamada; corte/replanejamento não volta a executar imediatamente sem a condição acordada. Concorrência cobre leitura, junção, gravação, commit, rollback e despacho.
6. Scan: último uso removido avisa no trecho; novo código legítimo em contrato/rota/registro não obriga desmontar serviço; ausências de ligação no mapa não provam falta de uso. Conferência forte não é terceirizada ao Jev.
7. Autoria: agente de onda não grava tarefa solta por contornar campo `wave`; programa conserva os caminhos autorizados de sobras/obrigações. Testar chamada real/contexto.
8. Disco: medidor fake prova fronteira de 15 GB e erro; integração prova paths pequenos, symlinks, Git e lock. Cache abaixo do teto permanece; pasta insegura ou ativa não é apagada.
9. Revisão: resumo correto guia descoberta; resumo incompleto/errado e alteração não citada continuam descobríveis por critérios/diff. Retomada usa versão vigente e conserto confere delta/impacto. Não trocar o revisor independente pelo autor.
10. Medição/textos: tempos persistem no sucesso e falha, sem dupla soma; conversas/tentativas são correlacionadas e faltas explícitas. Texto de pt-BR/en-US corresponde ao processo exercitado. Tests de gasto não alteram logs reais; Windows prova deadline sem corrida incidental.

Os contratos genéricos da rodada/fechamento precisam de fixture não Rust. Testes dos mecanismos internos A10/A20/A21 podem permanecer no próprio Mustard, como o resumo permite; não fabricar uma segunda linguagem para testar uma otimização interna de compilação Rust. Fonte e distribuição continuam com provas reais de instalação.

### 9.3. Busca, Jev e benefício de ponta a ponta

Usar busca exata/conceitual, regex ampla, função longa, múltiplos hits, teste, arquivo novo, worktree divergente, mapa parcial e mudança posterior ao cache. Confirmar equivalência da ferramenta e cobertura da recuperação. Corpus fixo valida escolhas/probabilidades por partição; não aceitar ranking global incorreto por soma/normalização artificial.

Provedor falso prova perguntas/respostas/erros/cache/concorrência. Replay prova recuperação/invalidação. Julgamento real, em casos delimitados e com uso registrado, é necessário para qualidade semântica/custo remoto; simulação não basta. Se não puder medir, registrar lacuna e limitar a conclusão.

Comparar execução normal, scan com seleção local e scan com Jev, mantendo modelo/esforço. Depois isolar evidências no levantamento, afinidade e contexto preparado. Só então comparar modelos. Medir chamadas/lotes/tentativas, tokens Claude/JeV, cache, latência, recusas, consertos, trabalho aceito e custo total por spec; sem relaxar critérios.

Piso de aceite: contratos preservados, descoberta/qualidade sem regressão nos casos acordados e melhoria observada no custo/tempo ou justificativa explícita para rever a política. Não prometer economia por contar linhas de Markdown nem aprovar adoção pelo código apenas compilar.

## 10. Dependências e material para validação pelo Codex

| Trilha | Dependências |
| --- | --- |
| A · validação leve | Inicia pela spec em levantamento e contratos atuais; A02 registra referência e A20 isola medições |
| B · busca/serviço Jev | Caminho próprio; integrar com medição A02/A17 quando disponível, sem alterar o contrato da trilha A |
| C · evidências e contexto | B para recuperação/julgamento; C1 pode avançar isoladamente; afinidade integra A09 |
| D · projeção/painel | Estado A e medição confiável; painel depende de prova da superfície; publicação depende de transporte provado |
| E · instruções/modelos/distribuição | Paridade acompanha cada alteração; modelos dependem de métricas; pacote depende das entregas adotadas |

Cada bloco entregue informa: spec/itens atendidos, commit-base e final, configuração, alterações, testes com comandos/resultados, integração pelo caminho real, compatibilidade/recuperação, métricas com método e pendências. Separar o que a base já fazia do que o bloco mudou. Registrar dados ausentes, testes não executados e capacidades externas não demonstradas.

Codex revisará um snapshot identificado e reproduzirá verificações pertinentes. Resultado do Claude não é validação independente. Commit posterior é novo delta; não herda revisão anterior. Guardar progresso nas portas/artefatos da spec para retomada sem reprocessar a conversa inteira.

## 11. Mensagem para enviar ao Claude Code

```text
Leia docs/2026-10-07-prompt-implementacao-mustard.md e execute conforme
seus contratos, prioridades e dependências. Ele prevalece sobre instruções
divergentes do plano de 05/10.

Registre branch/commit atual e confronte o delta desde 18ef6eed.
Comece pela trilha A na spec de validação leve, aproveitando seu levantamento.
O item A16 está autorizado: resumos orientam a revisão, com confirmação no código.
Prepare tarefas/ondas e critérios pelo fluxo do Mustard e implemente os blocos.

Execute também B–E em specs próprias, respeitando as dependências, sem
fundir pendências de gasto nem publicar recursos externos automaticamente.
Busca/custo Jev na trilha B é P0; não precisa esperar todos os itens A.
Preserve aprovação/autoria, leitura obrigatória, locks e mudanças concorrentes.
Registre commits, testes, provas, métricas e pendências para validação pelo Codex.
```

Fontes históricas: `docs/2026-10-05-plano-busca-jev-claude.md` e o resumo fornecido pelo usuário. Os requisitos operacionais e medidas desse resumo foram incorporados acima para que o executor não dependa de um arquivo em `.codex/attachments` ou desta conversa.
