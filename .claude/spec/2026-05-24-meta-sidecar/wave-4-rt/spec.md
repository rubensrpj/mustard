# Onda 4 — Cleanup: remover headers do `.md` e simplificar parser

## Resumo

Esta onda é a limpeza final. Agora que toda spec tem `meta.json` (Onda 2) e todos os escritores escrevem nele (Onda 3), os headers `### X:` no `.md` ficaram redundantes. Esta onda remove os headers, simplifica o parser que tinha a tabela de variantes pra reconhecê-los, e decide o destino do `migrate_spec_headers.rs`.

## O que muda neste passo

1. **Remoção dos headers no `.md` de todas as specs.** Um subcomando `mustard-rt run cleanup-spec-md` percorre `.claude/spec/**` e remove as linhas `### Stage:`, `### Outcome:`, `### Phase:`, `### Scope:`, `### Lang:`, `### Checkpoint:`, `### Parent:` de todo `*.md`. Mantém qualquer outra linha intacta (incluindo o `# Título` do topo e qualquer `## Seção` abaixo). Idempotente.

2. **Atualização dos escritores: param de emitir os headers no `.md`.** `wave-scaffold`, `emit-pipeline`, `tactical-fix` deixam de escrever as linhas `### X:` no `.md` recém-criado. O `meta.json` é a única fonte.

3. **Atualização do template `feature/SKILL.md` e similares.** As instruções pro orquestrador escrever specs deixam de mencionar headers no corpo do `.md`. Mencionam `meta.json` quando for relevante.

4. **Simplificação do `spec_sections.rs`.** A tabela `variants(key)` perde as entradas relacionadas a stage/outcome/phase/scope (que nunca foram seções de conteúdo, mas o parser tinha cobertura conjunta). Continua reconhecendo só as seções de conteúdo (`Context`/`Contexto`, `Files`/`Arquivos`, etc.) — essas o conteúdo narrativo do `.md` ainda precisa.

5. **Decisão sobre `migrate_spec_headers.rs`.** Esse módulo existia pra reescrever headers no `.md`. Com headers fora do `.md`, ele perde função:
   - Se nenhum lugar mais chama ele, **deleta**.
   - Se algum lugar chama (pra outro fim correlato), simplifica ou redireciona pra `write_meta`.

6. **Remoção do comando `dashboard_specs` (versão antiga) e do parser correspondente, se redundante.** Dashboard agora consome `read_spec_meta` (Onda 3). O comando antigo que parseava `.md` pode ser removido se nada mais chamar.

## Arquivos

- `apps/rt/src/run/cleanup_spec_md.rs` (novo) — subcomando de limpeza.
- `apps/rt/src/run/mod.rs` — registra o subcomando.
- `apps/rt/src/run/wave_scaffold.rs` — não emite mais headers no `.md`.
- `apps/rt/src/run/emit_pipeline.rs` — não muta mais headers no `.md` (só JSON).
- `apps/rt/src/run/tactical_fix.rs` (ou equivalente) — idem.
- `apps/rt/src/run/spec_sections.rs` — simplificado.
- `apps/rt/src/run/migrate_spec_headers.rs` — decisão (deletar ou redirecionar).
- `apps/cli/templates/commands/mustard/feature/SKILL.md` — instruções atualizadas.
- `apps/cli/templates/commands/mustard/spec/SKILL.md` — idem.
- `apps/cli/templates/commands/mustard/tactical-fix/SKILL.md` — idem.
- `apps/cli/templates/refs/feature/spec-language.md` — tabela de variantes de heading agora só vale pras seções de conteúdo, não pros campos `### X:`.
- `apps/dashboard/src-tauri/src/commands/specs.rs` — remoção do `dashboard_specs` antigo se redundante.
- `.claude/spec/**/*.md` — limpos pelo novo subcomando.

## Tarefas

### rt Agent (Wave 4)

- [ ] Criar `apps/rt/src/run/cleanup_spec_md.rs` com:
  - Função `run(opts: CleanupOpts) -> Result<CleanupReport>`.
  - Percorre `.claude/spec/**`, lê cada `.md`, remove linhas `### {Stage|Outcome|Phase|Scope|Lang|Checkpoint|Parent}:` (com regex simples ou string scan), regrava atomicamente.
  - Flag `--dry-run`.
- [ ] Registrar o subcomando no dispatch.
- [ ] Em `wave_scaffold.rs`, parar de emitir os headers `### X:` no `.md` recém-criado (só emitir o `meta.json`).
- [ ] Em `emit_pipeline.rs`, remover qualquer mutação de headers no `.md` (só atualizar `meta.json` e SQLite).
- [ ] Em `tactical_fix`, idem.
- [ ] Em `spec_sections.rs`, remover entradas de `variants(key)` referentes a stage/outcome/phase/scope/lang/checkpoint/parent (se existirem). Manter só `context`, `summary`, `boundaries`, `files`, `rootCause`, `tasks`, `acceptanceCriteria`, `nonGoals`, `concerns`, `decisions`, `dependencies`, `entityInfo`, `symptom`.
- [ ] Decisão sobre `migrate_spec_headers.rs`: rodar `grep -r migrate_spec_headers apps/` — se nenhum call site, deletar; senão simplificar.
- [ ] Atualizar templates de SKILL (`feature`, `spec`, `tactical-fix`) e `spec-language.md` removendo qualquer referência a `### Stage:`/etc. como instrução.
- [ ] Rodar `mustard-rt run cleanup-spec-md` em `.claude/spec/` deste repo.
- [ ] `cargo build --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`.
- [ ] AC-5, AC-6 e AC-8 do wave-plan passam.

### ui Agent (Wave 4 — opcional, se aplicável)

- [ ] Se `apps/dashboard/src-tauri/src/commands/specs.rs` ainda expõe `dashboard_specs` que parseia `.md`, e nenhum consumer do frontend chama mais essa função, remover.

## Dependências

Depende das Ondas 2 e 3. Não dá pra remover os headers do `.md` antes que todas as specs tenham `meta.json` (Onda 2) e que todos os escritores tenham migrado pra escrever `meta.json` (Onda 3).

## Limites

Esta onda **só** remove redundância. Não introduz capacidade nova. Tudo o que ela apaga já existe em outro lugar (no `meta.json`).

## Preocupações

- **Specs que alguém já tem clonadas em outro repo.** Quem tiver fork ou clone do Mustard precisa rodar `cleanup-spec-md` localmente, ou as ferramentas vão tratar os headers como conteúdo (não vai quebrar nada — só vai ficar ruído no `.md`).
- **Documentação em CONTEXT.md, ADRs, etc.** Pode ter referência a `### Stage:`. Sweep e atualizar onde fizer sentido.
