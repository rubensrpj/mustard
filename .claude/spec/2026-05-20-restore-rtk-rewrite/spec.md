# Restaurar `rtk-rewrite` no `bash_guard`

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full
### Checkpoint: 2026-05-20T01:00:00Z
### Lang: pt

## PRD

## Contexto

Toda chamada `Bash` que o Claude Code dispara passa pelo `PreToolUse` do `mustard-rt`. Por contrato, o módulo `bash_guard` deve reescrever comandos crus (ex.: `cat foo`, `grep -n x src/`) em equivalentes RTK (`rtk read foo`, `rtk grep -n x src/`), emitindo um `Verdict::Rewrite` com `updatedInput`. A regra global do usuário (`~/.claude/CLAUDE.md` "Golden Rule") é "Always prefix commands with `rtk`" — e a documentação do projeto (`apps/cli/templates/CLAUDE.md:127`) afirma textualmente que o `bash_guard` faz isso de forma transparente, gerando 60-90% de economia de tokens.

Desde o port JS→Rust concluído em 2026-05-19 (spec `eliminate-bun`, wave b6) a função `rtk_rewrite` em `apps/rt/src/hooks/bash_guard.rs:477` é um stub `Wave-1`: ignora o input e retorna `None`. Nenhum comando é reescrito; nenhum evento `rtk-rewrite` é gravado no event store; o segmento RTK do statusline (`run/statusline.rs`) consulta `rtk gain` mas só vê reescritas anteriores ao port. O último registro em `.claude/.metrics/rtk-rewrite.jsonl` é `2026-05-19T17:50:10` — pré-port; tudo depois disso virou perda silenciosa.

O impacto é diário e cumulativo: cada sessão paga 60-90% a mais de tokens em saídas de `git`, `cargo`, `gh`, `grep`, `ls`, etc., sem qualquer sinal visível ao usuário. O contrato `rtk rewrite <cmd>` do binário RTK está vivo (exit 0 com stdout reescrito, exit 1 silencioso quando não há equivalente) — a quebra é puramente no consumidor.

## Usuários/Stakeholders

Todo usuário do Mustard com `rtk` instalado — em particular o autor (Rubens), que opera Claude Code em janelas longas onde o custo de tokens de Bash domina. Stakeholder secundário: o próprio sistema de telemetria (`/mustard:stats`, `/mustard:metrics`, statusline) que perdeu o canal de medição da economia real.

## Métrica de sucesso

Após uma sessão de uso normal (≥10 comandos Bash crus dispostos a reescrita), `rtk gain --all --format json` reporta `commands > 0` e `saved > 0`, e `.claude/.metrics/rtk-rewrite.jsonl` (ou equivalente no SQLite event store) recebe ≥1 nova entrada com `event: "rtk-rewrite"` e `command_head` truncado.

## Não-Objetivos

- Não reescrever a infraestrutura RTK em si — `rtk rewrite <cmd>` é o contrato existente e basta.
- Não computar `tokens_saved` no momento da reescrita (impossível sem rodar o comando). `metrics.rs:112,133,324` já ignora `tokens_saved` para `rtk-rewrite`; emitimos `0` consistente.
- Não introduzir nova abstração de injeção de dependência além do estritamente necessário para tornar o subprocesso testável.
- Não tocar nos outros 4 gates do `bash_guard` (`bash-safety`, `bash-native-redirect`, `review-gate`, `pr-detect`).
- Não migrar o formato do evento `rtk-rewrite` — paridade com o JSON histórico em `.claude/.metrics/rtk-rewrite.jsonl`.

## Critérios de Aceitação

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: Build do crate compila sem warnings novos — Command: `rtk cargo build -p mustard-rt`
- [x] AC-2: Suite de testes unitários do `bash_guard` passa, incluindo os novos casos comportamentais de `rtk-rewrite` — Command: `rtk cargo test -p mustard-rt rtk_rewrite`
- [x] AC-3: Comando cru sem prefixo rtk produz updatedInput quando RTK tem equivalente — Command: `rtk cargo test -p mustard-rt rtk_rewrite_e2e_rewrites_unprefixed_command`
- [x] AC-4: Comando já com prefixo rtk faz pass-through silencioso, stdout vazio — Command: `rtk cargo test -p mustard-rt rtk_rewrite_e2e_passes_through_rtk_prefixed_command`
- [x] AC-5: Reescrita bem-sucedida grava evento `rtk-rewrite` no event store — verificado por teste de integração que dispara `BashGuard::evaluate()` com payload `cwd=<temp>` + closure-rewriter que retorna `Some("rtk git status")`, então abre `<temp>/.claude/.harness/mustard.db` e asserta `SELECT count(*) FROM events WHERE event='rtk-rewrite' AND ts > <T0> >= 1` — Command: `rtk cargo test -p mustard-rt rtk_rewrite_emission`
- [x] AC-6: Doc-comments do stub Wave-1 removidos — Command: `node -e "const fs=require(\"fs\");const t=fs.readFileSync(\"apps/rt/src/hooks/bash_guard.rs\",\"utf8\");if(t.includes(\"Wave 1 does not spawn\")||t.includes(\"let _ = cmd;\")&&t.includes(\"// No \\\"rtk\\\" subprocess in Wave 1\"))process.exit(1)"`
- [x] AC-7: Comportamento fail-open quando `rtk` ausente do PATH — verificado por teste unitário que injeta closure simulando ENOENT — Command: `rtk cargo test -p mustard-rt rtk_rewrite_fail_open`

## Plano

## Informações da Entidade

Não há entidade nova. O alvo é o módulo `BashGuard` (`apps/rt/src/hooks/bash_guard.rs`), `Check` do `PreToolUse(Bash)`. Conforme `entity-registry.json`, este já está catalogado como módulo do crate `mustard-rt`. O contrato externo é `rtk rewrite <cmd>`:

| Campo | Valor |
|---|---|
| Binário | `rtk` (resolvido via PATH) |
| Subcomando | `rewrite` |
| Stdout (exit 0) | Comando reescrito, uma linha, terminada em `\n` |
| Exit 1 | Sem stdout — sinal de "sem equivalente" |
| Stderr | Banner `[rtk] /!\ No hook installed — run \`rtk init -g\`...` (descartar) |
| Timeout sugerido | 2s (subprocess local, sem rede) |

## Arquivos

| Arquivo | Mudança |
|---|---|
| `apps/rt/src/hooks/bash_guard.rs` | Implementar `rtk_rewrite()` real + helper `run_rtk_rewrite_subprocess()` + emissão SQLite; remover comentários "Wave 1" |
| `apps/rt/src/hooks/bash_guard.rs` `#[cfg(test)] mod rtk_rewrite_tests` | NOVO — 8 testes comportamentais inline (crate `mustard-rt` é bin-only, integration tests externos não acessam funções `pub(crate)`; módulo inline preserva acesso direto a `rtk_rewrite_with` e `RTK_REWRITE_TEST_OVERRIDE`) |
| `apps/rt/tests/rtk_rewrite_emission.rs` (create) | NOVO — integration test que dirige o binário via subprocess + temp DB; valida AC-5 |
| `packages/core/src/model/contract.rs` | **Wave 4 (descoberto durante validação):** `Outcome::fold` precisa preservar verdicts decisivos quando module subsequente retorna `Verdict::Allow` (no-opinion). Sem esse fix, `tool_use_counter`/`main_context_counter` (modules `ToolMatch::Any`) sobrescrevem o `Verdict::Rewrite` do bash_guard com Allow → rewrite engolido pelo dispatcher |
| `apps/rt/.claude/commands/modules.md` | Atualizar descrição se mencionar stub |
| `apps/cli/templates/CLAUDE.md` | Verificar afirmação L127 — sem edição se já correta |

## Tarefas

### Implementation Agent (Wave 1)

- [ ] Adicionar constante `RTK_REWRITE_TIMEOUT: Duration = Duration::from_secs(2)` próxima ao bloco `// rtk-rewrite`
- [ ] Criar `fn run_rtk_rewrite_subprocess(cmd: &str) -> Option<String>` — espelha o pattern de `run_build` (linhas 569-643): `Command::new("rtk").args(["rewrite", cmd])`, stdin=null/stdout=pipe/stderr=null, spawn em thread + `recv_timeout`. Retorna `None` em: spawn-failure, exit≠0, timeout, stdout vazio
- [ ] Refatorar `rtk_rewrite()` em duas camadas: `rtk_rewrite_with<F: FnOnce(&str)->Option<String>>(cmd, rewriter)` (pura, testável) e wrapper `rtk_rewrite(cmd)` que passa `run_rtk_rewrite_subprocess` como rewriter
- [ ] Em `rtk_rewrite_with`: short-circuit se `is_rtk_wrapped(cmd)` (helper já existente na linha ~436), invocar rewriter, retornar `None` se vazio ou idêntico (após `trim`), senão `Verdict::Rewrite { tool_input: json!({"command": rewritten}) }`
- [ ] Emitir evento `rtk-rewrite` no `SqliteEventStore` quando a reescrita for decisiva — mesmo pattern de `bash_guard.rs:716-719`. Schema do payload: `{event: "rtk-rewrite", tokens_affected: <bytes_do_cmd>, tokens_saved: 0, note: "rewritten via rtk", command_head: <primeiros 60 chars>, rewritten_head: <primeiros 60 chars>}` — paridade com `.claude/.metrics/rtk-rewrite.jsonl` histórico
- [ ] Remover todos os doc-comments mencionando "Wave 1 does not spawn" (linhas 472-482) e "JS fail-open branch" — substituir por descrição da implementação corrente
- [ ] Adicionar `#[must_use]` no `run_rtk_rewrite_subprocess` e doc-comment explicando os 4 caminhos fail-open

### Test Agent (Wave 2)

- [ ] Criar `apps/rt/tests/rtk_rewrite_behavior.rs` (integração no crate root, lê `pub` da crate)
- [ ] Caso `rewrite_pass_through_when_rtk_prefixed`: input `"rtk grep x"` + rewriter dummy → `None`
- [ ] Caso `rewrite_emits_updated_input_when_rewriter_returns_change`: input `"grep -n x src/"` + closure `|c| Some(format!("rtk {}", c))` → `Verdict::Rewrite` com `tool_input.command == "rtk grep -n x src/"`
- [ ] Caso `rewrite_fail_open_when_rewriter_returns_none`: closure `|_| None` → `None` (cobre rtk-ausente e exit-1)
- [ ] Caso `rewrite_fail_open_when_rewriter_returns_identical`: input `"grep -n x"` + closure que retorna o mesmo `"grep -n x"` → `None`
- [ ] Caso `rewrite_fail_open_when_rewriter_returns_empty`: closure `|_| Some(String::new())` → `None`
- [ ] Caso `rewrite_strips_trailing_whitespace`: closure que retorna `"rtk grep x\n"` → `tool_input.command == "rtk grep x"` (sem `\n`)
- [ ] Caso unitário separado para `run_rtk_rewrite_subprocess` chamado `rtk_rewrite_fail_open` — invoca com `MUSTARD_RTK_BIN=__not_a_real_binary__` env var override (a função lê a env var, default `"rtk"`) — atende AC-7 sem depender de PATH

### Validation Agent (Wave 3)

- [ ] Rodar `rtk cargo build -p mustard-rt` — exit 0
- [ ] Rodar `rtk cargo test -p mustard-rt` — todos passam, incluindo o novo arquivo
- [ ] Rodar `rtk cargo clippy -p mustard-rt --no-deps` — sem warning novo
- [ ] Reinstalar binário com `rtk cargo install --path apps/rt --force` (memória `project_mustard_rt_stale_binary` — sem isso o PATH segue stale)
- [ ] Smoke-test manual: rodar AC-3 e AC-4 manualmente, capturar JSON de saída, anexar como "Evidence" em `## Concerns` se algo desviar
- [ ] Confirmar que `apps/cli/templates/CLAUDE.md:127` ainda descreve corretamente o comportamento — se sim, nenhuma edição; se mencionar paridade quebrada, ajustar

## Dependências

- Crate `mustard-core` já expõe `EventSink`, `SqliteEventStore::for_project`, `HarnessEvent`, `Actor`, `ActorKind`, `SCHEMA_VERSION` — todos já importados em `bash_guard.rs:28-31`
- Binário `rtk` precisa estar instalado para validação end-to-end; testes unitários NÃO dependem do binário real
- Sem dependência nova no `Cargo.toml`

## Limites

Edições restritas a:
- `apps/rt/src/hooks/bash_guard.rs` (modificar apenas a região "rtk-rewrite — rewrite a command through RTK" entre as linhas 461-482, mais o ponto de emissão de evento)
- `apps/rt/tests/rtk_rewrite_behavior.rs` (novo)
- `packages/core/src/model/contract.rs` (apenas `Outcome::fold` — single function — preservar verdicts decisivos contra subsequente `Verdict::Allow`)
- `apps/rt/.claude/commands/modules.md` (apenas se necessário)
- `apps/cli/templates/CLAUDE.md` (apenas se necessário)

Fora dos limites: qualquer outro módulo de `hooks/`, qualquer mudança em `protocol.rs`, qualquer mudança no `Verdict` enum, qualquer mudança no schema do event store. Edições fora destes paths serão marcadas como `[BOUNDARY WARNING]`.
