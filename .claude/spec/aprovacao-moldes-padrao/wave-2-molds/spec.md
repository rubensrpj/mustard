---
id: wave.aprovacao-moldes-padrao.2-molds
---

# wave-2-molds

## Summary

A instrucao entregue ao autor de moldes passa a mostrar paths na forma YAML exata que o molde deve carregar, o validador passa a conferir os quatro titulos e a tolerar as duas formas de paths, e o relay para de responder ok:true para um arquivo que leu e nao entendeu.

## Network

- Parent: [[spec.aprovacao-moldes-padrao]]

## Tasks

- [ ] Em agent/render/role.rs, o worklist deixa de imprimir os valores de paths juntados por virgula numa linha so. Passa a imprimir o bloco YAML literal que o molde deve carregar, indentado, de modo que copiar ao pe da letra — que e o que a instrucao manda — produza a forma que o validador aceita. Esta e a causa das 19 recusas em 79 moldes medidas em campo: a instrucao brigava com o validador, o agente obedeceu a instrucao.
- [ ] Em scan_patterns/apply.rs, declared_paths passa a ler tambem a forma inline escalar (paths: valor) e a sequencia de fluxo (paths: [a, b]), alem da lista em bloco que ja le. A checagem existe para provar o VALOR copiado do worklist; a forma YAML nao e o que ela mede, e recusar por forma custou tres re-execucoes e cerca de 110 mil tokens sem provar nada.
- [ ] Ainda em apply.rs, normalizar a forma na ESCRITA: um molde aceito com paths inline e gravado com paths em lista em bloco, que e a forma canonica dos moldes ja existentes. Tolerar na leitura sem normalizar na escrita deixaria em disco uma forma que a plataforma pode nao ler.
- [ ] Adicionar a apply.rs a checagem estrutural que falta: os quatro titulos canonicos (## Purpose, ## Convention, ## How to apply, ## Examples), cada um exatamente uma vez, nessa ordem, e nenhum outro titulo de nivel dois. O prompt ja contrata exatamente isso e o validador nunca conferiu — foi assim que dois moldes ficaram gravados com ## How to apply duplicado e sem ## Convention, e com ## Examples no meio. Um molde e escrito uma vez e depois carrega sozinho em toda edicao da pasta, entao um defeito de forma e permanente.
- [ ] Antes de fechar a onda, rodar a checagem nova contra TODOS os moldes -pattern que este repositorio ja carrega e confirmar que passam. Eles foram medidos como conformes; se algum reprovar, a checagem e que esta errada.
- [ ] Em scan_patterns/relay.rs, o relatorio de lido-e-sem-blocos deixa de ser condicionado a from_json e passa a valer para o canal de ARQUIVO inteiro. Um arquivo que foi lido, nao e JSON reconhecivel e nao demarca nenhum bloco cai hoje em Envelope::Raw e volta a imprimir ok:true blocks:0, que se le como o agente nao devolveu nada quando na verdade foi nao consegui interpretar. O envelope literal passado direto em --content mantem seu relatorio vazio fail-open, sem virar um modo de falha novo.
- [ ] Testes cobrindo: o worklist entregando o YAML copiavel; um molde com paths inline aceito e gravado como lista; um molde com titulo faltando, duplicado ou fora de ordem recusado e um molde correto aceito; e um arquivo lido, nao-JSON e sem blocos relatado como ok:false nomeando o arquivo.

## Files

- `apps/rt/src/commands/agent/render/role.rs`
- `apps/rt/src/commands/scan_patterns/apply.rs`
- `apps/rt/src/commands/scan_patterns/relay.rs`
- `apps/rt/src/commands/scan_patterns/mod.rs`
