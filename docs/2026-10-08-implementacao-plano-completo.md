# Implementação do plano completo

Autorizada pelo usuário em 08/10/2026. Base: `18ef6eed4d2e1d367a1f0ce111178bf8d51d7cc8`, branch original `feature/validacao-leve-por-trecho`. Implementação isolada na branch `codex/mustard-plano-completo`; a instalação e a branch em uso são preservadas.

Contrato de entrada: `2026-10-07-prompt-implementacao-mustard.md`. O escopo abrange as trilhas A–E e o item A16 aprovado. Não envolve publicar recursos externos ou atualizar a instalação pessoal automaticamente.

Esclarecimento do usuário em 08/10: suporte ao Codex é futuro. A preparação atual conserva interfaces de julgamento e projeção de estado independentes dos adaptadores do Claude. Não será implementada integração Codex nesta entrega; não haverá alteração da configuração pessoal desse cliente.

## Estado da implementação em 08/10

| Trilha | Mudança entregue no checkout | Aceite ainda não comprovado |
| --- | --- | --- |
| A · validação e ondas | Build/provas na rodada; lint/suíte no fechamento; recibo por conteúdo/configuração/ambiente; conserto rastreável; métricas; autoria; retirada/reassunção; dependentes liberados; agregação de tentativas; preservação de build até 15 GB; perfil incremental nativo | Revisão independente, execução nativa no Windows e comparação de tempo/custo numa obra real |
| B · Jev | Interface de provedor, perguntas tipadas, recuperação local antes do julgamento, cache por evidência/revisão, exclusão entre processos, diário de chamadas físicas/retries, uso desconhecido explícito, perfis reutilizados e pares candidatos; Grep/rg literais sem chamada Jev | Precisão semântica e economia reais com o serviço pago; nenhum benchmark de custo foi simulado como gasto real |
| C · evidência e contexto | Cobertura/parse parcial/origem no scan; relações/testes como candidatos; regras obrigatórias; afinidade local de fluxo/leitura/interferência; trechos preparados por componente, validados contra o worktree; pacotes de conhecimento, interpretações com várias fontes, consulta e Markdown nativos | O piloto inicial encontrou 2/6 destinos; a melhoria de descoberta encontra 4/6 no mesmo conjunto e 6/6 nas perguntas adicionais locais. Não há aceite de oráculo por intenção de negócio, wiki semântica completa, resolução por compilador nem comparação independente com o Puzzle |
| D · acompanhamento | Projeção Rust, statusline, painel Mods com Projeto/Specs/Execução/Consumo, exportação explícita sanitizada/idempotente, layout mostarda/carvão com cartões e temas claro/escuro | Aceite autenticado do transporte Cloudflare na conta do usuário; aceite visual do Mods numa sessão real |
| E · instruções/distribuição | Agentes e mapa pt-BR/en-US, comandos, READMEs, referência operacional, skills de desenvolvimento e paridade atualizados; orçamento de instruções preservado; instalação real em pasta vazia | Calibração de modelos em operação e publicação/distribuição externa, que não foram executadas |

### Responsabilidade do binário

A decisão mais recente do usuário é executar nativamente toda operação mecânica possível. Estado, regras, recuperação, referências, contexto canônico, despacho, cálculos, validação e arquivos/páginas são responsabilidade do Mustard. O modelo interpreta e implementa; Jev julga somente ambiguidades pertinentes. A política foi distribuída nos mapas de início de sessão dos dois idiomas.

O painel observa eventos de ferramentas/turnos/compactação/agentes, agrupa consultas em 200 ms e mantém polling a cada 2 s para mudanças externas, sem scan, Jev ou turno do modelo. Dados de consumo vêm também de `session.measure`. Publicação explícita de projeto, spec, relatório e gasto gera HTML/JSON/manifesto nativos e usa Cloudflare Pages Direct Upload quando configurado. O transporte segue o protocolo oficial: JWT para assets, hash BLAKE3 compatível com Wrangler, verificação de assets ausentes, upload seletivo e deployment multipart. Tokens permanecem no ambiente; erros não exibem credenciais/diagnósticos remotos. Só deployment pronto com URL no domínio do projeto confirma publicação. O identificador aceito fica persistido antes da consulta; repetições consultam a mesma publicação, inclusive após erro de prontidão. Um deployment que falhou no servidor pode ser tentado novamente sob pedido explícito. Não há retry cego do POST de criação.

Sem configuração/token, os arquivos ficam locais. Nenhuma conta/projeto remoto foi criado e nenhum upload autenticado foi executado nesta implementação. A prova local do protocolo não é apresentada como aceite real do Cloudflare. Se o POST criar um deployment e a conexão cair antes da resposta/identificador, é necessária reconciliação na conta antes de repetir: não se afirma idempotência nesse caso ambíguo.

### Publicação e limpeza

Nenhuma abertura de sessão, plano, rodada, fechamento ou gravação de pedido sincroniza páginas externas. Projeto e specs ficam no painel local. O comando de gasto mede localmente por padrão; preparação externa requer --publish ou --republish. A exportação da spec requer run publish ou o comando explícito do Mods. Geração local não é publicação remota.

Foram retirados os templates automáticos de projeto/spec, suas fixtures, o template/lotes ArtifactData de gasto e o observador sem produtor de lotes antigos. O catálogo já havia removido 328 chaves; nesta continuação foram removidas mais 22 chaves sem consumidores. Permanecem leitores de recibos/URLs históricos, a publicação estática explícita de gasto e a migração de arquivos reconhecidos como pertencentes ao Mustard. Dados/arquivos pessoais e registros de uso não são apagados por idade. O levantamento de 05/10 é memória histórica, não instrução distribuída ao modelo.

O instalador também deixou de adicionar a permissão ArtifactData: o fluxo nativo atual não precisa liberar publicação para o modelo. Uma permissão externa que já estiver nas configurações continua preservada como decisão pessoal; a atualização não tenta atribuir sua autoria ao Mustard. A constante antiga e a regra de inclusão automática foram retiradas, e os testes de instalação passaram a conferir ausência dessa inclusão e preservação das decisões existentes.

### Provas realizadas

- CLI: 56 testes unitários e 16 integrações aprovados.
- Núcleo: 1.104 unitários e 38 integrações aprovados, incluindo limites de texto, contexto preparado, cache por componente, invalidação por fonte secundária, fontes divergentes, regras e protocolo ativo de gasto.
- Runtime: 1.820 unitários aprovados, 2 testes ignorados herdados; 280 integrações aprovadas, incluindo duas máquinas temporárias em paralelo, consumo isolado, custos cumulativos, identidade canônica do projeto, proteção para eventos em projetos sem Mustard, limpeza, transporte HTTP local e ausência de exportação no gasto local. Referências desatualizadas de publicação foram corrigidas; recusas e limites de instruções foram preservados.
- Scan: 96 unitários e 399 integrações aprovados, incluindo pacotes persistidos, relações conferidas nos dois sentidos, interpretação que sobrevive ao scan mas expira por conteúdo e evidência de cópia de onda divergente.
- Total da execução geral mais recente: **3.815 testes Rust distintos aprovados**, 0 falhas, além dos 2 ignorados herdados (`cargo test --workspace`). Log: `/tmp/mustard-concepts-workspace.log`. Inclui recuperação/projeções, anotações com linhas originais e reutilização incremental, navegação exata e consumidores, ciclos/homônimos/ambiguidades, cópia divergente, fila de revisão e fingerprint integral; nenhuma alteração de produção posterior.
- Lint estrito: `cargo clippy --workspace --all-targets -- -D warnings`, aprovado em `/tmp/mustard-concepts-clippy.log`. O build anterior dos três programas foi aprovado em `/tmp/mustard-knowledge-build-checked.log`; a atualização do pacote inclui nova compilação identificada pelo commit, em `/tmp/mustard-concepts-build.log`.

- Kit oficial Mods: **10 testes de interação aprovados**, 0 falhas (`claude plugin test ./plugin`), em `/tmp/mustard-oracle-mods-tests.log`. Consulta local, fechamento, erro com estado datado, caminho .exe, publicação sem turno mesmo com ferramentas externas anunciadas, resultado confirmado/pendente, botão de projeto sem spec, atualização agrupada por eventos, relatório com espaços e persistência nativa de `session.measure`/campo oficial de contexto. Manifesto/hooks aprovados no CLI 2.1.292 (`claude plugin validate ./plugin`), em `/tmp/mustard-oracle-mods-validate.log`. Documentação: [Mods](https://code.claude.com/docs/en/plugins/mods) e [interface](https://code.claude.com/docs/en/plugins/mods/interface).
- Build frio real dos três programas no perfil mustard-dev, em target isolado vazio: 2 min 43 s. Build incremental após alterações: 6,61 s; repetição sem alteração: 0,11 s. Os três executáveis foram rodados e identificaram a base g18ef6eed4d2e-dirty. Essas medidas locais não equivalem a aceleração medida de uma obra real.
- Instalação real: `target/debug/mustard init --yes` em projeto temporário vazio com scan real, seguida de atualização pt-BR. Configuração do Git e regras/modelos pessoais preservados; ausência de permissão ArtifactData automática e preservação de uma permissão pessoal existente; início de sessão sem páginas externas; gasto local sem página; painel somente leitura. Exportação explícita de projeto **sem spec**, relatório para gestores com tabela/layout comum, spec da **branch atual mesmo com variável de spec divergente** e spec com consumo conferidas. Gasto explícito prepara recursos sem ordem para o modelo ou recibo inventado. Nenhum caso sem credencial confirma publicação remota. Fixture removida ao terminar. Log: `/tmp/mustard-pages-native-acceptance.log`; registro: `/tmp/mustard-native-acceptance-result.json`.
- Pacote local de revisão: `target/review-plugin`, com CLI/runtime/scan recompilados no commit desta continuação, identificados por SHA-256 e versão no manifesto `target/review-plugin-manifest.json`. Validação e 10 testes do kit oficial conferidos nessa cópia; logs em `/tmp/mustard-oracle-package-validate.log` e `/tmp/mustard-oracle-package-tests.log`. Nenhuma cópia foi instalada na conta pessoal ou publicada.
- Página de spec gerada pelo binário inspecionada em 1440×1100 e 420×1000, temas claro/escuro; relatório para gestores inspecionado em 1440×1100 (`/tmp/mustard-report-desktop.png`). Cartões, menu, tabelas e barra de progresso mantêm a identidade existente. São dados fictícios, sem publicação ou aprovação de uma spec real.

Nenhuma chamada paga ao Jev, upload remoto, release, push ou atualização da instalação pessoal foi executada. O checkout original permaneceu com os mesmos dois documentos de entrada não versionados. O trabalho está numa branch isolada para revisão; ainda não foi integrado ao checkout em uso.

### Limites da conclusão

Testes com provedor falso comprovam contratos, cache e concorrência; não comprovam qualidade do julgamento remoto ou redução de custo total. Não há percentual de economia validado. A leitura do código e o teste de UI pelo próprio executor não substituem revisor independente. O kit que simula Windows não é execução nativa naquele sistema.

A documentação oficial do Jev foi conferida novamente: [introdução](https://docs.typesafe.ai/introduction), [estado](https://docs.typesafe.ai/concepts/state), [Noul](https://docs.typesafe.ai/primitives/noul) e [modelos](https://docs.typesafe.ai/models). O cliente mantém perguntas pequenas e tipadas, estado relevante, julgamento independente por candidato e versão de modelo explícita. Regras objetivas são combinadas no binário. Agrupamento só cabe quando as perguntas compartilham o mesmo estado; duplicações são tratadas com cache e exclusão entre processos. Esses princípios orientam a redução de chamadas; não dispensam medir a qualidade e o custo na operação real.

A implementação foi realizada diretamente no checkout isolado; não se fabricou um histórico de aprovação/spec/ondas como prova retroativa. Isso é uma divergência do método solicitado no prompt original, apesar das provas dos fluxos do produto.

## Continuação: conhecimento do scan e consumo local

O usuário definiu como condição de utilidade que o conteúdo permita criar specs e compreender o código sem repetir leituras completas do repositório. O levantamento `puzzle-fluxo-completo.md` é referência de organização e perguntas; suas interpretações estáticas não são gabarito de execução verificado por esta implementação.

O usuário autorizou aplicar a pesquisa ao scan e concluir o que fosse verificável localmente. O banco agora conserva pacotes de evidência e interpretações separadas, cada uma com todas as suas fontes. Arquivo, intervalo e hash identificam o conteúdo do parser; mudança em qualquer fonte exclui a interpretação atual, sem renovação silenciosa pelo scan. Relações antigas de chamador e alvo são excluídas; cópias de onda usam seu próprio conteúdo. A recuperação usa o índice local existente, apresenta candidatas e lacunas e projeta Markdown nativamente, sem Jev/gerador pago.

`run knowledge` é a porta de consulta, registro e exportação. O levantamento recebe evidência de descoberta sem fechar pontos automaticamente; contexto preparado recebe pacote pertinente quando couber, preservando itens e regras obrigatórios. O mapa de início de sessão menciona consulta e expansão, ficando em 2.474 bytes pt-BR e 2.513 bytes en-US, sem aumentar o limite de instruções.

O piloto inicial sobre o próprio Mustard encontrou **dois dos seis destinos específicos esperados**. A melhoria alterna responsabilidade documentada com o índice local, preserva nomes exatos, diversifica arquivos e prioriza descoberta sobre expansão de relações. No mesmo snapshot, encontra **quatro dos seis destinos**, e as seis perguntas adicionais passam de cinco para **seis acertos**, sem chamadas remotas. Essa eficácia ainda não autoriza prometer substituição da investigação por intenção de negócio nem economia. A passada anterior teve construção de 17,49 s, consultas de 2,05–2,70 s e respostas de 21.093–35.209 bytes. O comparativo desta melhoria mantém o snapshot de código e registra os resultados anteriores e atuais, por pergunta, com hashes e versões dos executáveis em `apps/scan/tests/fixtures/knowledge-comparison.json`. A leitura consulta os pacotes diretamente no SQLite, sem desserializar todas as declarações/textos/história do projeto. A projeção inicial compacta detalhes, e `--detail`/Markdown expandem a evidência armazenada dos mesmos símbolos. O contexto das ondas leva versão da evidência integral, para que uma mudança além do trecho visível também invalide o cache. Bytes não são tokens; uma medida local não compara motores nem qualidade semântica. A pesquisa, referências de código, protocolo, comandos e extensões pendentes estão em [Scan como oráculo](2026-10-08-scan-oraculo.md). Não foi implementado gerador automático de narrativas de negócio nem integração MCP para o Codex.

Consumo local também foi completado: histórico de tokens por raiz canônica do projeto, histórico da máquina separado, detalhamento de entrada/saída/cache e uso parcial; custo cumulativo recebido do host não duplica leituras, e intervalos entre branches ficam sem atribuição. Esse custo é estimativa equivalente de API, nunca cobrança da assinatura. Statusline fornece observação local reutilizada pelo painel. Consulta usual não reconta transcrições; o adaptador solicita atualização inicial e a cada 30 s. Eventos globais em projetos sem Mustard não criam pastas de uso.

Aceite dos executáveis reais em fixture temporária: instalação/scan, consulta de chamada, recibo de interpretação, exportação Markdown sem publicação, invalidação por fonte secundária sem novo scan, recusa de fonte antiga/fora da raiz, custo cumulativo duplicado e evento em projeto sem Mustard. Log: `/tmp/mustard-knowledge-native-acceptance.log`; registro: `/tmp/mustard-knowledge-native-acceptance-result.json`. Fixture removida. Scripts usados no desenvolvimento não são dependências do produto.

Cada entrega registrará aqui comandos, resultados e limitações. Testes com provedor falso não serão apresentados como prova de economia ou qualidade semântica do Jev real. Revisão desta implementação pelo próprio executor não será chamada de validação independente.

### Aplicação dos conceitos pesquisados ao scan

Implementação própria no motor Rust existente, sem acrescentar dependências ou incorporar os projetos consultados. Code Context Graph orienta as anotações explícitas de intenção/regras/condições/efeitos; Codebase Memory MCP e Serena orientam descoberta seguida de navegação por símbolo/consumidores; ContextGraph orienta a revisão incremental das interpretações antigas. SQLite e projeção progressiva seguem os conceitos também presentes em ckg/CodeGraph-Rust. SCIP permanece referência para precisão; não foi integrado um indexador SCIP/LSP.

`run knowledge --symbol <id> --direction callers` recupera consumidores estáticos da declaração exata, com origem/distância, fontes conferidas no checkout e lacunas por profundidade/validade. `outgoing` segue chamadas/referências e `both` os dois sentidos. `--refresh` apresenta fila de revisão das interpretações afetadas, sem gerar texto ou renovar fontes automaticamente. Anotações entram na recuperação e no contexto preparado com versão por evidência integral; presença de regra escrita não prova sua execução. Exportação Markdown é nativa e local.

As instruções de sessão ficaram menores que as anteriores: **2.468 bytes pt-BR e 2.500 bytes en-US**, mantendo a referência ao estilo e espaço para a retomada. Testes do bloco de sessão aprovados. Documentação e limites em [Scan como oráculo](2026-10-08-scan-oraculo.md#conceitos-da-pesquisa-aplicados-ao-produto); benchmark histórico de 12 perguntas preservado, sem alegar nova medição de custo/acerto nesta etapa.

O aceite dos executáveis desta continuação instala a cópia em pasta temporária e exercita anotações, navegação, combinações inválidas, fila de revisão, Markdown, validade das fontes e ausência de publicação automática. Registro operacional local: `/tmp/mustard-concepts-native-acceptance-result.json`; log: `/tmp/mustard-concepts-native-acceptance.log`. Os programas e seus hashes estão no manifesto do pacote isolado, sem adoção pela instalação pessoal.

## Contratos retirados

Os testes abaixo exigiam substituir ou enriquecer a busca antes da execução, chamar Jev em Grep/rg ou medir esse gancho antigo. Esse comportamento foi retirado. A nova prova exercita passagem fiel e compactação após execução; as demais proteções de leitura/escrita permanecem testadas. A régua histórica deixa de servir como aceite do gancho novo; a comparação futura precisa usar resultados executados.

- `hooks::bash::reading::tests::a_find_by_name_runs_with_one_line_of_the_mark`
- `hooks::bash::reading::tests::a_find_that_starts_a_text_search_is_answered_like_the_recursive_grep`
- `hooks::bash::reading::tests::a_git_grep_for_a_mapped_name_is_answered_by_function`
- `hooks::bash::reading::tests::a_git_grep_the_answer_cannot_stand_for_passes`
- `hooks::bash::reading::tests::a_message_of_the_user_after_the_last_speech_leaves_the_speech_of_the_terminal_search_empty`
- `hooks::bash::reading::tests::a_partial_terminal_search_of_a_subagent_gives_the_filter_the_speech_of_the_subagent_and_never_the_main_one`
- `hooks::bash::reading::tests::a_partial_terminal_search_gives_the_filter_the_description_and_the_last_speech_of_the_agent`
- `hooks::bash::reading::tests::a_partial_terminal_search_records_its_measured_call_in_the_conversation_spec`
- `hooks::bash::reading::tests::a_path_with_a_wildcard_is_answered_as_its_folder_with_the_name_filter`
- `hooks::bash::reading::tests::a_recursive_search_for_a_mapped_name_is_answered_by_function`
- `hooks::bash::reading::tests::a_read_and_a_partial_search_on_one_line_run_whole_with_the_note`
- `hooks::bash::reading::tests::a_search_that_only_lists_names_or_counts_runs_plain_with_one_line_of_the_mark`
- `hooks::bash::reading::tests::a_search_the_map_finds_nothing_for_runs_plain_with_one_line`
- `hooks::bash::reading::tests::a_search_inside_a_working_copy_rereads_the_files_the_wave_changed`
- `hooks::bash::reading::tests::a_search_whose_exclusions_leave_mapped_code_in_is_answered`
- `hooks::bash::reading::tests::a_search_with_a_word_the_map_lacks_is_answered_as_pinned`
- `hooks::bash::reading::tests::a_word_in_the_text_language_finds_the_names_in_the_code_language`
- `hooks::bash::reading::tests::a_search_the_answer_cannot_stand_for_passes_without_error`
- `hooks::bash::reading::tests::an_output_filter_with_a_folder_answers_with_the_files_outside_it`
- `hooks::bash::reading::tests::the_same_search_repeated_in_the_session_passes`
- `hooks::bash::reading::tests::the_answer_never_carries_the_key_and_the_plain_search_still_hides_it`
- `hooks::bash::reading::tests::the_answer_speaks_the_text_language_of_the_project`
- `hooks::write::write_gate::tests::a_glob_with_a_word_of_the_name_runs_with_one_line_of_the_mark`
- `hooks::write::write_gate::tests::a_partial_grep_runs_with_a_note_and_no_permission_while_the_pinned_one_is_denied`
- `hooks::write::write_gate::tests::a_partial_grep_through_the_hook_records_its_measured_call_in_the_conversation_spec`
- `hooks::write::write_gate::tests::a_partial_grep_whose_filter_fails_records_the_call_with_the_reason`
- `hooks::write::write_gate::tests::a_search_for_a_mapped_name_in_a_code_folder_is_answered_by_function`
- `hooks::write::write_gate::tests::a_search_that_only_lists_names_or_counts_runs_plain_with_one_line_of_the_mark`
- `hooks::write::write_gate::tests::a_search_the_filter_never_judges_or_a_session_with_no_spec_records_no_call`
- `hooks::write::write_gate::tests::a_search_inside_a_working_copy_rereads_the_files_the_wave_changed`
- `hooks::write::write_gate::tests::a_search_the_map_finds_nothing_for_runs_plain_with_one_line`
- `hooks::write::write_gate::tests::a_search_whose_glob_leaves_out_all_the_mapped_code_passes`
- `hooks::write::write_gate::tests::a_search_with_a_word_the_map_lacks_is_answered_as_pinned_without_asking_again`
- `hooks::write::write_gate::tests::the_answer_never_carries_the_key_and_the_plain_search_still_hides_it`
- `hooks::write::write_gate::tests::the_answer_key_off_lets_the_search_pass`
- `shared::word_search::ruler::tests::a_conversation_without_the_agent_speech_before_the_call_is_a_search_without_speech`
- `shared::word_search::ruler::tests::a_filter_that_fails_is_a_failure_and_a_search_it_never_got_is_not_a_call`
- `shared::word_search::ruler::tests::a_search_the_filter_answers_records_its_charge_and_the_group_sums_it`
- `shared::word_search::ruler::tests::a_right_file_the_map_ranks_below_the_fifth_is_told_apart`
- `shared::word_search::ruler::tests::a_search_whose_right_file_is_outside_the_map_is_impossible_and_stays_out_of_the_hit_rate`
- `shared::word_search::ruler::tests::a_search_whose_right_file_never_went_to_the_filter_is_impossible`
- `shared::word_search::ruler::tests::a_search_with_a_recorded_session_gives_the_filter_the_speech_before_the_call_and_never_a_later_one`
- `shared::word_search::ruler::tests::a_search_without_the_session_file_goes_on_without_speech`
- `shared::word_search::ruler::tests::the_cut_keeps_what_was_written_up_to_the_second_of_the_call_in_any_time_zone`
- `shared::word_search::ruler::tests::the_filter_of_the_warm_up_search_is_summed_apart_from_the_groups`
- `shared::word_search::ruler::tests::the_partial_note_the_hook_gives_is_read_for_its_file_and_its_code`
- `shared::word_search::ruler::tests::the_ruler_hears_what_the_hook_answers_to_the_same_text_in_the_same_folder`
- `shared::word_search::ruler::tests::the_thermometer_gives_the_place_among_the_shown_files_and_the_cause_of_each_miss`


O gerador de sincronização automática `pages/copy` foi retirado. Restam o filtro de segredos usado pelos fluxos e o reconhecimento de recibos históricos; o painel lê o estado e a exportação explícita usa uma lista permitida. Os testes do gerador retirado são substituídos por provas de ausência de geração no fluxo, exportação sanitizada, concorrência/idempotência e compatibilidade dos registros históricos. Não há caminho de produção antigo restaurado apenas para satisfazer testes. Casos retirados com o gerador:

- `pages::copy::tests::the_step_that_closes_the_preparation_runs_with_the_lock_held`
- `pages::copy::tests::the_copy_carries_only_the_items_after_the_last_copied_number`
- `pages::copy::tests::a_secret_outside_the_text_field_is_withheld_too`
- `pages::copy::tests::a_republished_page_restarts_the_copy_from_zero`
- `pages::copy::tests::the_copy_after_a_same_address_republish_sends_only_what_is_missing`
- `pages::copy::tests::a_purge_after_the_copy_sends_the_clean_item_again_or_takes_it_out`
- `pages::copy::tests::the_copy_order_pins_the_documents_it_overwrites`
- `pages::copy::tests::the_copy_order_always_teaches_what_to_do_with_a_refused_version`
- `pages::copy::tests::copy_without_a_stored_version_still_asks_for_a_read`
- `pages::copy::tests::the_project_row_is_copied_only_when_the_phase_changes`
- `pages::copy::tests::project_line_already_copied_asks_for_the_version_or_goes_with_the_stored_one`
- `pages::copy::tests::line_carried_by_the_copy_of_another_spec_asks_for_the_version_or_goes_with_the_stored_one`
- `pages::copy::tests::every_piece_of_a_range_goes_with_its_stored_version`
- `pages::copy::tests::every_project_row_already_copied_goes_with_its_stored_version`
- `pages::copy::tests::the_old_project_page_gets_the_template_in_a_new_link`
- `pages::copy::tests::first_copy_stays_with_the_orchestrator_without_an_agent`
- `pages::copy::tests::a_long_spec_fits_the_page_database`
- `pages::copy::tests::an_old_installed_template_is_rewritten_before_the_first_publish`
- `pages::copy::tests::the_installed_template_is_compared_by_the_whole_stamp`
- `pages::copy::tests::a_page_with_an_old_layout_is_not_published_again_and_the_user_is_told_once`
- `pages::copy::tests::a_notice_for_another_layout_version_does_not_count`
- `pages::copy::tests::the_project_page_notice_recorded_in_another_spec_counts`
- `pages::copy::tests::a_copy_after_a_request_leaves_the_notice_to_the_next_milestone`
- `pages::copy::tests::a_new_page_is_published_and_the_user_is_told_where_the_link_is`
- `pages::copy::tests::another_mustard_version_with_the_same_layout_does_not_publish_again`
- `pages::copy::tests::the_project_page_stamp_recorded_in_another_spec_counts`
- `pages::copy::tests::the_batch_result_records_the_copy_with_its_versions`
- `pages::copy::tests::a_batch_elsewhere_or_a_failed_one_records_nothing`
- `pages::copy::tests::the_batch_result_as_an_object_records_the_copy_with_its_versions`
- `pages::copy::tests::with_two_batches_only_the_last_records_with_both_versions`
- `pages::copy::tests::a_preparation_arriving_while_the_witness_works_waits_for_it`
- `pages::copy::tests::the_same_result_read_twice_at_once_records_once`
- `pages::copy::tests::a_late_return_from_an_older_preparation_records_nothing`
- `pages::copy::tests::the_spend_line_sums_tokens_without_a_file_ruler`
- `pages::copy::tests::the_spend_line_shows_up_without_a_delivered_file`
- `pages::copy::tests::spend_line_shows_turns_per_task`
- `pages::copy::tests::the_record_of_a_cut_line_never_goes_to_the_page_copy`
- `pages::copy::tests::the_spend_line_stays_out_without_any_token`
- `pages::copy::tests::a_real_round_carries_the_spend_numbers_to_the_computed_document`

## Continuação: transporte nativo

Destino sugerido ao usuário: Cloudflare Pages Direct Upload, sem turno do modelo,
Jev ou ferramentas auxiliares instaladas para gerar/enviar páginas. Configuração
operacional e comandos em `MUSTARD-COMMANDS.md`. O layout de consumo passou para
o mesmo motor estático das publicações de specs. Os antigos produtores de
ArtifactData, template de gasto e fixtures sem consumidores foram removidos;
ledger/histórico pessoal não foram apagados.

A tentativa interativa de Mods usou somente uma instalação/sessão temporária,
sem turno de modelo. A sessão não reconheceu autenticação e não exibiu os comandos.
A conferência posterior identificou também que `defaultEnabled:false` exige
habilitar `mustard@inline` quando o pacote é passado por `--plugin-dir`; a
tentativa anterior não isolou esse requisito. O aceite visual real segue
pendente. Credenciais temporárias e fixture foram removidas, sem alterar a
configuração pessoal. Resultado local em
`/tmp/mustard-mods-interactive-result.json`.

Resultados finais estão na seção de provas acima. Calibração paga do Jev,
aceitação autenticada de Cloudflare/Mods, revisão independente, Windows e o
piloto semântico do scan continuam sem serem substituídos por mocks.

Esclarecimentos incorporados: `/mustard-pages project` funciona sem uma spec
aberta; `/mustard-pages spec` resolve a spec da branch atual no binário;
`/mustard-pages spec <nome>` usa uma escolha explícita; `/mustard-pages report
arquivo.md` publica somente o relatório solicitado no layout existente. Análises
ou resumo para gestores podem exigir raciocínio do modelo, mas Markdown→HTML,
recursos e envio são mecânicos/nativos. O adaptador inicial não envia anexos
locais implicitamente. O layout não depende do fornecedor de hospedagem.

A observação de eventos Mods não grava novamente recibos de entrega/validação.
Os gates clássicos permanecem no binário. O teste do botão revelou que chamar o
comando do próprio plugin pode contornar seu handler; botão e comando agora
compartilham a função local que executa o binário e apresenta o resultado.
Nove testes do kit oficial passaram, incluindo botão de projeto sem spec,
coalescência por eventos e caminho de relatório com espaços. Esse aceite do kit
continua distinto do aceite numa sessão autenticada real.

### Análises, resumos e capacidade da hospedagem

O mesmo motor de layout gera projetos, specs, consumo e relatórios Markdown.
Publicar uma análise não depende de o Cloudflare entender seu conteúdo: o
binário prepara HTML/JSON e a hospedagem serve esses recursos. O raciocínio para
produzir uma análise nova continua pertencendo ao modelo, quando necessário.
Não se promete que o renderizador atual reproduza qualquer infográfico ou envie
anexos locais; o suporte atual de relatório cobre o documento textual e tabelas.

O plano gratuito de Pages admite 20.000 arquivos por site e 25 MiB por arquivo;
a publicação atual usa dois arquivos e verifica esse limite de tamanho. Isso
acomoda relatórios textuais comuns, mas não é uma prova de envio na conta do
usuário. Fonte: [limites oficiais](https://developers.cloudflare.com/pages/platform/limits/).
As publicações estáticas atuais não implementam as permissões de audiência e os
comentários colaborativos integrados dos
[Artifacts do Claude](https://code.claude.com/docs/en/artifacts). Essas capacidades
são requisitos próprios, caso necessários; não devem ser anunciadas como
presentes somente porque uma página foi hospedada.

## Continuação: configuração no mustard.json

Em 08/10, o usuário confirmou que a configuração de Cloudflare pode ficar no
`mustard.json`. O bloco opcional `publication` contém `provider`, `accountId` e
`projectName`; a autenticação continua exclusivamente no ambiente
`CLOUDFLARE_API_TOKEN`. A confirmação conserva a publicação sob pedido explícito.
O preenchimento dos identificadores reais aguarda o Account ID e o nome do
projeto Direct Upload fornecidos pelo usuário.

O runtime agora distingue arquivo de configuração ilegível, destino inválido e
token ausente antes de tentar comunicação remota. Os diagnósticos orientam a
correção sem incluir valores de credenciais ou o corpo da configuração.

Provas adicionais sobre o commit-base `c6eab2b9`:

- 10 testes focados no transporte aprovados, incluindo os dois novos casos de
  configuração inválida. Comando: `cargo test --locked -p mustard-rt --lib
  shared::publication`; log: `/tmp/mustard-cloudflare-config-tests.log`. Essa
  contagem inclui testes existentes; não deve ser somada à suíte geral anterior.
- Build dos três programas aprovado em `/tmp/mustard-cloudflare-config-build.log`.
  Lint estrito do workspace e de todos os alvos aprovado em
  `/tmp/mustard-cloudflare-config-lint.log`.
- Instalação e `mustard config --yes` exercitados com os executáveis reais em
  projeto temporário. Preservaram o bloco Cloudflare, uma opção futura e as
  configurações pessoais. A publicação explícita leu o destino configurado e
  informou token ausente, sem envio remoto. Log:
  `/tmp/mustard-cloudflare-config-native.log`; fixture removida ao terminar.

O pacote local de revisão é atualizado com os três executáveis identificados
pelo commit dessa continuação. A instalação em uso e seu `mustard.json`
permanecem preservados enquanto faltam os identificadores reais. Aceites
autenticados de Cloudflare/Mods, medição paga de Jev, Windows e revisão
independente mantêm as limitações já registradas.

## Continuação: teste pelo usuário no Claude Code

O usuário confirmou que fará o aceite na sessão real do Claude. O pacote local
permite isso sem instalar uma release: `--plugin-dir` seleciona a cópia em
revisão, `--settings` habilita `mustard@inline:true` somente na sessão e o PATH
aponta primeiro para os três programas desse pacote. A leitura do CLI confirmou
o plugin `mustard@inline` habilitado, com escopo `session` e o caminho do pacote
de revisão; isso comprova reconhecimento da configuração, não aceite visual.
O CLI do pacote prepara/atualiza os arquivos locais do projeto de teste.

Foram corrigidas três referências ao nome antigo `/mustard-publish` que ainda
estavam em `continue.md` e nos mapas de sessão pt-BR/en-US. Cinco testes de
instalação/entrega do mapa ao início de sessão passaram em
`/tmp/mustard-claude-handoff-tests.log`. A busca nos textos ativos do plugin e nos
templates confirma o uso de `/mustard-pages`. O procedimento está em
`MUSTARD-COMMANDS.md`, seção de teste da versão de desenvolvimento.

O usuário deve conferir `/plugin`, `/mustard-panel`, statusline e uma alteração
pequena pelo fluxo de levantamento, aprovação, ondas e fechamento. O resultado
real ainda precisa ser registrado. A preparação não publicou páginas, iniciou
um turno de modelo, atualizou a instalação permanente ou adotou a branch no
checkout original.
