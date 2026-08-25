---
id: spec.carimbo-aprovacao-nao-se-versiona
---

# Um bloco de exclusao de modo privado numa instalacao compartilhada escondeu 45 das 86 unidades do git e deixaria o carimbo de aprovacao ser versionado

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

Um bloco de exclusao de modo privado numa instalacao compartilhada escondeu 45 das 86 unidades do git e deixaria o carimbo de aprovacao ser versionado.

Uma exclusão local do git — `.git/info/exclude` — carrega um bloco de regras do **modo
privado**, que é a instalação feita para não deixar rastro no repositório do cliente. Este
clone não é essa instalação: a configuração do projeto, os ajustes do harness, o guia da raiz,
o censo, os mapas e os 37 moldes de padrão estão todos versionados aqui.

O comentário do próprio código que gera esse bloco diz o que deveria valer aqui: a saída
durável do harness é "deliberately left versioned because in a shared install it belongs to
the repository". E o `.claude/.gitignore`, que está versionado, já implementa exatamente
isso — segura `spec/*/.events/` e `spec/*/.dispatch/`, e diz em texto que o conteúdo da spec
fica versionado.

A consequência foi medida em 24/08/2026: **45 dos 86 diretórios de unidade estão fora do
git, e 41 estão dentro.** A mecânica explica o número. Uma exclusão do git nunca desrastreia
nada — ela só esconde o que nascer depois. Cada bloco novo escrito no arquivo congela o
conjunto já rastreado e torna invisível tudo o que vier a seguir. Foi assim com as specs, e
é o que vai acontecer com os moldes: os 37 de hoje estão rastreados, o trigésimo oitavo
nasceria invisível.

Quem lê `git log .claude/spec/` não vê uma decisão. Vê uma prática abandonada no meio.

E o conserto ingênuo abriria um furo. Um ensaio com a linha em bloco removida stageou 297
arquivos — zero eventos e zero prompts, porque o `.gitignore` versionado faz o recorte
sozinho — mas **entre eles 9 `.approved-by-user` e 13 `.clarified`**. O `approve-spec`
decide pela PRESENÇA do arquivo de carimbo. Versioná-lo faria um clone novo nascer com a
aprovação já dada.

## Usuários/Stakeholders

Quem revisa um pull request deste projeto. Hoje o raciocínio da unidade — a spec, o plano
de ondas, os critérios com seus comandos, os achados da revisão — não viaja com o diff, e
precisa ser reconstruído a partir da lista de commits.

Quem instala este harness em qualquer projeto. A semente do `.claude/.gitignore` é a mesma
para todos, então o furo do carimbo não é deste repositório: é de todo projeto que nascer.

## Métrica de sucesso

Todas as unidades em disco versionadas, e zero carimbo de portão rastreado. A partir daí um
`git add -A` numa unidade nova traz o registro dela sozinho, sem ninguém precisar lembrar.

## Não-Objetivos

- **Escrever o detector.** Nada avisa quando uma instalação compartilhada carrega regras de
  modo privado. Essa é a causa raiz, e é unidade própria — ampliar este conserto para
  absorvê-la é o movimento que já produziu defeito pior neste projeto.
- **Versionar `.events/` e `.dispatch/`.** São 11.256 arquivos e 19,8 MB de log
  append-only e de prompts regeneráveis. O `.gitignore` versionado já os segura e continua
  segurando.
- **Mexer no modo privado em si.** Ele está correto para o que se propõe. O defeito é ter
  sido aplicado a um clone que não é privado.
- **Reescrever história.** As unidades atrasadas entram num commit novo, nunca num rebase.

## Critérios de Aceitação

- **AC-1** — quando a semente escreve um `.claude/.gitignore` num projeto novo, então esse
  arquivo segura `spec/*/.approved-by-user` e `spec/*/.clarified`, de modo que nenhum projeto
  possa versionar um carimbo de portão.
  Command: `cargo test -p mustard-core the_seeded_gitignore_holds_back_the_gate_markers 2>&1 | grep -E "[1-9][0-9]* passed"`
- **AC-2** — quando se pergunta ao git qual regra decide sobre um carimbo de portão, então
  quem responde é o `.claude/.gitignore` versionado, e não a exclusão local — porque só a
  regra versionada viaja para outro clone.
  Command: `git check-ignore -v .claude/spec/x/.approved-by-user | grep -q '^\.claude/\.gitignore:' && git check-ignore -v .claude/spec/x/.clarified | grep -q '^\.claude/\.gitignore:'`
- **AC-3** — quando se faz a mesma pergunta sobre `CLAUDE.local.md`, então quem responde é o
  `.gitignore` versionado, e não a exclusão local.
  Command: `git check-ignore -v CLAUDE.local.md | grep -q '^\.gitignore:'`
- **AC-4** — quando as unidades que TEM um spec.md em disco sao contadas contra as que o git rastreia, entao os dois numeros sao iguais, e nenhum carimbo de portao aparece rastreado
  Command: `test $(ls .claude/spec/*/spec.md 2>/dev/null | wc -l) -eq $(git ls-files ".claude/spec/*/spec.md" | grep -c "^\.claude/spec/[^/]*/spec\.md$") && test -z "$(git ls-files ".claude/spec/*/.approved-by-user" ".claude/spec/*/.clarified")"`
- **AC-5** — o build do projeto passa verde.
  Command: `cargo build --workspace`

## Checklist

- [ ] T1 — a semente do `.claude/.gitignore` ganha `spec/*/.approved-by-user` e
      `spec/*/.clarified`, com um teste que prova que um projeto recém-semeado os segura
- [ ] T2 — o `.claude/.gitignore` deste repositório recebe as mesmas duas linhas
- [ ] T3 — `CLAUDE.local.md` migra do bloco privado local para o `.gitignore` versionado,
      cobrindo também as cópias por subprojeto
- [ ] T4 — o bloco de modo privado sai do `.git/info/exclude` deste clone, com backup
- [ ] T5 — o registro das unidades atrasadas entra num commit, conferindo antes que nenhum
      carimbo, evento ou prompt renderizado veio junto

## Definitions

- **bloco de modo privado** — o conjunto de regras que o instalador escreve em .git/info/exclude quando a instalacao NAO deve deixar rastro no git do cliente; cada bloco entra com o comentario 'this clone only'
- **instalacao compartilhada** — a instalacao em que os artefatos do harness pertencem ao repositorio e sao versionados — e o que este clone e: mustard.json, .claude/settings.json, CLAUDE.md, o censo, os mapas e os 37 moldes estao todos rastreados
- **carimbo de aprovacao** — o arquivo <spec>/.approved-by-user, cujo valor inteiro e nascer de um ato que o modelo nao consegue autorar; o approve-spec decide pela PRESENCA dele, nao pelo conteudo

## Decisions

- fechar o furo dos carimbos ANTES de remover a exclusao
  Reason: o ensaio mostrou que remover primeiro versionaria 9 .approved-by-user e 13 .clarified; como o approve-spec decide pela presenca do arquivo, um clone novo nasceria com a aprovacao ja dada nessas specs
- a regra dos carimbos entra na SEMENTE que escreve o .claude/.gitignore, nao so na copia deste repositorio
  Reason: o arquivo e gerado; corrigir apenas a copia deixa todo projeto novo nascer com o mesmo furo, e este e um furo de portao de aprovacao
- CLAUDE.local.md migra do bloco privado para o .gitignore versionado
  Reason: medido: e a UNICA linha do bloco que morde de verdade e nao esta coberta por nenhum ignore versionado; uma regra que todo clone quer nao pode morar num arquivo que nao viaja
- remover o bloco privado inteiro deste clone, nao so a linha das specs
  Reason: toda linha dele e inerte (o caminho ja esta rastreado) ou e um racha futuro: os 37 moldes estao rastreados hoje, mas o 38o nasceria invisivel pela mesma mecanica que escondeu 45 specs
- NAO escrever agora o detector de instalacao compartilhada com regras privadas
  Reason: e a causa raiz e e unidade propria; ampliar este conserto para absorve-la e o movimento que ja produziu defeito pior neste projeto

## Evidence

- 45 de 86 diretorios de spec estao fora do git e 41 estao rastreados — uma exclusao do git nunca desrastreia, so esconde o que vier depois, entao cada bloco novo congela o conjunto rastreado e torna invisivel tudo o que nascer dali para frente
  Evidence: `.claude/.gitignore:46`
- o proprio .claude/.gitignore, que esta RASTREADO, diz que o conteudo da spec fica versionado e que so os sidecars regeneraveis sao ignorados — e ele ja segura spec/*/.events/ e spec/*/.dispatch/ sozinho
  Evidence: `.claude/.gitignore:46`
- o ensaio com a linha em bloco removida stageou 297 arquivos, 690 KB, com ZERO eventos e ZERO dispatch — o gitignore versionado faz o recorte certo sem ajuda
  Evidence: `.claude/.gitignore:40`
- entre esses 297 entrariam 9 .approved-by-user e 13 .clarified, porque o .claude/.gitignore nao lista nenhum dos dois
  Evidence: `.claude/.gitignore:55`
- o approve-spec decide pela PRESENCA do arquivo de carimbo — alternar o arquivo inverte a decisao
  Evidence: `apps/rt/src/commands/spec/approve_spec.rs:1012`
- o comentario de HARNESS_CLAUDE_FILES declara que essa lista serve ao modo privado e que numa instalacao compartilhada esses caminhos pertencem ao repositorio
  Evidence: `packages/core/src/platform/project_seed.rs:121`
- este clone e instalacao COMPARTILHADA: mustard.json, .claude/settings.json, CLAUDE.md e .claude/mustard/orchestrator.md estao todos rastreados, apesar de estarem listados no bloco privado — a exclusao e inerte para arquivo ja rastreado
  Evidence: `packages/core/src/platform/project_seed.rs:145`
- CLAUDE.local.md nao e coberto por nenhum ignore versionado: sai do bloco privado e fica visivel
  Evidence: `.gitignore:1`
- hipotese REFUTADA — que o instalador tivesse um defeito ao esconder a saida do scan: censo, mapas, capabilities e os 37 moldes estao todos rastreados, entao a instrucao do scan.md de commitar a saida como unidade propria E seguivel hoje; o unico caminho que realmente sumiu foi .claude/spec/
  Evidence: `plugin/commands/scan.md:16`
