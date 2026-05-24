# Onda 2 — Migração one-shot

## Resumo

Esta onda percorre todas as specs existentes em `.claude/spec/**` e cria um `meta.json` ao lado de cada `spec.md`, copiando os valores que hoje vivem nos headers `### X:`. Headers continuam no `.md` (modo espelho) — a limpeza final é Onda 4. Aqui o objetivo é: nenhuma spec fica sem `meta.json`.

## O que muda neste passo

1. **Novo subcomando `mustard-rt run migrate-to-meta`.** Em `apps/rt/src/run/migrate_to_meta.rs`. Percorre `.claude/spec/**` (root + cada wave/review/qa subdir). Pra cada `.md` que tem headers `### Stage:`, etc., gera o `meta.json` correspondente no mesmo diretório.

2. **Idempotente.** Se o `meta.json` já existe, o comando NÃO sobrescreve. Roda duas vezes seguidas = nada acontece na segunda.

3. **Tolerante a specs antigas.** Algumas specs no histórico podem estar incompletas: faltar `### Phase:`, ter `### Lang:` ausente, etc. O migrador aplica defaults razoáveis: se faltar `phase`, deriva de `stage` (Plan→PLAN, Execute→EXECUTE, etc.); se faltar `lang`, assume `pt`; se faltar `scope`, assume `full`.

4. **Saída JSON byte-estável.** Importante porque o pipeline parsea stdout do `mustard-rt run`. O output do comando reporta `{ migrated: N, skipped: M, errors: [...] }` em formato pretty-print de 2 espaços (convenção do repo).

5. **Flag `--dry-run`.** Útil pra inspecionar o que vai mudar sem gravar nada. Default: dry-run desligado.

## Arquivos

- `apps/rt/src/run/migrate_to_meta.rs` (novo) — subcomando + lógica de extração.
- `apps/rt/src/run/mod.rs` — registra o novo subcomando no dispatch.
- `apps/rt/src/main.rs` ou `cli.rs` — argumento de linha de comando.

## Tarefas

### rt Agent (Wave 2)

- [ ] Criar `apps/rt/src/run/migrate_to_meta.rs` com:
  - Função `run(opts: MigrateOpts) -> Result<MigrateReport>`.
  - Iteração sobre `.claude/spec/**`: identifica diretórios de spec, lê `spec.md` (ou `wave-plan.md`), extrai headers via parser atual (`spec_sections::is_heading` ainda existe), monta `SpecMeta`, escreve via `mustard_core::meta::write_meta`.
  - Defaults pra campos faltantes (regras descritas em "O que muda neste passo").
  - Skip se `meta.json` já existe.
  - Flag `--dry-run` reporta sem gravar.
- [ ] Registrar o subcomando em `apps/rt/src/run/mod.rs`.
- [ ] Adicionar flag de linha de comando (`run migrate-to-meta [--dry-run]`).
- [ ] Rodar o comando em `.claude/spec/` deste próprio repo (sem `--dry-run`) — todas as specs ganham seu `meta.json`.
- [ ] `cargo build --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo test -p mustard-rt`.
- [ ] AC-1 e AC-7 do wave-plan passam.

## Dependências

Depende da Onda 1 (`write_meta` em `mustard-core` precisa existir).

## Limites

Esta onda **só** cria `meta.json` em specs existentes e adiciona o subcomando de migração. Não muda nenhum escritor (Onda 3), não remove os headers do `.md` (Onda 4).

## Preocupações

- **Specs incompletas no histórico.** Algumas specs antigas podem ter mistura PT/EN ou faltar campos. O migrador precisa ser robusto: campo ausente → default; valor desconhecido → loga warning e usa o default. Nenhuma spec deve falhar a migração inteira.
- **Specs já completed/cancelled.** Não importa o `outcome`. O migrador captura o estado atual, não tenta "ativar" nada.
