# Wave 8.5 — mustard-install-grammars (papel: cli, opcional)

### Stage: Analyze
### Outcome: Active
### Scope: light
### Lang: pt-BR
### Parent: 2026-05-27-mustard-v4-foundation
### Checkpoint: 2026-05-27T17:56:09.926Z

## Contexto

Wave **opcional** introduzida pelo redesign v2 da Spec A. O módulo `mustard_core::ast` (W1.5) é agnóstico via `tree_sitter::Loader` — descobre grammars instaladas pelo usuário em `~/.config/tree-sitter/config.json`, sem nenhum grammar linkado no binário Mustard. Quando o usuário roda o gate de regressão num projeto cuja linguagem detectada ainda não tem grammar instalada, o gate cai para fallback `vocabulary::scan` (W1) — funciona, mas perde precisão AST. Este CLI helper opcional fecha o ciclo de UX: lê o stack detectado pelo `detect_libs` do projeto-alvo e imprime, para cada linguagem, **o repositório canônico do grammar tree-sitter e o comando shell exato** a rodar para clonar+compilar localmente. **Mustard não baixa, não clona, não compila** — apenas sugere. Isso preserva a primícia [[feedback_mustard_agnostic]] (Mustard nunca embute conhecimento de linguagem) ao mesmo tempo que reduz a fricção para o usuário sair do modo fallback.

**Caveat de naming:** este diretório usa `wave-8_5-cli/` (underscore) pelos mesmos motivos descritos em `wave-1_5-core/spec.md` — `parse_wave_dir_number` em `apps/rt/src/run/dependency_precheck.rs:855` para na primeira não-dígito, então `wave-8.5-cli/` e `wave-8_5-cli/` resolvem para wave=8 e colidiriam com `wave-8-mixed/`. Tooling downstream que precisar distinguir W8 de W8.5 deve consumir o nome literal do diretório.

## Arquivos tocados

- `apps/cli/src/commands/install_grammars.rs` (NOVO) — `install_grammars::run` (subcomando)
- `apps/cli/src/main.rs` (ESTENDIDO) — wiring do subcomando `mustard install-grammars`
- `apps/cli/src/commands/mod.rs` (ESTENDIDO) — re-export do módulo

## Funções tocadas

### Em `apps/cli/src/commands/` (NOVO)
- `install_grammars::run`

## Acceptance Criteria

Subset relevante desta wave:
- AC-A-18: `mustard install-grammars` lê o stack detectado em `detect_libs` e guia o usuário a clonar+compilar grammars das linguagens detectadas. Mustard **não** baixa nem compila grammars — apenas sugere os repos canônicos e o comando shell

## Tarefas

- [ ] T8.5.1: Criar `apps/cli/src/commands/install_grammars.rs` com `pub fn run(args: InstallGrammarsArgs) -> Result<()>` que recebe `project_root` (default: cwd), chama `mustard_core::context7::detect_libs(project_root)` (W1.5 vizinha já oferece, ou pode ser reusado de runtime existente) e mapeia cada `LibSpec` para `language_id` via método existente
- [ ] T8.5.2: Implementar uma tabela estática de **sugestões** (não enforçável) mapeando `language_id` → `{ repo_url, install_cmd }` para os ids canônicos mais comuns (`rust`, `typescript`, `javascript`, `python`, `go`, `c_sharp`, `java`, `ruby`, `c`, `cpp`). Esta tabela vive **apenas no CLI helper** e tem caráter de bookmark — Mustard não usa esses ids no caminho do gate. Linguagem detectada sem entrada na tabela imprime `"<lang_id>: grammar não catalogado — buscar em https://tree-sitter.github.io/tree-sitter/#parsers"` (fallback explícito, fail-open)
- [ ] T8.5.3: Para cada linguagem detectada com entrada na tabela, imprimir: nome humano, `repo_url`, e o `install_cmd` shell-ready (típico: `tree-sitter init && cd <subdir> && tree-sitter generate`). Output formatado como blocos de markdown para que o usuário possa copiar+colar em terminais ou docs
- [ ] T8.5.4: Detectar grammars **já instaladas** consultando `tree_sitter_loader::Loader::find_all_languages` no config default (`~/.config/tree-sitter/config.json`) e marcar com `✓ já instalada` ao lado do nome — evita pedir ao usuário que reinstale o que já existe
- [ ] T8.5.5: Estender `apps/cli/src/main.rs` registrando o subcomando `install-grammars` e `apps/cli/src/commands/mod.rs` re-exportando o módulo
- [ ] T8.5.6: Adicionar teste `install_grammars::test_known_languages_table_and_fallback` validando: linguagem catalogada (`rust`) produz output com `repo_url` correto + comando; linguagem desconhecida (`brainfuck`) produz mensagem de fallback sem erro; linguagem catalogada + já instalada (mock do Loader retornando `Some(Language)`) produz marcador `✓`

## Não-Objetivos

- **Baixar, clonar ou compilar grammars** — proibido sempre. Mustard apenas sugere comandos shell. ([[feedback_mustard_agnostic]])
- **Embutir tabela de grammars no caminho do gate** — a tabela vive apenas neste comando CLI como UX bookmark; o gate (W4) e o `GrammarLoader` (W1.5) **não consultam** esta tabela.
- **Auto-instalação no `mustard init`** — fora de escopo; user roda manualmente quando quiser sair do modo fallback do gate.

## Dependências (waves anteriores)

- W1.5 (`tree_sitter_loader::Loader` reusado para detectar grammars já instaladas)

<!-- wikilinks-footer-start -->
- [feedback_mustard_agnostic](?) ⚠ não resolvido
<!-- wikilinks-footer-end -->