---
id: spec.trim-the-strong-digest-stdout
---

# trim the strong digest stdout: narrow the published candidate pool and drop the terms anchorsDetail duplicates

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

**Hoje.** Toda vez que uma spec nasce, o localizador (`mustard-rt run feature`) imprime um relatório
que entra inteiro na janela de contexto. Medido nesta sessão, numa intenção real com resultado
**forte**: **8.605 caracteres**. Desses, o campo `candidates` — a lista de 25 arquivos candidatos —
são **2.822** (33%), e o campo `anchorsDetail` são **1.257** (15%).

**Por que isso é um problema.** A própria instrução que consome esse relatório manda, no caso forte,
*"selecionar os 5-10 arquivos que um desenvolvedor abriria — nunca todos os ~25"*. Ou seja: o
relatório entrega 25 e o leitor tem ordem de usar no máximo 10. Os outros 15 são contexto pago e
descartado, em todo pedido forte — que é o caso comum.

E `anchorsDetail` repete o que já está ali. Medição desta sessão: **os 12 arquivos de
`anchorsDetail` estão todos entre os `candidates`** — sobreposição de 100%. Cada um traz `terms`,
que já aparecem na linha de evidência do candidato correspondente. A única informação exclusiva do
campo é o `scoreX1024`.

**A armadilha que quase entrou.** O caminho óbvio — cortar o pool antes de montá-lo — quebraria
outra coisa. O campo `uncovered`, o radar de ausência que obriga o orquestrador a resolver cada
conceito sem candidato antes de planejar, é calculado **a partir do pool inteiro**. Estreitar o pool
antes dele faria conceitos genuinamente cobertos aparecerem como não cobertos, e o radar de ausência
é um portão, não um enfeite. O corte tem de acontecer **só na publicação**, depois desse cálculo.

**Por que agora.** É o item de contexto mais barato do plano: o caminho comum fica mais leve sem
que o leitor perca nada que a instrução mande usar.

## Usuários/Stakeholders

Todo pedido que abre uma spec paga esse relatório. O ganho é por invocação, no caminho mais
frequente (resultado forte).

## Métrica de sucesso

O stdout do caso forte encolhe em torno de **um terço** sem que nenhum campo mude de forma: mesmas
chaves, mesmos tipos, mesma ordem. A medição de referência desta sessão é 8.605 caracteres, com
`candidates` em 25 itens.

## Não-Objetivos

- **Esconder `candidates` no caso forte.** Já refutado: [feature.md:29](plugin/commands/feature.md)
  manda selecionar dele exatamente no caso forte. Esvaziar o menu onde a instrução o consome
  trocaria economia por cegueira.
- **Tocar o contrato de `attach_retrieval`.** Ele promete "sempre presente, possivelmente vazio".
  Continua valendo — o corte é na publicação, não na estrutura.
- **Mexer no cálculo de `uncovered` ou na fusão.** O pool inteiro segue alimentando os dois.
- **Estreitar o caso fraco.** Ali o planejamento já é retido; não há o que economizar.

## Critérios de Aceitação

- **AC-1** — when o relatório sai com resultado forte, then o campo `candidates` publica no máximo
  o novo teto, e não os 25 de antes
  Command: `cargo test -p mustard-rt strong_pool_is_narrowed`
  Expect: `[1-9][0-9]* passed`
- **AC-2** — when o relatório sai com resultado fraco ou nenhum, then a publicação NÃO é estreitada,
  porque o corte vale só para o caso forte
  Command: `cargo test -p mustard-rt non_strong_pool_is_untouched`
  Expect: `[1-9][0-9]* passed`
- **AC-3** — when o pool é estreitado para publicação, then o campo `uncovered` continua idêntico ao
  que o pool inteiro produzia, porque o radar de ausência é calculado antes do corte
  Command: `cargo test -p mustard-rt uncovered_is_computed_before_the_trim`
  Expect: `[1-9][0-9]* passed`
- **AC-4** — when o relatório sai com resultado forte, then `anchorsDetail` deixa de repetir os
  `terms` que já estão na evidência do candidato, mantendo arquivo e score
  Command: `cargo test -p mustard-rt anchors_detail_drops_duplicated_terms`
  Expect: `[1-9][0-9]* passed`
- **AC-5** — when o conjunto de chaves do relatório é comparado, then ele permanece exatamente o
  mesmo de antes: nenhuma chave somiu, nenhuma nasceu
  Command: `cargo test -p mustard-rt payload_key_set`
  Expect: `[1-9][0-9]* passed`
- **AC-6** — when o localizador roda numa intenção real deste repositório, then o stdout do caso
  forte fica abaixo de 6.500 caracteres (era 8.605)
  Command: `mustard-rt run feature --intent "digest candidates pool strong reason stdout trim anchors detail feature retrieval"`
  Expect: `"reason": "strong"`

## Arquivos

- `apps/rt/src/commands/feature_retrieval.rs` — o teto de publicação do caso forte
- `apps/rt/src/commands/feature.rs` — o corte aplicado só ao que vai para o stdout
- `plugin/commands/feature.md` — a instrução cita "nunca todos os ~25"; o número muda junto

## Checklist

- [ ] T1 — teto de publicação do pool no caso forte, aplicado DEPOIS do cálculo de `uncovered`.
- [ ] T2 — `anchorsDetail` sem os `terms` duplicados no caso forte.
- [ ] T3 — testes dos quatro comportamentos + o conjunto de chaves atualizado, não apagado.
- [ ] T4 — `plugin/commands/feature.md` acompanha no mesmo commit: a instrução diz "nunca todos os
      ~25" e esse número muda.