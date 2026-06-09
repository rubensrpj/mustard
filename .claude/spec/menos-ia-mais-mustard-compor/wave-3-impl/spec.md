# wave-3-impl

## Resumo

RT: comandos compostos plan-materialize, wave-advance e close-pipeline (composicao direta, sem fachada)

## Rede

- Pai: [[menos-ia-mais-mustard-compor]]
- Depende de: [[wave-2-impl]]

## Tarefas

- [ ] - [ ] Estudar como os comandos existentes se chamam internamente: cada subcomando run tem uma fn de entrada com Options (padrao do crate). Composicao = chamar essas fns (ou as fns de dominio que elas usam) DIRETAMENTE no mesmo processo; PROIBIDO shellar para o proprio binario e PROIBIDO copiar logica. Se uma fn de comando nao for chamavel (le argv, imprime no meio), extraia o miolo para uma fn pura reutilizada por ambos (refactor minimo, sem wrapper morto).
- [ ] - [ ] `plan-materialize --spec-dir <dir> --plan <plan.json>`: compoe wave-scaffold + analyze-validation (ja com a checagem de AC da onda 2) + emit-pipeline scope + emit-phase PLAN. Pressupoe spec.md/meta.json ja materializados pelo spec-draft (que continua separado porque a IA folda o corpo ENTRE draft e scaffold). Saida JSON unica {scaffold:{created_files}, validation:{issues}, events:[...]}, byte-estavel.
- [ ] - [ ] `wave-advance --spec <slug>`: compoe dispatch-plan (incl. spec unica, onda 2) + para cada item do PROXIMO nivel pendente, renderiza o prompt inline (chamando o miolo do agent-prompt-render) e devolve [{wave, role, subproject, subagent_type, prompt}] com o texto pronto — o orquestrador despacha Tasks direto, sem N rodadas de prompt_cmd. Nivel pendente = itens cujas dependencias estao completas e que ainda nao foram despachados (use as projecoes de eventos existentes; se nao houver sinal confiavel de 'despachado', devolva o nivel 0 nao-completo e documente).
- [ ] - [ ] `close-pipeline --spec <slug>`: compoe verificacao de review.results (eventos existentes; advisory), qa-run, e — somente com QA pass — complete-spec + pipeline-summary. Saida JSON {reviews, qa:{overall,criteria}, completed:bool, summary}. QA fail -> completed=false com os AC reprovados (sem fechar).
- [ ] - [ ] Registrar os 3 no enum RunCmd + dispatch() (os DOIS registros cada). Saidas deterministicas (JSON ordenado, sem timestamps volateis).
- [ ] - [ ] Testes `composite_*` por comando: caminho feliz (fixture de spec com waves em tempdir) + degradado (spec inexistente; QA fail nao fecha; nivel sem pendencia devolve lista vazia). Espelhar o estilo dos testes de approve_spec/complete_spec.
- [ ] - [ ] Rodar `cargo test -p mustard-rt` completo; reportar numeros.

## Arquivos

- `apps/rt/src/commands/mod.rs`
- `apps/rt/src/commands/pipeline/plan_materialize.rs`
- `apps/rt/src/commands/pipeline/wave_advance.rs`
- `apps/rt/src/commands/pipeline/close_pipeline.rs`
