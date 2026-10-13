# Diagnóstico, economia e diretório do orquestrador

Auditoria em 13/10/2026. Base: main `57a38a61` (0.2.19). Alterações no checkout isolado `codex/fix-plugin-bootstrap`. O backend em execução não foi atualizado, reiniciado nem teve código/spec alterados por esta auditoria.

## Evidências do backend

Consulta nativa somente de leitura: `mustard-rt run panel --root <backend> --spec pi-leitura-leve-do-input --section usage`, observada às 13:13 BRT. Números podem mudar com a execução.

| Medida | Registro | Interpretação permitida |
| --- | --- | --- |
| RTK no diretório do projeto | 9.016 comandos; 7.585.459 → 4.656.563 tokens estimados; 2.929.659 evitados; 38,622% | Compressão estimada de saída entre 31/08 e 13/10. Não é economia líquida da spec nem da assinatura. O RTK calcula estimativas por comando; a soma pode diferir da subtração dos totais arredondados. |
| Jev no projeto | 17 solicitações; 33.997 tokens de entrada; US$ 0,001428 estimados | 11 solicitações de planejamento de ondas, 6 de contexto. Não houve chamada Jev de busca nesta amostra. Acionamento não prova benefício. |
| Busca na spec | 1 consulta; 1 cruzada com índice; 1 inferência local; 0 remotas | Recursos acionados, amostra insuficiente para concluir acerto ou economia. Não demonstra que todas as leituras foram substituídas. |
| Operações da spec | 159 chamadas; 18 recusas (11,32%) | Inclui validações/proteções; não representa 18 defeitos. Entrada/saída do transporte são estimativas separadas do consumo do modelo. |
| Consumo das ondas | subtotal 3.162.278 tokens; 2 ondas sem medição | Incompleto. Inclui 3.010.374 tokens de cache relido e 6.648 de saída. Não é o contexto ocupado. |
| Orquestrador | 23.304.315 tokens acumulados observados | Sessão cumulativa, podendo incluir trabalho anterior; não atribuir integralmente à spec nem somar aos registros de solicitações outra vez. |

Os oito registros de sessões mostrados no diagnóstico totalizavam 30 ocorrências de ferramentas. O contador `tool_failures` era acumulado por sessão, enquanto a coluna adjacente mostrava `last_event`. Portanto, `PreToolUse · 6 falhas` **não** significava seis falhas do hook PreToolUse. Os registros antigos não guardam causa, comando ou resultado; não é possível reconstruir cada motivo a partir deles. O ledger separado do painel também tinha dois registros `usage-record-failed`, sem causa específica. Os contadores e o ledger têm coberturas diferentes; não devem ser somados como defeitos únicos.

O log da sessão principal registrava cwd terminando em `pi-leitura-leve-do-input/a`. O statusline usava o basename do cwd e procurava configuração ali: por isso mostrava apenas `a`, perdendo projeto, spec e acesso ao painel. O checkout principal continuava na branch `fix/pi-leitura-leve-do-input`; o commit `1d9f14c5` tinha referência nessa branch, sete arquivos e recibo nativo de build aprovado (13,8 s). HEAD destacado nas cópias não implica commit órfão no checkout principal.

## Mudanças

- Diagnóstico explica contadores históricos sem inventar causas. Novos eventos guardam ferramenta, horário, categoria e código de saída quando fornecido; duplicação do mesmo identificador é ignorada. Há limites de armazenamento, sem comandos, prompts, respostas ou credenciais.
- Bloqueios explicitamente reconhecidos, interrupções e comandos com saída de erro são diferenciados. Falhas cuja causa não é identificável continuam desconhecidas. Eventos de Mods também podem observar bloqueios que não disparam PostToolUseFailure.
- O gravador de métricas preserva os motivos seguros devolvidos pelo runtime, em vez de substituí-los sempre por `usage-record-failed`.
- Consumo abre com redução RTK no período do projeto, custo adicional Jev, subtotal/cobertura das ondas e uso do scan/recusas. Há atalhos para investigar cada medida. Nenhuma nota de eficiência ou percentual líquido é inventado.
- Consumo por onda mostra arquivos da entrega, resumo, tokens conhecidos e recibos de provas de critérios. Ausência de recibos não comprova que nenhum teste manual foi executado.
- Novas buscas por `--shell-output`/saída nativa registram bytes originais e entregues da mesma solicitação, sem repetir busca nem chamar modelo para medir. O painel mostra cobertura e respostas menores/maiores; histórico anterior não ganha baseline fictícia. É medição de transporte stdout, incluindo enquadramento de recibos quando presente, não economia líquida nem avaliação de acerto.
- Statusline resolve o projeto proprietário e identifica `cópia a` explicitamente, mantendo spec/modelo/contexto/acesso ao painel.
- Bash do orquestrador isola entradas em cópias Mustard em subshell. Se já estiver numa cópia, restaura primeiro o checkout principal e executa o comando no diretório original dentro de subshell, preservando saída/status. Agentes identificados pelo host mantêm seu cwd; outros worktrees não são afetados. Proteções de comandos continuam antes da reescrita. O gancho não executa checkout, reset ou alteração de branch.
- Etapa sem provas executadas passa a ser `skipped`, não `pass`.
- Novos commits aprendem escopo do histórico dos arquivos afetados, ordenam resumos e levam identificadores das ondas no corpo. Recuperação continua aceitando o formato antigo. Nenhum commit existente foi reescrito.
- Rodada omite somente o conteúdo repetitivo da preparação opcional sem revisão pendente; mantém despachos, instruções novas, alertas, revisão necessária e contadores.
- Sessões de trabalho omitem o aviso geral de branches mescladas. A manutenção continua disponível nas bases e pelo diagnóstico.
- Bootstrap manual/instalador tem 90 s para download; SessionStart mantém 10 s; demais hooks não baixam. Installer reconhece plugin já habilitado pela lista pública. Testes executam os scripts reais com ferramentas isoladas, incluindo download de 11 s, sem rede ou conta real.

## Limites da avaliação

As alegações retiradas pelo usuário — código 144, erro “Is a directory”, 95 mil tokens, teste vermelho 6/15, despacho na raiz — permanecem não verificadas. Não foram usadas para atribuir causa nem modificar preparação de dependências ou regras do código do backend. O relato de testes frágeis da onda 5 requer revisão do código dessa onda; um comentário não é recibo de execução.

A utilidade líquida do scan ainda precisa de avaliação de tarefas equivalentes: fontes corretas encontradas, leituras complementares, latência, volume retornado e consumo total até o resultado verificado. Índice populado, chamada de modelo ou arquivo alterado não substituem essa prova. A visão de consumo torna essa lacuna visível; não cria uma conclusão sem medição.

O isolamento é verificável em shell local. A integração com a sessão Desktop/SSH autenticada deve ser conferida após instalar a release, conforme decisão do usuário. A auditoria não alterou a sessão ativa.

## Relato adicional das ondas 5 a 8

Os recibos nativos confirmam builds aprovados antes dos commits das ondas 5 e 6 (16.039 e 13.736 ms, saída 0). Nessas rodadas não havia recibos individuais `criterion-proof`. A etapa agregada `criterion-proofs` sozinha não comprova execução de testes. Durações e contagens de tokens citadas pelo agente continuam distintas das medidas cumulativas do painel e não foram usadas como benchmark comparativo.

| Pedido | Ajuste e alcance |
| --- | --- |
| Consulta sem despacho | CLI `run round` sem `--report` e sem `--dispatch` consulta o estado sem gravar a spec, preparar cópias ou chamar modelo. Retomadas e sugestões de ação usam `--dispatch`. API interna da rodada mantém semântica executora. |
| Processos encerrados por cwd | A implementação realmente enviava SIGTERM por heurística de diretório. Agora somente detecta/lista candidatos (`stuck-found`). Não mata shell, editor ou agente. O código 144 e a ordem de saída do host não foram reproduzidos; não atribuímos a eles uma causa comprovada. |
| Cópia errada herdada | Vínculo agente/despacho é obtido antes da primeira escrita pelo transcript local versionado. Write/Edit são conferidos contra a cópia registrada; Bash corrige cwd herdado e recusa entrar na cópia de outra onda. Formatos desconhecidos não ganham vínculo inventado. Não equivale a sandbox de sistema operacional para scripts arbitrários. |
| Dependências das cópias | Binário executa `prepareCommand` antes do despacho e da revisão final; reutiliza recibo no diretório administrativo do Git enquanto manifests/lockfiles/esquemas reconhecidos não mudam. A ausência de node_modules local invalida o recibo. Um atalho é substituído somente na cópia, preservando o alvo; dependências ficam fora da lista de alterações/entregas. Preparo que falha não libera a onda. Sem comando configurado, não se inventa instalador. |
| Comando Jest | Execução nativa e recibo usam `--roots=<pasta>` quando há um Jest reconhecido, uma raiz e arquivos de teste explícitos. O mesmo ajuste vale no Bash antes da execução, preservando os demais argumentos e proteções. Múltiplas raízes, scripts opacos e outras ferramentas permanecem intactos. Textos antigos de tarefas do backend não foram regravados. |
| Resultado numérico versus texto | Painel distingue arquivos entregues, combinados declarados atendidos/pendentes e recibos nativos. Commit intermediário não equivale a aceitação de toda a spec. Itens globais que dependem de ondas futuras não são artificialmente declarados concluídos. |
| Provas novas sem recibo | Foram encontrados comandos novos executados após o commit como aviso. Passam a ser verificados antes do commit, inclusive quando o critério ainda tem trabalho em outras ondas. Falha, teste ausente e execução que informa zero testes recusam a rodada. Comando idêntico é reutilizado apenas dentro da mesma verificação, com recibo explicitamente marcado; não há cache de resultados entre códigos alterados. |
| Resultado das provas | Resposta da rodada mostra resultado, saída do processo, tempo e reutilização; até 16 recibos e indicação explícita dos demais. O registro completo fica na spec/painel. Ausência de execução é `skipped`. |
| Aprovação REVIEWED | O fingerprint comprova correspondência de conteúdo, não que o revisor compreendeu o diff. Uma nota ou recibo anexado pelo próprio modelo não resolve essa limitação. Reforçamos a verificação nativa antes do commit e tornamos os resultados visíveis; a avaliação técnica continua responsabilidade do revisor e da revisão final. Não afirmamos revisão semântica automática. |
| Consumo ausente | Novos vínculos começam na primeira ferramenta e usam a data do despacho original, preservando associação em atualizações do mesmo envio. Reenvios reais são tentativas diferentes. Histórico sem arquivo ou com tentativa sem associação continua incompleto; não é preenchido com zero nem com contagem declarada pelo modelo. |

Os testes usam repositórios, cópias e processos próprios em diretórios temporários. A auditoria não rodou a rodada real do backend, não instalou dependências nele e não alterou suas tarefas.

Referência de contrato: [Hooks oficiais do Claude Code](https://code.claude.com/docs/en/hooks#posttoolusefailure-input), incluindo `error`, `is_interrupt`, `agent_id`, `updatedInput` e a distinção entre falha, validação e permissão.

## Envios 9 e 10 e novos relatos

Leitura posterior confirmou duas recusas reais: envio 466 da onda 9 usava PID 1981126; o hook 468 recusou e o reenvio 471 passou a usar PID 1809027. Na onda 10, envio 515 usava PID 2024306 (já encerrado na conferência), hook 518 recusou e reenvio 520 passou ao PID 1809027, ainda vivo. A decisão de andamento dependia desse vínculo. Isso confirma a inconsistência entre envio e hook; não prova qual processo recebeu um sinal nem a causa do código 144.

- Hooks passam a propagar a sessão exata aos comandos nativos. O despacho consulta o recibo do processo dessa sessão, validando PID e hora de início, sem escolher outra sessão por data. Isso cobre comandos em background que perderam a ancestralidade original.
- A ordem de encerramento continua impedindo trabalho novo. Permite encadear operações admitidas de leitura/gravação da spec e verificações declaradas; comandos alheios, substituições e redirecionamentos para arquivos continuam recusados. A remoção de um link legado `node_modules` é permitida somente em worktree, sem apagar o diretório nem o alvo.
- Critérios distintos com a mesma prova geram uma ocorrência no texto do novo lote. A execução mantém um recibo por critério, reutilizando comandos idênticos na mesma verificação. Três critérios com o mesmo comando exigem uma execução, em vez de três; esse ganho controlado de 66,7% nas execuções não representa redução de 66,7% no consumo total.
- Limpeza pronta, pequena, relacionada por arquivos/uso comprovado e dentro do orçamento pode entrar no último lote funcional, depois da tarefa principal. Limpeza bloqueada, sem relação, concorrente ou acima do orçamento espera; não se descarta obrigação por ser pequena.
- Direções fracas de importação continuam no mapa, mas deixam de interromper a rodada com avisos estatísticos. Regras fortes e ciclos novos continuam conferidos. Não há regra específica para nomes do backend.
- A resposta compacta mostra tamanho como informação em `changes`, preservando alertas de falhas e bloqueios. O corpo do commit mantém a medida.
- A abertura do Mods informa registro do painel, interfaces conectadas e renderização observada desde o pedido atual. Registro no host não é prova de visibilidade no Desktop; a sessão autenticada ainda precisa do teste combinado após a release.

A instalação ativa do backend não foi trocada durante a auditoria. Na versão 0.2.19, executar a rodada em primeiro plano é um contorno do vínculo observado; executar `round` nessa versão ainda avança a execução. O comportamento de leitura pura pertence à versão em preparação.

## Fechamento, lint e ambiente do leitor

A leitura do `package.json` confirmou `lint: eslint . --fix --quiet --cache`; o Dockerfile usa Node 24, enquanto o Node local consultado era 26.7.0. Não executamos o fechamento, o lint, a suíte nem o leitor no backend. Os sete erros de `scripts/ms-iam/`, a restauração do `jest.config.ts` e a perda de abas em 142/240 leituras são relatos da sessão: não foram reproduzidos nesta auditoria.

O fechamento do Mustard agora acrescenta `--no-fix` somente a invocações simples reconhecidas de ESLint ou scripts de pacote cujo conteúdo reconhecido invoca ESLint com `--fix`. Preserva filtros e erros, registra o comando efetivo e não muda a configuração do projeto. Scripts compostos/opacos e efeitos de outros programas não ganham promessa de imutabilidade. Não foi incluída exclusão de `scripts/ms-iam/`: uma falha real continua impedindo o fechamento.

Além do teste do comando e do caminho de fechamento, uma execução do ESLint instalado contra um arquivo sintético em pasta temporária confirmou saída 1, erro corrigível reportado e fonte inalterado com `--fix --no-fix`. O projeto real não foi lintado. Os fingerprints de preparação também abrangem arquivos `.prisma` separados em subpastas; mudança de schema invalida a preparação da cópia.

Os eventos nativos passaram a preservar nanossegundos. Testes reais de despacho/rejeição expuseram que truncar envio e entrega ao mesmo segundo impedia distinguir o agente antigo do reenvio; os 39 testes desse contrato passaram após a correção.

O workaround de preenchimento no ZIP exige evidência independente com o arquivo real, Node 24 e leitura integral das abas esperadas. Commit, árvore limpa, testes sintéticos e relato do agente não comprovam esse comportamento. Esta auditoria do Mustard não certifica a correção do leitor de Excel nem reabre/modifica a spec ativa.

## Verificação local desta leva

- `cargo test --workspace --no-fail-fast -- --test-threads=8`: 4.139 aprovados, zero falhas, dois testes antigos ignorados; inclui 1.929 testes do runtime, 299 integrações e as suítes do scan.
- `cargo clippy --workspace --all-targets -- -D warnings`: aprovado sem avisos.
- `claude plugin validate ./plugin` e `claude plugin test ./plugin`: contrato válido e 58 testes aprovados.
- `python3 packaging/test-plugin-bootstrap.py`: oito testes comportamentais aprovados, incluindo download simulado de 11 segundos, distinção entre plugin já habilitado e falha real de habilitação e aviso de falha real no download.
- ESLint real contra fonte temporário: erro corrigível mantido, saída 1 e bytes do fonte preservados.

São verificações locais em Linux. A validação de CI nos três sistemas e a publicação da nova versão têm resultado próprio; os testes autenticados Desktop/SSH permanecem para depois da release, por decisão do usuário. Nenhum destes números comprova economia líquida de tokens de uma spec real.

A primeira CI expôs aspas incorretas no chamador de teste do `cmd`, diagnóstico com bytes fora de UTF-8 no macOS e um aviso falso do instalador Windows após download bem-sucedido. O teste passa a preservar a linha nativa do `cmd` e representar bytes inválidos sem perder as verificações de saída/status. O instalador usa `Process.Start` com prazo e código de saída, evitando o `ExitCode` nulo observado com `Start-Process -NoNewWindow`. O novo teste mantém a cobrança de aviso quando o download realmente falha. Referência: [problema registrado no PowerShell](https://github.com/PowerShell/PowerShell/issues/20400).
