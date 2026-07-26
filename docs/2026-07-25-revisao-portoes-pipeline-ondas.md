# Revisão — confiabilidade dos portões no pipeline de ondas

> **Correção posterior (2026-07-25, depois da execução).** A rota de **isolamento por onda** que a seção 6 colocou em 1º lugar (R4, absorvendo o R5) foi **descartada** após três revisões adversariais — a última mostrou que ela perderia o trabalho de toda onda em silêncio. O mecanismo foi removido do código; no lugar dele ficou **uma regra de fluxo**: commitar **uma vez por RODADA**, depois que toda onda da rodada voltou (`plugin/refs/spec/resume-loop.md` §B). A razão é a mesma evidência da seção 5.3: ondas da MESMA rodada são independentes por construção e têm `## Files` auditados disjuntos; ondas de rodadas DIFERENTES são sequenciais. A única exposição real era o `git add -A` varrer o trabalho em voo de uma irmã — e o commit por rodada a elimina, sem cópias e sem etapa de transporte. **Leia as seções 2 (R4, R5, R10), 3 e 6 sob essa correção**; as demais permanecem válidas.
>
> **O que é este documento.** O cruzamento de três fontes sobre os mesmos dez pontos: um levantamento de campo (PRD), o código do Mustard, e a documentação oficial do Claude Code. A documentação é a autoridade; o levantamento é depoimento; o código é o fato. Onde as três divergiram, este documento registra qual venceu e por quê.
>
> **Data.** 2026-07-25. **Base.** `dev`.
>
> **Fontes cruzadas.**
> - Levantamento de campo: PRD "Confiabilidade dos portões e da ordem de operações num pipeline de ondas", produzido por `/btw` numa execução real de seis ondas.
> - Documentação oficial: `code.claude.com/docs` — best-practices, hooks, sub-agents, worktrees, agents, agent-teams, large-codebases, goal, features-overview.
> - Cookbook oficial: `github.com/anthropics/claude-cookbooks` — `patterns/agents` (basic_workflows, orchestrator_workers, evaluator_optimizer).
> - Código: verificado arquivo por arquivo nesta revisão; cada afirmação abaixo cita `arquivo:linha`.
>
> **Escrito em português** por ser documento de decisão do operador. Código, identificadores e nomes de arquivo permanecem em inglês.

---

## 1. Placar

Dos dez requisitos do levantamento:

| Resultado | Quantos | Quais |
|---|---|---|
| Confirmados como escritos | 5 | R2, R6, R7, R8, R9 |
| Confirmados, implementação reescrita pela doc | 3 | R1, R4, R10 |
| Rejeitados | 1 | R5 |
| Sintoma confirmado, causa refutada pelo código | 1 | R3 |

E **seis defeitos que o levantamento não viu** apareceram no cruzamento (seção 4).

---

## 2. Tabela mestra

| # | O levantamento pediu | O Mustard faz (verificado) | O que a doc propõe | Veredito |
|---|---|---|---|---|
| **R1** | varrer também os pacotes vizinhos pelo mapa de exports | varre a string `export X` dentro do subprojeto — `apps/rt/src/commands/review/dependency_precheck.rs:576` | *"finding where a symbol is defined or used can cost many file reads and grep calls → code intelligence plugins connect Claude to a language server"* (large-codebases). O gatilho aparece literal na tabela de features-overview | **Reescrito.** A doc rejeita os dois lados: nem varrer o subprojeto, nem ampliar a varredura. O oráculo de símbolo é o language server. Alcance entre pacotes: `permissions.additionalDirectories` |
| **R2** | executar o critério antes da implementação e exigir que reprove | linter **estático** de tautologia — `apps/rt/src/commands/review/analyze_validation.rs:336` | *"Have Claude show evidence rather than asserting success"* (best-practices). `/goal` exige *"a stated check: how Claude should prove it"*. Cookbook `evaluator_optimizer`: quem julga é separado de quem produz | **Confirmado**, e a causa foi localizada (achado 6) |
| **R3** | normalizar caminhos hostis (parênteses, colchetes, acentos) | matcher byte-a-byte, sem regex e já normalizando separador — `apps/rt/src/hooks/write/boundary_gate.rs:341,366` | *"Put guardrails in hooks. An instruction is a request, not a guarantee. A PreToolUse hook that blocks the edit is enforcement"* (features-overview) — endossa o portão como hook | **Confirmado; mecanismo diferente do suposto.** Os parênteses **são** parte da causa, mas não por escape mal feito: chegam numa anotação ` (new)` que nunca foi retirada do campo de caminho (achado 7). O conserto não é no casador. Ver seção 5 |
| **R4** | verificar isolamento no orquestrador, antes do primeiro despacho | portão age na **primeira edição** — `apps/rt/src/hooks/write/work_branch_gate.rs:361` | `isolation: worktree` no frontmatter do subagente; desde v2.1.216 um comando git apontado para fora da cópia **falha com erro** (`git -C`, `--git-dir`, `GIT_DIR`, `GIT_WORK_TREE`, `cd` antes) | **Confirmado, implementação reescrita.** Não é verificação no orquestrador: é declaração no agente + enforcement da plataforma |
| **R5** | escopar `git add` pela lista de arquivos da onda | lei `add -A` — `plugin/refs/git/git-flow.md:67`, *"NEVER infer a partial scope"* | *"Do the tasks touch the same files? Isolate the work with worktrees"* (agents) | **Rejeitado.** A doc resolve por isolamento, não por escopo de commit. Com árvore própria, `add -A` **é** a fronteira da onda; a lei do projeto sobrevive intacta |
| **R6** | classificar o lock (idade, tamanho, processo dono) em vez de insistir | nenhum tratamento de lock em todo o `apps/rt` | `PostToolUseFailure` — dispara **após a falha** da chamada, com `decision: block` e `reason` | **Confirmado, com evento melhor.** Reativo e automático, em vez de receita no prompt |
| **R7** | auditar arquivo do plano sem onda dona | seam das auditorias já aberto — `apps/rt/src/commands/pipeline/plan_materialize.rs:200` | sem equivalente: a doc não decompõe em ondas. Só reforça que a spec deve nomear arquivos e o que está fora de escopo | **Confirmado.** Território próprio do Mustard |
| **R8** | o agente encaminha o achado para a onda dona | captura no `SubagentStop` funciona | *"Subagents report results back to the conversation that spawned them"* — subagente não fala com subagente. Agent teams tem o canal, mas é experimental, desabilitado por padrão e sem times aninhados | **Confirmado, desenho corrigido:** o agente **reporta**, o **orquestrador** roteia |
| **R9** | emenda de critério como operação de primeira classe | `spec.md` congelado por regra do próprio Mustard | *"Press Ctrl+G to open the plan in your text editor for direct editing"* + `/rewind` — plano editável é o comportamento normal | **Confirmado e reenquadrado.** A rigidez é invenção do Mustard; a emenda reconcilia |
| **R10** | rodada por conflito real de arquivos | filtro por nível de dependência — `apps/rt/src/commands/pipeline/wave_advance.rs:139` | *"Do the tasks touch the same files? Isolate the work with worktrees"* — o critério oficial é o arquivo | **Confirmado, mas rebaixado.** Com isolamento o conflito some; e 7 de 11 planos do histórico não têm nenhuma onda paralela |

---

## 3. O que o cookbook acrescentou

O cookbook oficial confirma dois padrões, e o segundo revelou o buraco mais sério desta revisão.

### Evaluator-optimizer

O avaliador é **separado** de quem produz. Devolve aceite ou rejeição **com crítica**, a crítica volta ao produtor, e o laço roda até aceitar ou atingir um teto de iterações.

É exatamente o desenho do R2 e do R9: quem escreve o critério não pode ser quem atesta que ele funciona. E valida o laço de revisão que o Mustard já implementa (um veredito `rejected` re-emite a rodada até chegar um `approved`).

### Orchestrator-workers — o padrão tem cinco passos, não três

```
COOKBOOK                          MUSTARD (antes desta revisão)

1. orquestrador divide            decompõe em ondas              OK
2. distribui                      despacha por nível             OK
3. workers em paralelo            mesma árvore de trabalho       OK
4. orquestrador COLETA            não existe                     FALTA
5. orquestrador AGREGA            não existe                     FALTA
```

Hoje os passos 4 e 5 são **gratuitos por acidente**: como todos os agentes escrevem na mesma árvore, a coleta acontece sozinha. No instante em que se isola (R4), eles deixam de ser gratuitos e passam a ser obrigatórios.

**Consequência para o levantamento:** o R4 propôs isolamento sem coleta. Isso é o padrão pela metade — entrega separação e perde o resultado. A peça faltante entrou na spec como uma onda própria (ver seção 7).

---

## 4. Os seis defeitos que o levantamento não viu

| # | Achado | Evidência | Destino |
|---|---|---|---|
| 1 | O corte de worktree não-unidade usa `origin/HEAD`, que aqui resolve para `origin/main` — contrariando o `mustard.json#git.flow`, que declara `"*": "dev"`. O mesmo arquivo já lê `integration_bases()` no outro ramo: a informação está carregada e não é consultada | `apps/rt/src/commands/work_unit_open.rs:273-278` | spec, AC-2 |
| 2 | Uma cópia é checkout limpo: código não commitado não viaja. O `/scan` escreve em dois lugares — artefatos em `.claude/` (redirecionados para o checkout principal, seguros) **e**, junto do código, os `## Guards` de cada subprojeto e suas skills `{role}-pattern`. Uma onda isolada leria os guards antigos, que a revisão trata como lei blocante | `packages/core/src/io/workspace.rs:41` | spec, AC-3 |
| 3 | Nada traz o trabalho da onda de volta para a branch da unidade. `git-settle` é o ritual de saída da **unidade** contra a base remota, não de uma **onda** contra sua unidade | `apps/rt/src/commands/git_settle.rs` | spec, AC-5 e AC-6 |
| 4 | O portão de fronteira só extrai caminho **entre crases**; o outro leitor da mesma seção `## Files` aceita com ou sem crase. Dois parsers, um contrato | `apps/rt/src/hooks/write/boundary_gate.rs:211` vs `apps/rt/src/commands/review/dependency_precheck.rs:263` | R3 |
| 5 | Com ondas paralelas, o portão resolve a spec por `wave-{current_wave}-*` — **um** número por spec. Quatro agentes simultâneos são todos julgados contra o `## Files` de uma onda só | `apps/rt/src/hooks/write/boundary_gate.rs:163` | R3 |
| 6 | O linter de critério **isenta** busca por ausência (`--files-without-match`, `grep -L`, `rg -v`) por considerá-la uma pós-condição real. É justamente a que casa zero e sai verde quando o padrão não bate nada | `apps/rt/src/commands/review/analyze_validation.rs:385` | R2 |
| 7 | **A causa-raiz do R3.** O checklist da onda guarda a anotação DENTRO do campo de caminho: `"path": "plugin/agents/<arquivo>.md (new)"`. O portão compara um caminho de disco contra uma string que não pode existir — e avisa sobre arquivos corretamente declarados. Pior: a anotação foi **exigida** por outro verificador, que recusa o plano quando um arquivo novo não está marcado `(new)`. Um pede a marca, outro não a retira | `.claude/spec/*/wave-*/meta.json`, campo `path` | R3 |

O achado 6 é a causa-raiz precisa do sintoma que abriu o levantamento: dois de dez critérios verdes antes de qualquer trabalho existir. O achado 7 foi encontrado por um agente implementador **durante a execução da onda 2** — fora do seu papel e do seu escopo, exatamente a classe de achado que o R8 quer rotear.

---

## 5. Duas correções de rumo registradas

### 5.1 — A auditoria de sobreposição não garante o que parecia garantir

No meio desta análise argumentou-se que a auditoria de sobreposição (`wave-overlap-check`) já garantiria arquivos disjuntos entre ondas paralelas, e isso foi usado para relativizar o R4.

**O argumento estava errado e foi retirado.** Aquela auditoria compara **listas declaradas**, não edições reais, e emite aviso sem bloquear. O portão de fronteira existe justamente porque agentes editam fora da lista declarada — e roda em modo aviso. Foi usar garantia de papel para dispensar garantia de execução, que é o oposto do que a documentação prega.

### 5.2 — A causa do R3 foi refutada cedo demais

O levantamento suspeitou que parênteses e acentos quebravam o casamento de caminhos. Esta revisão refutou a hipótese mostrando que o casador é byte-a-byte, sem regex e sem escape — e ofereceu duas causas alternativas (achados 4 e 5).

**A refutação foi apressada.** Durante a execução da onda 2, um agente implementador encontrou a causa real (achado 7): os parênteses **estão** envolvidos, porque chegam dentro de uma anotação ` (new)` que é copiada para o campo de caminho e nunca retirada. O levantamento estava mais perto do alvo do que esta revisão.

A lição vale além do R3: refutar um mecanismo proposto não refuta o sintoma, e "a hipótese está errada" não é o mesmo que "a causa é outra". Os achados 4 e 5 continuam válidos e independentes.

---

## 5.3 Evidência colhida durante a execução

A implementação da primeira spec produziu três demonstrações ao vivo de requisitos ainda abertos:

| Requisito | O que aconteceu |
|---|---|
| **R2** | O critério AC-4 desta própria spec foi escrito como `cargo test -p mustard-rt <nome>`. O teste vive num alvo de integração separado, então o filtro casou **zero** — e `cargo` saiu **0**. Um critério aprovaria sem executar nada. O que impediu o falso-verde foi a linha `Expect: ok\. [1-9][0-9]* passed`, porque `0 passed` não casa `[1-9]`. Evidência direta de que tornar o `Expect:` obrigatório entrega parte do R2 sem custo |
| **R5** | A onda 2 terminou enquanto a onda 1 ainda escrevia na mesma árvore. Commitar a onda 2 sozinha, sob a lei `add -A`, teria varrido o trabalho em voo da onda 1. Foi feito um commit por **rodada**, não por onda — a mitigação que este documento recomenda na seção 2 |
| **R9** | Emendar os critérios AC-4 e AC-8 exigiu editar o arquivo congelado à mão, porque não existe operação de emenda. A emenda ficou registrada como evento `decision` — que é rastro, não caminho oficial |

Nenhuma dessas foi provocada de propósito. Todas apareceram na primeira execução real depois da revisão, o que reforça a fila da seção 6 em vez de alterá-la.

---

## 6. Fila aprovada

| Ordem | Item | Mudança em relação ao levantamento | Razão |
|---|---|---|---|
| **1** | **R4 + R5** | R4 sobe de 2º; R5 morre dentro dele | Vira configuração declarativa mais a coleta que o cookbook exige. **Já especificado e aprovado** |
| **2** | **R2 + R9** | R9 sobe de 5º para junto do R2 | Único falso-verde da lista. O próprio levantamento chama o R9 de pré-requisito do R2 — então não pode vir depois |
| **3** | **R7** | mantém 3º | Subiu de valor: com isolamento, um arquivo órfão não é tocado por ninguém **nem** volta em nenhuma coleta |
| **4** | **R1** | mantém 4º, troca o escopo | Language server, não varredura ampliada |
| **5** | **R6** | sobe de 6º para 5º | Barato, com evento nativo próprio |
| **6** | **R8** | mantém | Com o roteamento no orquestrador, não no agente |
| **7** | **R3** | desce de 8º — **com ressalva** | O isolamento tira sua urgência. Mas os achados 4 e 5 **crescem** com o paralelismo: se o R4 entrar e as ondas passarem a rodar mais em paralelo, o R3 volta a subir. Reavaliar depois do R4, não antes |
| **8** | **R10** | último | O isolamento remove o conflito que ele otimizaria, e 7 de 11 planos do histórico não têm onda paralela nenhuma |
| — | ~~R5~~ | absorvido | vive dentro do R4 |

---

## 7. Estado da execução

**R4 + R5** → `.claude/spec/isolate-each-wave-s-implementer/`, escopo full, cinco ondas, base `dev`, branch `dev_isolate-each-wave-s-implementer`. Aprovada em 2026-07-25 e **executada por inteiro** — e então **descartada** (ver a correção no topo deste documento): três revisões adversariais reprovaram o mecanismo, a última demonstrando perda silenciosa do trabalho de cada onda. O que **sobreviveu** da execução são três defeitos reais que ela consertou de passagem, independentes do isolamento:

```
corte de worktree não-unidade segue o git.flow declarado, não o origin/HEAD do remoto  (achado 1)
worktree-gc coleta pelo critério declarado {base}_, não por um prefixo agent- inexistente
git worktree list --porcelain passa a reportar checkout detached
```

No lugar do mecanismo ficou a regra de fluxo do `resume-loop.md` §B: **um commit por rodada**.

Os demais oito requisitos permanecem sem trabalho agendado, na ordem da seção 6.

---

## 8. Limites desta revisão

O que foi **verificado**: toda afirmação sobre o código cita `arquivo:linha` e foi lida nesta revisão. As citações da documentação são textuais das rotas listadas no cabeçalho. A contagem de ondas paralelas veio dos onze planos em `.claude/spec/`.

O que **não** foi medido:
- O custo real de compilação a frio numa cópia nova. O comando que apenas percorre `target/` estourou dois minutos; o impacto por onda foi estimado, não cronometrado. Reuso de artefato de build entre cópias ficou fora de escopo por isso — medir antes de dimensionar.
- A frequência com que cada defeito aparece. O levantamento veio de **uma** execução; os defeitos são reais, a taxa de incidência não está medida.
- O comportamento de `worktree.sparsePaths` sob o hook `WorktreeCreate` do plugin. A documentação afirma que um hook configurado substitui a criação nativa por completo, o que torna esse ajuste inerte aqui — inferido da documentação, não testado.
