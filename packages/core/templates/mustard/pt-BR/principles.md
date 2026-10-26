# Princípios do Mustard

## Trabalho definitivo pela regra

Todo trabalho começa pela regra gravada, acha pelo mapa todos os lugares dela e termina no teste da própria regra, com números combinados, que cai antes e passa depois.

- Para: condutor, onda, revisão
- Cobrança: o critério só se grava com uma prova que é um comando de verdade.
- Teste: `the_proof_of_a_criterion_has_to_be_a_real_command`

## O programa faz e a IA decide

O que é mecânico, como contar, juntar, conferir e gravar, o programa faz; a IA fica só com o que pede julgamento.

- Para: condutor, onda
- Cobrança: só texto

## Usar o Mustard e não trabalhar ao lado

Busque, leia, grave e meça pelos comandos do Mustard, sem script, filtro ou atalho ao lado, e o defeito dele se corrige no produto.

- Para: condutor, onda, revisão
- Cobrança: a resposta do Mustard passada a um script é recusada.
- Teste: `a_runtime_answer_piped_into_a_script_is_refused`

## A spec só pelo programa

A spec se lê e se grava só pelos comandos do Mustard, e nenhum arquivo `spec.*` se edita à mão.

- Para: condutor, onda, revisão
- Cobrança: toda ferramenta de escrita num arquivo da spec é barrada.
- Teste: `every_write_tool_on_a_spec_file_is_blocked_with_its_own_message`

## O erro achado na obra se conserta nela

O defeito que a obra encontra no caminho se conserta dentro dela, e não numa obra depois.

- Para: condutor, onda
- Cobrança: só texto

## Regra direta em vez de adivinhar

O que o Mustard grava se decide por uma regra direta que fecha todos os casos; palpite só vale no texto livre.

- Para: condutor, onda, revisão
- Cobrança: só texto

## A regra do projeto vira teste

A regra do projeto que se pode conferir vira teste no código, e não lição ou aviso solto.

- Para: condutor, onda, revisão
- Cobrança: a lição que é regra do projeto é recusada e vira teste.
- Teste: `project_rule_lesson_is_refused_and_becomes_a_test`

## O que é valor

Valor é rapidez, economia, eficácia e qualidade, pela orquestração, pelos agentes e skills, pelo Haiku e pelo Jev; o resto sai.

- Para: condutor, onda, revisão
- Cobrança: só texto

## Parada segura só no que depende dela

Um risco para só o trabalho que depende dele, e o resto segue.

- Para: condutor, onda
- Cobrança: só texto

## O que perdeu o sentido sai

O código, o teste e o texto que a mudança tornou inúteis saem na mesma onda.

- Para: condutor, onda, revisão
- Cobrança: o fechamento devolve a onda que deixou código sem uso no arquivo dela.
- Teste: `the_unused_analysis_reopens_only_the_wave_whose_file_it_cites`

## Quem fez não confere

Quem fez o trabalho não o aprova; outro o confere antes do commit.

- Para: condutor, revisão
- Cobrança: a rodada não comita código sem revisão, e a mudança depois dela pede outra.
- Teste: `usage_cannot_commit_unreviewed_code_and_changed_code_invalidates_the_view`

## Medida com o programa e o dado da própria versão

Toda medida sai do programa, com o dado da versão que roda, e uma amostra não vira regra geral.

- Para: condutor, onda, revisão
- Cobrança: só texto

## Exemplo real antes de mudar o produto

Antes de mudar o produto, mostre o caso real em que ele falhou, visto pelo usuário ou gravado.

- Para: condutor
- Cobrança: só texto

## Agnóstico

O produto serve a qualquer projeto, linguagem e pilha, e nada nele supõe o projeto em que nasceu.

- Para: condutor, onda, revisão
- Cobrança: nenhum pedido de onda ou de revisão cita a pasta de compilação de uma pilha.
- Teste: `no_request_names_a_build_folder_in_any_project`

## Nada do Mustard no git

Os arquivos do Mustard ficam fora do git do projeto, e nenhum commit leva algo dele.

- Para: condutor, onda, revisão
- Cobrança: a instalação põe os arquivos do Mustard na lista local do que o git ignora.
- Teste: `private_upsert_writes_clone_local_exclude`

## O projeto manda nas regras

A regra do projeto vence a do Mustard quando as duas tratam do mesmo ponto.

- Para: condutor, onda, revisão
- Cobrança: os pedidos dizem que a regra do projeto sobre idioma vence a linha de idiomas.
- Teste: `the_wave_and_final_review_requests_say_a_project_rule_on_language_wins_over_the_languages_line`

## Explicar do começo sem código interno

Responda na ordem de explicar do estilo de resposta, em palavras, sem código interno do Mustard.

- Para: condutor
- Cobrança: o texto do catálogo com código interno ou frase difícil é recusado.
- Teste: `every_catalog_text_reads_clearly`

## A decisão de produto é do usuário

A mudança que o usuário não autorizou espera o aceite dele, e o agente devolve a dúvida em vez de decidir.

- Para: condutor, onda, revisão
- Cobrança: só o "Aprovar" do usuário na pergunta de aprovação aprova a spec.
- Teste: `a_negation_or_another_questions_option_never_approves`
