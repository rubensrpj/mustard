---
id: spec.scope-scan-generated-role-pattern
---

# scope scan-generated role-pattern molds with a paths glob derived from the census, and set the plugin command frontmatter the platform honors

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

**Hoje.** Quando o `/scan` termina, ele deixa em cada subprojeto um punhado de moldes
`{papel}-pattern` — arquivos que ensinam "é assim que se escreve um módulo deste tipo aqui". O
frontmatter (o cabeçalho YAML do arquivo) que ele escreve traz três chaves: `tags`, `appliesTo` e
`scope`. As três são invenção do Mustard, servem ao ranqueador interno (`skill-resolve`) e a
plataforma não olha para nenhuma delas. A chave que a plataforma honra — `paths:`, que limita
quando um molde entra no contexto — não é escrita. Zero dos 19 moldes deste repositório a têm; zero
dos 84 de um projeto real escaneado.

**Por que isso é um problema.** Sem `paths:`, a plataforma cai na regra grossa: tocou um arquivo do
subprojeto, entram as descrições de **todos** os moldes daquele subprojeto. Medido dentro da própria
sessão que escreveu esta spec: ao ler um único arquivo de `packages/core/`, entraram as descrições
dos 8 moldes de core, das quais 1 era pertinente; ao ler um único arquivo do dashboard, entraram
outras 3. Num projeto real de 12 subprojetos são 84 moldes, cujas descrições somam ≈2.500 tokens por
requisição. **Quem escreveu esses arquivos foi o Mustard** — o custo é responsabilidade dele, não do
projeto cliente.

**A armadilha a evitar.** O glob não pode ser o diretório do subprojeto (`packages/core/**`): isso é
exatamente o que a descoberta automática já faz, e o ganho seria zero. Ele precisa ser o diretório
onde a convenção daquele molde realmente mora — e esse dado o censo já tem, no `dirs`/`exemplars`
do cluster. Derivar dali é fato; inventar `src/hooks/**` porque "parece" seria detectar papel por
nome, o que a lei da casa proíbe.

**Junto, o frontmatter dos comandos.** Os 19 comandos do plugin não usam nenhuma das chaves que a
plataforma oferece para baratear invocação. Seis utilitários poderiam declarar que o modelo não os
invoca sozinho; o `/review` poderia rodar em contexto separado. O `/qa` **não** pode — a lei dele é
exit code observado, e um resumo de segunda mão não serve como prova.

## Usuários/Stakeholders

Quem edita código em qualquer projeto que o Mustard escaneou: paga o contexto dos moldes a cada
requisição. E quem mantém o Mustard: os comandos ficam mais baratos de invocar.

## Métrica de sucesso

Ao tocar um arquivo de um subprojeto, entram apenas os moldes cujo glob casa aquele arquivo, em vez
de todos os do subprojeto. Medida no próprio repositório: tocar `apps/rt/src/hooks/*.rs` deixa de
puxar os moldes que descrevem outras áreas de `rt`.

## Não-Objetivos

- **Remover `tags`/`appliesTo`/`scope`.** Elas não são lixo: o `skill-resolve` as consome para
  pontuar relevância. `paths:` soma, não substitui.
- **Migrar os Guards para `.claude/rules/`.** Já refutado: quebra submódulo e some após compactação.
- **Listar moldes em `skills:` de subagente.** Já rejeitado: fixaria os moldes deste repositório em
  toda instalação.
- **`context: fork` no `/qa`.** A lei do exit code observado não é negociável.
- **`effort: low` no `/scan`.** A mineração é binário; o único trabalho de modelo ali é a prosa de
  enriquecimento, justamente o que seria degradado.

## Critérios de Aceitação

- **AC-1** — when o censo registra os diretórios de um cluster de papel, then a worklist de moldes
  entrega o glob daquele cluster derivado **apenas** desses diretórios, sem caminho escrito à mão
  Command: `cargo test -p mustard-rt scan_patterns::list`
  Expect: `test result: ok`
- **AC-2** — when o prompt do papel `patterns` é renderizado, then o contrato do molde exige a chave
  `paths:` com o glob que a worklist entregou
  Command: `cargo test -p mustard-rt render::role`
  Expect: `test result: ok`
- **AC-3** — when um molde traz `paths:` no frontmatter, then o parser o reconhece como campo tipado
  e não como chave desconhecida jogada em `extra`
  Command: `cargo test -p mustard-core frontmatter`
  Expect: `test result: ok`
- **AC-4** — when `scan-patterns-apply` grava um molde cujo corpo autorado traz `paths:`, then a
  chave sobrevive à gravação
  Command: `cargo test -p mustard-rt scan_patterns::apply`
  Expect: `test result: ok`
- **AC-5** — when o frontmatter dos comandos do plugin é auditado, then os seis utilitários carregam
  `disable-model-invocation: true`, o `review` carrega `context: fork` com `agent:` e
  `background: false`, e o `qa` não carrega chave de fork nenhuma
  Command: `cargo test -p mustard-rt command_frontmatter`
  Expect: `test result: ok`
- **AC-6** — when o prompt do papel `patterns` é renderizado para um subprojeto real deste
  repositório, then o contrato do molde na saída exige a chave `paths:`
  Command: `mustard-rt run agent-prompt-render --role patterns --subproject rt`
  Expect: `paths:`
- **AC-7** — when `dependency-precheck` não consegue ler a spec que lhe indicaram, then ele responde
  `ok: false` em vez de aprovar em silêncio, mantendo o campo de erro explícito
  Command: `cargo test -p mustard-rt dependency_precheck`
  Expect: `test result: ok`

> **Extensão de escopo registrada durante o EXECUTE.** O AC-7 não estava no plano aprovado. Ele
> entrou porque, ao rodar o precheck desta própria spec, o comando respondeu `ok: true` e saiu 0
> sobre uma spec que **não conseguiu ler** — verificado contra um caminho inexistente. O fluxo trata
> esse comando como portão ("nunca pule o `dependency-precheck`"), e um portão que aprova sem ter
> lido é a mesma doença que a Onda 1 corrigiu no doctor e no coletor de métricas. A lei da casa
> manda corrigir o achado na mesma passada; por estar fora do escopo aprovado, ele foi dobrado aqui
> em vez de virar registro para depois.

<!-- PLAN -->

## Arquivos

- `apps/rt/src/commands/scan_patterns/list.rs` — a worklist passa a carregar o glob do cluster
- `apps/rt/src/commands/agent/render/role.rs` — o contrato do molde no prompt do papel `patterns`
- `apps/rt/src/commands/scan_patterns/apply.rs` — preservar `paths:` na gravação
- `packages/core/src/domain/skill/frontmatter.rs` — `paths` vira campo tipado
- `apps/rt/src/commands/review/dependency_precheck.rs` — deixa de aprovar spec que não leu
- `plugin/commands/knowledge.md` — `disable-model-invocation`
- `plugin/commands/maint.md` — `disable-model-invocation`
- `plugin/commands/skills.md` — `disable-model-invocation`
- `plugin/commands/stats.md` — `disable-model-invocation`
- `plugin/commands/status.md` — `disable-model-invocation`
- `plugin/commands/upsert.md` — `disable-model-invocation`
- `plugin/commands/review.md` — `context: fork` + `agent:` + `background: false`

## Limites

IN: o glob dos moldes gerados pelo `/scan`; o frontmatter dos comandos do plugin; os testes de
ratchet que impedem as duas fontes de divergirem de novo.

OUT: os moldes já escritos em projetos clientes (só o próximo `/scan` os regenera — os antigos são
varridos por deleção, não por refresh); os Guards no `CLAUDE.md`; o `skill-resolve`.

RISCO REGISTRADO: a documentação afirma que arquivo em `commands/` e skill em `skills/` *"work the
same way"*, mas não exemplifica `context: fork` num arquivo plano de `commands/`. Se a prova viva
falhar, o plano B é mover só o `review` para `plugin/skills/review/SKILL.md` com `name: review` — o
comando continua sendo `/mustard:review`. `background: false` exige Claude Code v2.1.218; esta
máquina tem 2.1.219, outras instalações podem não ter, e a chave é ignorada quando ausente.