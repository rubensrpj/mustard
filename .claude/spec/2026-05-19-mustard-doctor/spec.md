# Feature: mustard-doctor

completed| Phase: CLOSE | Scope: full
### Checkpoint: 2026-05-19T23:30:00Z
### Lang: pt

> **Fase 1 de 2** do epic *Mustard verificável sobre si mesmo* — ver `.claude/plans/self-verifiability-roadmap.md`. Direção derivada da análise competitiva `claude-code-harness` → Mustard (2026-05-19): das seis ideias avaliadas, esta ataca a única fricção **já registrada** na memória do projeto. As demais foram descartadas: já resolvidas (engine nativo, crítico de plano), em conflito com o design (release/PR, advisor) ou nice-não-necessário (review multi-perspectiva). Esta spec é Rust-first, agnóstica e read-only — não adiciona enforcement, adiciona diagnóstico.

## PRD

## Contexto

O Mustard depende de uma malha de hooks de enforcement e de um payload `templates/` que `mustard init` copia para o `.claude/` de cada projeto. Quando essa instalação derrapa — um hook some do `settings.json`, um arquivo copiado fica defasado da fonte, uma referência aponta para um script que não existe mais — nada avisa: os hooks falham em silêncio e o usuário só percebe quando o pipeline já degradou. Não existe hoje um comando que responda à pergunta "minha instalação está saudável?". O diagnóstico vive espalhado em validadores pontuais que ninguém roda em conjunto, e a cópia instalada diverge da fonte sem emitir sinal — uma fricção recorrente e documentada. Esta feature dá ao Mustard um diagnóstico único, read-only, que inspeciona a saúde da instalação e reporta cada categoria como OK, WARN ou FAIL.

## Usuários/Stakeholders

Quem mantém um projeto com Mustard instalado e quem desenvolve o próprio Mustard — ambos hoje sem um sinal de saúde da instalação. A demanda derivou de uma análise competitiva contra o `claude-code-harness` e de fricção registrada na memória do projeto (`feedback_mustard_self_scripts_stale`, `feedback_mustard_performance`).

## Métrica de sucesso

Um único comando — `/mustard:maint doctor` — reporta a saúde da instalação em categorias OK/WARN/FAIL e detecta um hook removido ou uma referência morta sem que o usuário precise saber onde procurar.

## Não-Objetivos

- Não criar novos hooks de enforcement — `doctor` é diagnóstico read-only e nunca bloqueia.
- Não consertar automaticamente o que detecta nem o doc-drift que encontra — `doctor` reporta, não conserta.
- Não escrever um manifesto de hashes no `mustard init`/`update`; sem ele, o check de drift roda só onde `templates/` é alcançável (o repo de desenvolvimento do Mustard) e degrada para `skip` em projeto consumidor. O manifesto fica para o futuro.
- Não emitir JSON nem HTML — a saída é um relatório de texto compacto.
- Nada de JavaScript — o módulo nasce em Rust no crate `mustard-rt`.

## Critérios de Aceitação

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: O crate `mustard-rt` compila com o novo módulo — Command: `cargo build -p mustard-rt`
- [x] AC-2: Os testes do `mustard-rt` passam (cobertura unitária de cada check) — Command: `cargo test -p mustard-rt`
- [x] AC-3: O subcomando `doctor` está registrado na CLI do runtime — Command: `cargo run -q -p mustard-rt -- run doctor --help`
- [x] AC-4: O wrapper `doctor` existe no payload do `/maint` — Command: `bash -c 'grep -q "doctor" apps/cli/templates/commands/mustard/maint/SKILL.md'`

## Plano

## Informações da Entidade

N/A — infraestrutura (diagnóstico da instalação; sem entidade de schema).

## Arquivos

- `apps/rt/src/run/doctor.rs` — NOVO. O módulo do subcomando: as quatro checagens (wiring de hooks, resíduo, drift de instalação, saúde de estado), o renderizador do relatório OK/WARN/FAIL e os testes unitários.
- `apps/rt/src/run/mod.rs` — EDIT. `mod doctor;`; a variante `RunCmd::Doctor { residue: bool }` com doc-comment; o braço correspondente no `dispatch()`.
- `apps/cli/templates/commands/mustard/maint/SKILL.md` — EDIT. Substituir a ação `audit` pela ação `doctor`: o `doctor` passa a ser o comando de saúde único de `/maint`, absorvendo os dois checks que o `audit` fazia (`skills orphans`, `diagnose-otel`) num só relatório.

## Tarefas

### Impl Agent (Wave 1) — subcomando `doctor` + wrapper `/maint`

- [x] Confirmar o padrão de subcomando `run` via skill `rt-run-subcommand-pattern` (módulo em `run/`, variante no enum `RunCmd`, braço no `dispatch()`); reusar `crate::report` para a saída, como em `run/verify_pipeline.rs`.
- [x] Check **wiring** — parsear `.claude/settings.json` e, para cada `mustard-rt on <event>` / `run <cmd>` referenciado, confirmar que o evento/subcomando existe no binário; reportar OK por entrada resolvida e FAIL por entrada quebrada.
- [x] Check **resíduo** (atrás da flag `--residue`) — varrer `settings.json`, SKILL.md e refs por referências a arquivos/scripts/comandos que não resolvem para nada existente (ex.: a pasta `scripts` listada como CORE_FOLDER sem scripts resolvíveis; nomes de `.js` mortos); reportar WARN.
- [x] Check **drift de instalação** — comparar por hash apenas as pastas que o `mustard-cli` regenera no `update` (derivar da constante `CORE_FOLDERS`, não hardcodear) entre o `.claude/` instalado e a fonte `templates/`; arquivos bespoke (`CLAUDE.md` raiz, `prompts/`, `context/`) **não são cópias** e ficam fora da comparação. Quando `templates/` não é alcançável a partir do cwd (projeto consumidor), degradar para `skip` com nota — manifesto de hashes é não-objetivo.
- [x] Check **saúde de estado** — `.claude/.pipeline-states/` órfãos (sem spec ativa correspondente), estados `closed-followup` vencidos, `entity-registry.json` ausente; reportar WARN por achado.
- [x] Renderizador — relatório de texto compacto OK/WARN/FAIL por categoria; `run(residue: bool)` é a entrada; exit 1 se houver qualquer FAIL, 0 caso contrário; read-only e fail-open em todo erro de IO (um check que falha vira WARN, nunca aborta).
- [x] Registrar em `run/mod.rs` — `mod doctor;`, a variante `RunCmd::Doctor { residue: bool }` (flag `--residue`) com doc-comment, e o braço `RunCmd::Doctor { residue } => doctor::run(residue)`.
- [x] Testes unitários com `tempdir` — wiring quebrado → FAIL; instalação limpa → OK; resíduo plantado → detectado; `.pipeline-states/` órfão → WARN.
- [x] Atualizar `apps/cli/templates/commands/mustard/maint/SKILL.md` — trocar a ação `audit` por `doctor` na tabela de ações; a seção `## doctor` roda `mustard-rt run doctor [--residue]` somada a `mustard-rt run skills orphans` e `mustard-rt run diagnose-otel`, apresentando um relatório consolidado.
- [x] `cargo build -p mustard-rt` e `cargo test -p mustard-rt` verdes.

## Dependências

Nenhuma externa. O padrão de subcomando `run` e o helper `crate::report` já existem — `run/verify_pipeline.rs` é o modelo direto (módulo diagnóstico, read-only, fail-open, discovery + relatório). O binário `mustard-rt` já é compilado.

## Limites

- `apps/rt/src/run/` — o novo `doctor.rs` e a edição em `mod.rs`.
- `apps/cli/templates/commands/mustard/maint/SKILL.md`.
- **Fora dos limites:** novos hooks de enforcement; a lógica da CLI instaladora (`apps/cli/src/`); o esquema do `mustard.db`; o protocolo MCP; consertar o doc-drift que o `doctor` *detecta* (corrigir os textos stale como "28 scripts" em `templates/CLAUDE.md` é uma limpeza separada — o `doctor` apenas reporta).

## Preocupações

Registradas no REVIEW (Wave 1 — verdict APPROVED, todas não-críticas, baixo risco):

- **`KNOWN_RUN_SUBCOMMANDS` sincronizado à mão** — o check de wiring valida contra uma lista estática de subcomandos `run`; um subcomando novo que não for adicionado a essa lista gera um FAIL falso no `doctor`. É o padrão aceito do crate (sem reflexão em Rust); um comentário apontando a obrigação de manutenção mitiga. Revisitar na fase 2 do epic.
- **Teste `drift_warns_on_hash_mismatch` frouxo** — aceita `WARN || SKIP`; dado o setup com `templates/` como irmão direto do tempdir, deveria afirmar `WARN` diretamente.
- **Estado `closed-followup` vencido sem teste end-to-end** — o helper de timestamp é testado, mas não há um caso com um state file `closed-followup` de shape completo + timestamp antigo.
