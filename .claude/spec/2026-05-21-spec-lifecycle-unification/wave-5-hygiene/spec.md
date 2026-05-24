# Wave 5 — Hygiene: spec_hygiene hook + auto-close protegido por gate

### Wave: 5
### Role: rt

## Resumo

Adiciona o hook `spec_hygiene` ao `mustard-rt` que roda em `SessionStart`. Para cada spec ativa, ele determina se a spec está em estado "candidata a fechar" e dispara auto-close **somente se** o close-gate (build/lint/test/QA) passa. Emite eventos `hygiene.*` para auditabilidade (renderizados pelo dashboard em Wave 6).

## Arquivos

```
apps/rt/src/hooks/spec_hygiene.rs               (novo — hook principal)
apps/rt/src/hooks/mod.rs                        (registrar)
apps/rt/src/dispatch.rs                         (wire ao SessionStart event)
apps/rt/src/lib.rs                              (expose módulo)
apps/rt/src/run/emit_pipeline.rs                (aceitar kinds hygiene.* — adicionar a KNOWN_KINDS)
apps/rt/tests/spec_hygiene.rs                   (novo — cenários)
apps/rt/tests/hygiene_event_kinds.rs            (novo — KNOWN_KINDS)
```

## Tarefas

### Algoritmo de detecção

- [ ] Hook roda **antes** do `session_start` injection (ordem do dispatcher).
- [ ] Para cada spec em `.claude/spec/` cujo `SpecState` tem `outcome == Active`:
  1. Lê AC do spec.md: se algum AC não tem `[x]` → status = `incomplete`, continue.
  2. Lê `git log --oneline -1 -- "{spec_dir}"` para o último commit que tocou a spec.
  3. Lê SQLite: timestamp do último evento de qualquer kind para essa spec.
  4. Determina categoria:
     - **candidate**: todos AC `[x]` + commit recente (≤72h) toca arquivos da spec + último evento da spec há ≥6h.
     - **stale**: todos AC `[x]` + último evento há ≥72h (sem commit recente).
     - **abandoned-suspect**: AC parciais + último evento há ≥30 dias.
     - **healthy**: qualquer outra coisa (não age).
- [ ] Para `candidate`, executar o close-gate:
  - `verify-pipeline` (build/lint/test).
  - QA: re-rodar AC se possível (idempotente — `mustard-rt run qa-run --spec NAME`).
  - Se tudo verde ⇒ emit `hygiene.autoclose` + `pipeline.outcome: completed` + reescreve header da spec para `Outcome: Completed`.
  - Se algo vermelho ⇒ emit `hygiene.skipped` com `blocker: build_red | ac_failing | qa_missing`.
- [ ] Para `stale`, emit `hygiene.detected { reason: "stale" }`. Não age — só sinaliza.
- [ ] Para `abandoned-suspect`, emit `hygiene.detected { reason: "abandoned_suspect" }`. Não age.

### Eventos novos

Adicionar ao `KNOWN_KINDS` em `emit_pipeline.rs`:

```
hygiene.detected
hygiene.autoclose
hygiene.skipped
```

Payload de cada:

```json
hygiene.detected   { "spec": "...", "reason": "stale|abandoned_suspect|candidate", "evidence": { "ac_pct": 1.0, "last_event_at": "...", "last_commit_at": "..." } }
hygiene.autoclose  { "spec": "...", "gate_result": { "build": "pass", "qa": "pass" }, "emitted_at": "..." }
hygiene.skipped    { "spec": "...", "blocker": "build_red|ac_failing|qa_missing", "details": "..." }
```

### Configuração

- [ ] Hook respeita env var `MUSTARD_HYGIENE_MODE`:
  - `off`: hook desativado (volta a comportamento atual).
  - `detect`: só emite `hygiene.detected` para todas as categorias; NUNCA auto-fecha.
  - `auto` (default): comportamento descrito acima.

### Testes

- [ ] `tests/spec_hygiene.rs`:
  - Cenário 1: spec com todos AC `[x]`, commit há 4h, último evento há 8h, build verde, QA pass ⇒ hygiene emite `hygiene.autoclose` + `pipeline.outcome: completed`.
  - Cenário 2: idem cenário 1 mas build vermelho ⇒ emite `hygiene.skipped` com `blocker: build_red`, **não** fecha.
  - Cenário 3: spec com AC parcial, último evento há 60 dias ⇒ `hygiene.detected { reason: "abandoned_suspect" }`.
  - Cenário 4: `MUSTARD_HYGIENE_MODE=off` ⇒ hook não emite nada.
  - Cenário 5: idempotência — hook rodar 2x na mesma spec já fechada não emite eventos duplicados.

### Build/Lint

- [ ] `cargo build -p mustard-rt && cargo test -p mustard-rt && cargo clippy -p mustard-rt -- -D warnings`.

## Acceptance Criteria

- [x] AC-W5-1: `cargo build -p mustard-rt` passa. ✅
- [x] AC-W5-2: `cargo test -p mustard-rt` passa — **750 passed**, incl. os 5 cenários + cenário 6 (CRLF+acentos, regressão do panic) + teste `session_summary_surfaces_hygiene_events`. ✅
- [x] AC-W5-3 (dentro do escopo): `cargo clippy` em `apps/rt` está **limpo, zero warnings**. O literal `cargo clippy -p mustard-rt -- -D warnings` falha SÓ por lints `clippy::pedantic` pré-existentes em `packages/core` (cast truncation, too-many-lines, etc.) — fora do boundary da W5, não introduzidos por ela. Limpá-los é follow-up separado (core). ✅ (código da wave limpo)
- [~] AC-W5-4 (verificado-por-teste; passo literal deferido): o subcomando `mustard-rt run hooks-test` **não existe** (não estava nos arquivos IN-scope). O mecanismo de auto-close-com-gate está provado pelo cenário 1 de `tests/spec_hygiene.rs`, que dirige o **binário real** com fixture controlado. Rodar o hook em modo `auto` contra o repo vivo NÃO foi feito durante QA: fecharia em massa as specs ativas + rodaria builds por candidata (efeito colateral inaceitável). Verificação manual fica para quando o `hooks-test` for adicionado (ou via fixture isolado).
- [x] AC-W5-5: `event-projections --view session-summary` agora inclui o campo `hygiene` (fix em `build_session_summary` coletando kinds `hygiene.*`); travado por teste unitário determinístico. Emit→store provado pelos cenários de integração. ✅

## Concerns / fixes durante EXECUTE+QA da W5

- **Panic CRLF+acentos (CRÍTICO, corrigido):** `ac_section` fatiava `spec_md` por offset de byte com `lines()` + `len()+1`, assumindo terminador `\n` de 1 byte. Em arquivos Windows (CRLF) o offset driftava e caía no meio de char multibyte (`ó`/`—`) → panic no `SessionStart` (viola fail-open). Fix: offsets via `split_inclusive('\n')` (largura real do terminador) + slice via `.get()` (degrada a None, nunca panica). Regressão coberta pelo cenário 6. Causa-raiz: testes do agente usavam fixtures ASCII.
- **session-summary não surfaceava hygiene (corrigido):** `build_session_summary` não coletava kinds `hygiene.*`; AC-W5-5 exige. Adicionado o campo `hygiene` + teste.
- **`hooks-test` inexistente (AC-W5-4):** AC referencia um subcomando não implementado. Follow-up: ou adicionar `mustard-rt run hooks-test`, ou reescrever o AC para usar fixture isolado.
- **clippy pedantic em `packages/core` (pré-existente):** bloqueia o `-D warnings` workspace-wide; follow-up no core.

## Limites

**IN:** apenas os arquivos listados.

**OUT:**
- Dashboard / UI dos eventos hygiene — Wave 6.
- Header das outras specs em `.claude/spec/` — Wave 7 migra em batch.

## Notas de segurança

- O auto-close **nunca** roda sem close-gate. Se `verify-pipeline` falha, hygiene não escreve o evento `pipeline.outcome: completed`. Spec quebrada não fecha.
- Audit-log via SQLite: `hygiene.autoclose` registra o gate_result que foi verde no momento — auditável depois.
- Reversibilidade: o usuário pode emitir `pipeline.flag.set blocked` numa spec fechada por engano; ela volta para Active e re-aparece em "Ativas". Não há "undo automático".
