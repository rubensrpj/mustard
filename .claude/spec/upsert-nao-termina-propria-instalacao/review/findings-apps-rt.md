## Quinta revisao: APPROVED — 0 criticos

Guards, moldes, AC-1, AC-2, AC-3, controle e suites (rt 2164, core 674): PASS. Clippy sem uma unica queixa nos arquivos tocados.

### Verificacao que nao aceitou a palavra do implementador
O revisor dirigiu o binario real num diretorio descartavel e conferiu o contrato do CLI contra o `claude` 2.1.239 de verdade: `plugin update` aceita mesmo `-s/--scope` e `-y/--yes` ("required when stdin or stdout is not a TTY"), a forma `plugin@marketplace` parseia, e — o ponto que importa — um `plugin update` que FALHA sai com codigo 1. Ou seja, o teste de sucesso por codigo de saida nao pode reportar `refreshed` para uma atualizacao que imprimiu falha. Duas execucoes seguidas deram diff IDENTICO.

Contencao confirmada: `private_surface.rs:118` fixa `CLAUDE_CONFIG_DIR` na fixture, e nenhum teste toca o registro real da maquina.

Sobre o tom: o revisor confirmou que NAO foi descartado em silencio — a instrucao de reversao do operador esta em `change-log.md` as 08:26 e o motivo em `spec.md`.

### Nao-bloqueantes — os dois de codigo, corrigidos
1. `shorten_paths` so tratava aspas como fronteira; caminho colado a parentese ou `=` sobreviveria. Nenhuma saida real do `claude` produz essa forma hoje, mas foi corrigido: `PATH_BOUNDARIES` passa a incluir parenteses, virgula, igual e dois-pontos, com teste cobrindo quadro de pilha e linha chave-valor.
2. `PluginRefresh` era `pub(crate)` sem consumidor fora do modulo; virou privado, como o molde pede.
3. A unidade propria do tom ainda nao existe — e a proxima, logo apos o PR desta.

### A olhada que o revisor pediu
`mustard.json` esta limpo e mantem `tone: didactic`. Nada se perdeu; o `M` do inicio da sessao era o selo de versao, ja commitado no dev.
