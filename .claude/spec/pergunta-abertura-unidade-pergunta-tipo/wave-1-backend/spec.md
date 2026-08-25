---
id: wave.pergunta-abertura-unidade-pergunta-tipo.1-backend
---

# wave-1-backend

## Summary

O portão passa a aceitar um nome escolhido pelo operador, por um sinal explícito que ganha da derivação — sem afrouxar a lei de um nome só.

## Network

- Parent: [[spec.pergunta-abertura-unidade-pergunta-tipo]]

## Tasks

- [ ] Em apps/rt/src/commands/event/emit_pipeline.rs, acrescentar ao EmitPipelineOpts um sinal EXPLÍCITO de nome escolhido pelo operador, distinto do --spec de hoje: --spec continua sendo palpite e continua perdendo para a derivação (com renamedFrom e a linha de stderr), enquanto o sinal novo GANHA. Canonizar o valor recebido pela mesma função que deriva o nome (spec_slug::canonical_for_project) para que um nome digitado com espaços, acentos ou barra vire o mesmo formato de slug — a unidade continua tendo exatamente um nome, e ele continua com uma grafia só.
- [ ] Registrar no relatório JSON de onde veio o nome que venceu (a derivação ou o operador), de modo byte-estável, sem timestamp nem caminho volátil.
- [ ] Em apps/rt/src/commands/event/cli.rs, declarar o sinal novo na variante EmitPipeline e repassá-lo no braço do dispatch — sem os dois, a flag existe e não chega a lugar nenhum.
- [ ] Atualizar a documentação da própria função mint_unit_name_at: hoje ela diz que preferir a grafia do chamador em silêncio foi o que criou o defeito dos dois nomes. Explicitar a distinção que este trabalho introduz — o chamador em silêncio continua perdendo; o operador que corrige de propósito ganha.
- [ ] Escrever a catraca operator_name_wins_over_the_derivation cobrindo os dois lados: com o sinal, o nome do operador nomeia a unidade; sem ele, um --spec discordante continua perdendo e ainda reporta renamedFrom.

## Files

- `apps/rt/src/commands/event/emit_pipeline.rs`
- `apps/rt/src/commands/event/cli.rs`
- `apps/rt/tests/run_command_surface.rs`
