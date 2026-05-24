# Onda 1 — Schema + leitor com fallback

## Resumo

Esta onda monta o alicerce. Cria o schema do `meta.json`, escreve um leitor confiável em `mustard-core`, e adapta o `pipeline_state_ingest` pra preferir o JSON quando ele existe — caindo pro parser antigo se não existir. Nenhuma spec é migrada ainda (isso é Onda 2), nenhum escritor passa a gerar o JSON (Onda 3). Aqui o objetivo é só: a infraestrutura de leitura está pronta e compatível com o estado atual.

## O que muda neste passo

1. **Novo módulo `packages/core/src/meta.rs`.** Define a struct `SpecMeta` (campos do schema descrito no wave-plan), com `serde` pra ler e escrever JSON. Função pública `read_meta(spec_dir: &Path) -> Option<SpecMeta>` que devolve `None` se o arquivo não existe.

2. **`pipeline_state_ingest` lê o JSON primeiro.** O fluxo atual varre o `.md` em busca de `### Stage:`, `### Outcome:`, etc. Refatorado: tenta `read_meta(...)` primeiro. Se devolveu `Some`, usa o JSON. Se devolveu `None`, cai pro caminho antigo (varredura de linha).

3. **Função utilitária `write_meta(spec_dir, &SpecMeta) -> Result<()>`.** Escreve atomicamente em `.tmp` e renomeia. Usada na Onda 2 (migração) e Onda 3 (escritores). Já entra disponível agora pra evitar trabalho repetido nas próximas ondas.

4. **Compat strict.** A função de leitura **não falha** se o JSON existe mas está malformado — devolve `None` e loga warning. Assim uma corrupção em uma spec não bloqueia o pipeline; só desativa o atalho.

## Arquivos

- `packages/core/src/meta.rs` (novo) — schema + read/write.
- `packages/core/src/lib.rs` — registra o módulo.
- `apps/rt/src/run/pipeline_state_ingest.rs` — leitura via `meta.json` com fallback.

## Tarefas

### rt Agent (Wave 1)

- [ ] Criar `packages/core/src/meta.rs` com:
  - Struct `SpecMeta { stage, outcome, phase, scope, lang, checkpoint, parent, is_wave_plan, total_waves }` (campos opcionais marcados com `Option`).
  - Enums tipados pra `stage`/`outcome`/`phase`/`scope` (com `#[serde(rename_all = ...)]` se precisar).
  - Função pública `read_meta(spec_dir: &Path) -> Option<SpecMeta>` — fail-open: arquivo ausente devolve `None`, JSON malformado devolve `None` + warning.
  - Função pública `write_meta(spec_dir: &Path, meta: &SpecMeta) -> Result<()>` — escreve `.tmp` + renomeia.
- [ ] Registrar `mod meta;` em `packages/core/src/lib.rs`.
- [ ] Em `apps/rt/src/run/pipeline_state_ingest.rs`, ler `meta.json` antes da varredura de headers. Se presente, popular a estrutura interna a partir dele e pular a varredura.
- [ ] Manter o caminho atual (varredura) intacto pra quando `meta.json` não existe.
- [ ] `cargo build --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo test -p mustard-core`, `cargo test -p mustard-rt`.
- [ ] AC do wave-plan que essa onda toca: nenhum direto ainda — ACs aparecem nas próximas ondas. Aqui valida-se via build + test.

## Dependências

Nenhuma. É a primeira onda.

## Limites

Esta onda **só** mexe nos arquivos listados. Em particular:

- Não cria nenhum `meta.json` em spec existente (isso é Onda 2).
- Não muda os escritores (Onda 3).
- Não simplifica o `spec_sections.rs` nem remove headers do `.md` (Onda 4).
- Não toca no dashboard.
