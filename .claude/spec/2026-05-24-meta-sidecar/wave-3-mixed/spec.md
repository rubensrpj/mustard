# Onda 3 — Escritores e dashboard

## Resumo

Esta onda atualiza os caminhos que criam specs novas pra escreverem `meta.json` desde o nascimento, e atualiza o dashboard pra ler o JSON direto. No fim dela, toda spec nova já nasce com `meta.json` e nenhum consumidor depende mais do parser de headers `### X:`.

## O que muda neste passo

1. **`wave-scaffold` escreve `meta.json`.** Hoje o `wave-scaffold` cria o `wave-plan.md` + os `wave-N-*/spec.md` com headers no topo. Passa a também criar um `meta.json` ao lado de cada um, refletindo o estado inicial (Stage=Plan, Outcome=Active, Phase=PLAN, etc.).

2. **`emit-pipeline` reflete no `meta.json`.** Quando emite `pipeline.scope`, `pipeline.status`, `pipeline.phase`, `pipeline.stage`, `pipeline.outcome`, o subcomando passa a também atualizar o `meta.json` da spec correspondente — em paralelo ao evento que vai pro SQLite. Eventos continuam sendo a verdade autoritativa pra estado vivo; `meta.json` espelha o último valor pra leitura rápida.

3. **`tactical-fix` cria sub-spec com `meta.json`.** O comando que gera sub-specs também escreve o JSON do mesmo modo.

4. **Comandos do CLI (`mustard:feature`, `mustard:spec`, `mustard:tactical-fix`).** As SKILLs desses comandos têm instruções de como criar spec — atualizadas pra mencionar `meta.json` como parte do scaffold (orientação pra o orquestrador).

5. **Dashboard ganha comando Tauri `read_spec_meta`.** Em `apps/dashboard/src-tauri/src/commands/specs.rs`, novo comando que recebe um path e devolve o conteúdo do `meta.json` como objeto tipado. Substitui o que hoje fazia parser do `.md`.

6. **Dashboard consome o novo comando.** Onde o frontend hoje pede `dashboard_specs` (que internamente parseia `.md`), passa a usar `read_spec_meta` direto. Mais simples, mais rápido.

## Arquivos

- `apps/rt/src/run/wave_scaffold.rs` — escrita de `meta.json` por spec criada.
- `apps/rt/src/run/emit_pipeline.rs` — espelhar mudanças em `meta.json`.
- `apps/rt/src/run/tactical_fix.rs` (se existir; senão a lógica está num módulo equivalente) — escrita de `meta.json`.
- `apps/cli/templates/commands/mustard/feature/SKILL.md` — instrução referente ao `meta.json`.
- `apps/cli/templates/commands/mustard/spec/SKILL.md` — mesma coisa.
- `apps/cli/templates/commands/mustard/tactical-fix/SKILL.md` — mesma coisa.
- `apps/dashboard/src-tauri/src/commands/specs.rs` — novo comando `read_spec_meta`.
- `apps/dashboard/src-tauri/src/lib.rs` — registra o handler.
- `apps/dashboard/src/lib/dashboard.ts` (ou equivalente) — fetcher Tauri tipado.
- `apps/dashboard/src/features/specs/**` — consome o novo fetcher.

## Tarefas

### rt Agent (parte 1 — Wave 3)

- [ ] Em `wave_scaffold.rs`, para cada `spec.md` criado, gerar também o `meta.json` correspondente via `mustard_core::meta::write_meta`.
- [ ] Em `emit_pipeline.rs`, após emitir o evento no SQLite, atualizar o campo correspondente no `meta.json` da spec (`pipeline.phase` → campo `phase`, etc.). Carregar `meta.json` existente, mutar o campo, regravar. Fail-open: se gravação falhar, loga warning mas o evento já foi pro SQLite.
- [ ] Em `tactical_fix` (ou onde sub-specs nascem), idem.
- [ ] Atualizar as 3 SKILLs (`feature`, `spec`, `tactical-fix`) com a instrução de que `meta.json` faz parte do scaffold automático.
- [ ] `cargo build --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo test -p mustard-rt`.

### ui Agent (parte 2 — Wave 3)

- [ ] Em `apps/dashboard/src-tauri/src/commands/specs.rs`, criar comando `read_spec_meta(spec_path: String) -> Result<SpecMeta>`. Reusar o tipo `SpecMeta` de `mustard-core` (já compilado pelo `src-tauri` que linka `mustard-core`).
- [ ] Registrar o handler em `apps/dashboard/src-tauri/src/lib.rs`.
- [ ] Em `apps/dashboard/src/lib/dashboard.ts`, adicionar a função `readSpecMeta(path)` que chama `invoke('read_spec_meta', { specPath: path })` e retorna tipado.
- [ ] Onde o frontend hoje recebe metadados via `dashboard_specs` parseando o `.md`, trocar pelo novo fetcher. Tipicamente em hooks tipo `useSpec(path)` ou em queries do TanStack.
- [ ] `pnpm --filter mustard-dashboard build` e lint passam.
- [ ] AC-3 e AC-4 do wave-plan passam.

## Dependências

Depende da Onda 1 (`SpecMeta` e `write_meta` existem). NÃO depende da Onda 2 — pode rodar em paralelo (uma toca em specs novas, a outra em existentes).

## Limites

Esta onda **só** mexe nos escritores e no dashboard. Não remove os headers `### X:` do `.md` (Onda 4) — eles continuam sendo escritos no scaffold durante essa transição (espelho).

## Preocupações

- **Sincronização entre evento SQLite e `meta.json`.** Pra cada mutação, escrever no SQLite primeiro, depois espelhar no `meta.json`. Se a escrita do JSON falhar, o evento SQLite já tá lá (fonte autoritativa) — `meta.json` fica stale por aquela rodada, mas a próxima emissão regrava. Fail-open por design.
- **Comando Tauri novo vs comandos antigos.** O `dashboard_specs` que parseia `.md` continua existindo nesta onda (não removemos). Frontend troca pra `read_spec_meta` gradualmente onde fizer sentido. Remoção total é cleanup da Onda 4.
