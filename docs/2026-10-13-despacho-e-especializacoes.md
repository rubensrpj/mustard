# Revisão do despacho, das evidências e das especializações

Esta revisão cobre o registro de uma sessão real de levantamento e retomada, além dos relatos sobre busca, aprovação do objetivo, medições, agentes e skills. A implementação acompanha as melhorias do painel na mesma branch `codex/mods-control-center`.

## Problemas confirmados e correções

| Pedido ou ocorrência | Diagnóstico | Correção e verificação |
| --- | --- | --- |
| Rodada recusada por `skills[1].sha` | O construtor de pedidos não incluía `sha`, embora o contrato do envio o exigisse. Os hashes das referências dentro das skills eram outro dado. | Hash dos bytes efetivamente carregados em cada skill, inclusive manual e gerada. Teste despacha uma onda com duas skills e confere os hashes gravados. |
| Repetir a rodada criava outras ondas | A recuperação remontava lotes ainda não enviados após uma recusa. | Reutiliza os lotes quando não houve mudança posterior do plano; nova alteração autorizada permite reagrupar. Na cópia isolada do incidente, os envios usaram as ondas 3 e 4, preservando seus números na repetição. |
| A spec passou a `running` sem envio aceito | A transição ocorria antes de registrar os envios. A comparação do Claude com a retomada anterior não era uma medição simultânea. | A transição ocorre após registrar o despacho. Planejamento e cópias recuperáveis podem permanecer se houver recusa; a rodada não é uma transação de todo o Git. |
| `doctor` dizia que a instalação estava saudável | Saúde da instalação não comprovava sucesso da última rodada. | `state-health` também mostra a última recusa da rodada e orienta a inspeção. Uma rodada posterior bem sucedida encerra esse aviso. |
| Busca contornada com Bash | O reescritor aceitava apenas uma busca simples. `cd`, encadeamentos e pipes comuns escapavam. | Encaminha segmentos literais reconhecidos, preservando operadores, diretório, argumentos e condições. Pipes recebem os bytes nativos, sem cartões misturados. Expansões e comandos não compreendidos conservam seu caminho de permissão. |
| Claude disse não ter usado o scan | O registro contém cinco cruzamentos de trechos já lidos pelo hook. Portanto, leitura direta não significou ausência total do Mustard. | Mantém Read exato sem uma segunda chamada. Não impõe uma pergunta ou uma busca extra apenas para cumprir uma contagem. |
| Aprovação do objetivo perdida após `/clear` | Uma sugestão aceita podia depender da interpretação da conversa, sem um marcador próprio na retomada. | `goal_approval` reconstrói o estado por origem direta, aprovação nativa da spec ou testemunha da pergunta exata. O clique é persistido no mesmo passo. Não interpreta um “sim” isolado nem pede de novo o objetivo de uma spec já aprovada. |
| Medições ficaram fora da spec | Os recibos nativos só cobriam ondas; faltava anexar uma medida ao ponto durante o levantamento. | Captura também as ferramentas do condutor e acrescenta `run evidence`. Anexa recibo, comando sanitizado, hashes, horário, resultado e interpretação ao ponto, preservando inclusive resposta e estado dos pontos fechados. Repetição não duplica; recibo de outra spec é recusado. |
| Fatos automáticos genéricos | Relação estática e comentário não comprovam comportamento ou efeito medido. | Preserva essa distinção. O próximo ponto mantém seus fatos completos; referências trazem assinatura e coordenadas atuais. O modo condensado preserva os fatos de todos os pontos. Medições são anexadas separadamente e a interpretação continua marcada como não verificada. |
| `grill` com cerca de 39 KB | A resposta incluía um dossiê volumoso junto da lista de pontos. | Entrega índice dos demais pontos e até três referências com coordenadas, mantendo o próximo ponto completo e comandos para expandir. `read agreed` recupera os registros; `read item-<código>` recupera um item. |
| Bloco inicial repetido quatro vezes | `once:false` repetia o guia gerenciado; reinícios também recolocavam o texto inteiro. | Deduplica o conteúdo gerenciado por sessão. Após clear/compact, envia guia curto e retomada. Instruções pessoais continuam integrais. Testes cobrem instalação real, repetição e arquivo personalizado. |
| Chave impressa por script | A proteção dos leitores conhecidos não abrangia scripts inline que citavam o arquivo. | Recusa esses casos comuns de Python/Node e orienta `config-read`, sem executar partes do script. Leitores suportados continuam substituídos por leitura mascarada, preservando os encadeamentos. |
| Agentes e skills grandes, com nomes opacos | A navegação por sufixos repetidos estava sendo exportada como especialização. Uma skill tinha 246 caminhos no cabeçalho; os agentes repetiam todo o executor. | Navegação permanece no índice, com agrupamento por diretórios e nomes legíveis. Só procedimentos específicos, com fontes atuais e revisão do fingerprint, podem ser exportados. O export reutiliza o protocolo comum e usa frontmatter padrão. |
| Ajustar os arquivos no projeto afetado | Quatro skills e quatro agentes eram exports genéricos ainda idênticos aos bytes do ledger de autoria. | Backup antes da limpeza; oito arquivos, total de 50.605 bytes, removidos pelo binário após conferir os hashes. Os três agentes do fluxo foram mantidos; código e itens da spec permaneceram iguais. Edições manuais são preservadas. |
| Catálogo de expertise excessivo | A listagem sem escopo poderia despejar todo o catálogo de navegação. | Mostra procedimentos e uma contagem da navegação interna; `--file` detalha a área pertinente. Perfis inalterados não abrem uma gravação por perfil. |

As afirmações sobre todas as abas de uma planilha, sobre não ter executado o caminho antigo e sobre qual entrada derruba o servidor não foram convertidas em fatos novos. Corrigir essas conclusões exige suas evidências originais e uma reprodução identificada. O recibo comprova a ferramenta observada; não transforma uma interpretação do modelo em verdade.

## Agentes e skills

A revisão aplicou a `skill-creator` disponível na sessão e conferiu a documentação oficial de [skills](https://code.claude.com/docs/en/skills) e [subagentes](https://code.claude.com/docs/en/sub-agents) do Claude. O conteúdo deve ser específico, carregar detalhes quando necessários e preservar as regras e o escopo do projeto. Uma lista de arquivos parecidos não demonstra um procedimento especializado.

O nome completo de uma skill exportada cabe em 64 caracteres. A origem fica em `metadata.source`; a leitura do marcador antigo continua compatível. Os caminhos completos ficam no registro nativo, em vez de ocupar o cabeçalho. Claude pode carregar a skill pelo campo `skills` do agente; disponibilidade na sessão é conferida antes de escolher um agente recém criado. O executor genérico continua disponível com a mesma skill quando o host ainda não descobriu a definição.

Na fixture de exportação, o validador da `skill-creator` aceitou os dois hosts: skill de 553 bytes; agente Claude de 871 bytes e agente Codex de 664 bytes. Isso verifica formato e exportação; não mede qualidade do raciocínio nem cria uma especialização artificial no projeto real.

A instalação executa o scan e a sincronização de expertise. A retirada de exports genéricos antigos, ou de fontes desatualizadas, respeita o ledger de autoria; um arquivo alterado pela pessoa permanece. Nenhum modelo, Jev ou processo adicional do Claude foi necessário para esta limpeza.

## Verificação

- Núcleo: 1.264 testes aprovados, incluindo 38 testes de integração do pacote.
- Runtime: 1.912 testes unitários aprovados; duas medições antigas permanecem ignoradas.
- Integração do runtime: 298 aprovados neste Linux; o ensaio de shell também se aplica ao macOS. No Windows esse ensaio POSIX é omitido.
- Plugin: 56 testes nativos aprovados.
- Clippy de todos os alvos: aprovado com warnings tratados como erro.
- Compilação de release e smoke de instalação/atualização, RTK, busca nativa e ausência de publicação automática: aprovados.

A reprodução usa uma cópia isolada do checkout, a mesma base e o mesmo prefixo da spec, com IA remota desabilitada. O projeto original não recebe envios de teste nem execução de agentes. O despacho aceitou as ondas 3 e 4; repetir a rodada manteve esses números. O `grill` passou de 39.311 para 3.892 bytes, redução de 90,1%, mantendo os cinco pontos e o próximo ponto idêntico. Essa comparação mede bytes, não economia líquida de tokens do LLM. Os resultados finais e hashes ficam em [validação](evidence/2026-10-13-despacho-validation.json).

Reprodução do tamanho de saída:

```bash
python3 scripts/bench-grill.py --root /caminho/projeto --spec nome --before-event 25 --baseline /caminho/v0.2.18/mustard-rt --candidate /caminho/candidato/mustard-rt --out /tmp/grill-benchmark.json
```

Os harnesses de benchmark usam o binário indicado, sem o redirecionamento de desenvolvimento para outra instalação. O protocolo de produção continua preservado. O Desktop Windows por SSH e os turnos autenticados de Haiku/Luna continuam sendo aceitação do usuário após a release, conforme combinado; os testes locais não afirmam cobrir essas sessões.
