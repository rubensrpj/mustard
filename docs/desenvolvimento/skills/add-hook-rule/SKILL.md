---
name: add-hook-rule
description: Acrescentar uma regra a um hook existente preservando autorização, comportamento e integração do veredito.
---
# Acrescentar uma regra

Leia o contrato atual do hook e uma regra vigente do mesmo assunto. Para o fim da resposta, a fonte é `apps/rt/src/hooks/task/end_of_turn_check.rs`: `TurnRule::check(&Turn) -> Option<Finding>`, `RULES` e `run_rules`. `Finding` contém `Block(String)`; `Finding::Warn` não existe. Uma regra retorna `None` quando não bloqueia. Clareza que não impede o turno segue o mecanismo próprio de registro para a entrada seguinte; não invente uma variante de aviso.

1. Coloque a regra no módulo do assunto e use os dados já fornecidos por `Turn`. Não releia o payload ou recrie resolução de projeto/sessão/idioma.
2. Registre em `RULES` na ordem do bloqueio. O registro geral e `hooks.json` só mudam se a responsabilidade/evento mudar; adicionar uma regra não cria um hook.
3. Toda nova mensagem do produto tem pt-BR/en-US no catálogo. Confira a implementação atual de `clarity_check`, inclusive a distinção entre idioma que pode bloquear e defeitos armazenados; não copie uma expectativa antiga de `Inject`.
4. Prove primeiro o caso que exige a regra e a passagem legítima; quando houver interação com outra regra, exercite o hook completo. Inclua contexto principal/subagente, autorização e retry pertinentes. Uma prova de helper isolado não demonstra o veredito serializado que o host recebe.
5. Reuse os testes de paridade, autoria e fase. Uma falha em leitura de evidência não prova que o código está correto nem autoriza agir. Não introduza Jev para regras determinísticas, critérios obrigatórios, soma de gastos ou renderização.

As regras de build/provas por entrega e lint/suíte final são do fluxo declarado. Não as replique em uma regra de fim de resposta. Expansão de leitura dirigida continua possível quando o conteúdo mudou ou a prova exige conferir o resultado.
