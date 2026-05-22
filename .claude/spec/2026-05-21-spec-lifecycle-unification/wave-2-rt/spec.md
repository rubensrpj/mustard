# Wave 2 — RT: spec_children_tree + AC detalhado + emit-pipeline aceita novos kinds

### Parent: [[2026-05-21-spec-lifecycle-unification]]
### Wave: 2
### Role: rt
### Stage: Close
### Outcome: Completed
### Flags: 
### Lang: pt
### Checkpoint: 2026-05-22T00:15:00Z

## Resumo

Adiciona ao `mustard-rt` o subcomando `spec-children-tree` (consumido pelo dashboard via Tauri command de mesmo nome em W3) que retorna `{waves: [...], acs: [...], subspecs: [...]}` em um único round-trip. Expande o projection de AC para conter cada AC individualmente (não só `ac_passed`/`ac_total` agregados). Adiciona suporte no `emit-pipeline` para os novos kinds `pipeline.stage` / `pipeline.outcome` / `pipeline.flag.set` / `pipeline.flag.clear`, mantendo os legados (`pipeline.status` / `pipeline.phase`) como aliases que escrevem o evento novo equivalente.

## Arquivos

```
apps/rt/src/run/spec_children_tree.rs        (novo — subcomando + projection)
apps/rt/src/run/mod.rs                        (registrar o subcomando)
apps/rt/src/run/emit_pipeline.rs              (aceitar novos --kind; manter aliases)
apps/rt/src/run/spec_sections.rs              (já leu novo formato em W1; nesta wave usa)
apps/rt/src/run/metrics_wave_status.rs        (alimenta children_tree.waves[])
apps/rt/src/run/rebuild_specs.rs              (atualizar para devolver SpecState consistente)
apps/rt/tests/spec_children_tree.rs           (novo)
apps/rt/tests/emit_pipeline_kinds.rs          (novo — aceita novos kinds)
```

## Tarefas

- [x] Implementar `run/spec_children_tree.rs::run(spec: &str) -> Result<ChildrenTree>` onde `ChildrenTree { waves: Vec<WaveChild>, acs: Vec<AcChild>, subspecs: Vec<SpecChild> }`.
- [x] `WaveChild { idx: u32, role: String, status: WaveStatus, started_at: Option<DateTime>, completed_at: Option<DateTime>, duration_ms: Option<i64> }`.
- [x] `AcChild { id: String, label: String, status: AcStatus, last_run_at: Option<DateTime>, evidence: Option<String> }` — `evidence` é o stdout/stderr resumido do AC pass/fail.
- [x] `SpecChild` já existe em `mustard-core` (sub-specs via `spec.link`). Reusar e popular o `status` derivando `SpecState` da sub-spec.
- [x] Adicionar o subcomando à dispatcher: `mustard-rt run spec-children-tree --spec NAME` retorna JSON.
- [x] Estender `emit_pipeline.rs::KNOWN_KINDS` com `pipeline.stage`, `pipeline.outcome`, `pipeline.flag.set`, `pipeline.flag.clear`. Conferir o memory entry [[project_emit_pipeline_kind_full_prefix]] — sem alias mágico, lista literal.
- [x] Mapping reverso: ao receber `pipeline.status: completed`, **também** escrever `pipeline.outcome: completed` (no mesmo timestamp, mesmo `sessionId`) para a transição. Idem `pipeline.phase: execute` ⇒ também `pipeline.stage: execute`. Tag `legacy_alias=true` no payload do evento legado para auditoria.
- [x] Testar idempotência: emitir o evento novo direto NÃO duplica.
- [x] `tests/spec_children_tree.rs`: cria spec sintética em tmpdir com 2 waves, 3 ACs (1 pass, 1 fail, 1 pending), 1 sub-spec. Verifica o JSON.
- [x] `tests/emit_pipeline_kinds.rs`: emite `pipeline.stage` direto; emite `pipeline.phase` legado; verifica que ambos resultam em row equivalente no SQLite.
- [x] Rodar `cargo build -p mustard-rt && cargo test -p mustard-rt && cargo clippy -p mustard-rt -- -D warnings`.

## Shape do JSON devolvido (contrato com W3)

```json
{
  "spec": "2026-05-21-flatten-spec-layout",
  "waves": [
    { "idx": 1, "role": "spec-hygiene", "status": "completed", "started_at": "...", "completed_at": "...", "duration_ms": 120000 },
    { "idx": 2, "role": "rt-events", "status": "in-progress", "started_at": "...", "completed_at": null, "duration_ms": null }
  ],
  "acs": [
    { "id": "AC-W4-1", "label": "grep returns empty", "status": "pass", "last_run_at": "...", "evidence": null },
    { "id": "AC-W4-2", "label": "build is green", "status": "fail", "last_run_at": "...", "evidence": "exit 101" }
  ],
  "subspecs": [
    { "spec": "2026-05-21-tf-skill-mirror", "status": "qa-review", "started_at": "...", "completed_at": null, "reason": "tactical-fix" }
  ]
}
```

`status` dentro de `subspecs` é o `Stage` da sub-spec (kebab-case). Frontend (W3) decide como pintar.

## Acceptance Criteria

- [x] AC-W2-1: `cargo build -p mustard-rt` passa.
- [x] AC-W2-2: `cargo test -p mustard-rt` passa.
- [x] AC-W2-3: `cargo clippy -p mustard-rt -- -D warnings` passa.
- [x] AC-W2-4: `mustard-rt run spec-children-tree --spec 2026-05-21-flatten-spec-layout-and-multi-collab` retorna JSON com pelo menos uma sub-spec (a `tf-skill-mirror` ligada).
- [x] AC-W2-5: `mustard-rt run emit-pipeline --kind pipeline.stage --spec test --payload '{"stage":"execute"}'` grava evento no SQLite com kind `pipeline.stage`.
- [x] AC-W2-6: `mustard-rt run emit-pipeline --kind pipeline.phase --spec test --payload '{"phase":"execute"}'` grava **dois** eventos (legacy + novo) com mesmo timestamp.

## Limites

**IN:** apenas os arquivos listados.

**OUT:**
- `mustard-core` — já em W1 (apenas consumido aqui).
- `apps/dashboard/src-tauri/` — Wave 3 conecta o command Tauri ao subcomando rt.
- Skills do CLI — Wave 4.
- Hook hygiene — Wave 5.

## Review notes (APPROVED, 0 CRITICAL)

- **HANDOFF para W3 (dashboard):** o JSON de `subspecs[]` carrega TANTO `state` (canônico `{stage,outcome,flags}`) QUANTO o `status` legado (`SpecStatus`, ex.: `"planning"`). W3 DEVE renderizar a partir de `state.stage` (kebab), **nunca** do `status` deprecado — eles divergem durante a janela W1→W7 (ex.: `approved`→`status:"planning"` mas `state.stage:"plan"`).
- WARNING: `pipeline.status {to:"active"}` não gera alias (nem tag `legacy_alias`) — `"active"` não é valor de status emitido na prática; edge teórico, deixado como no-op silencioso. Documentar se vier a ser emitido.
- NOTE: payload do teste de phase legada inclui chave `"phase"` morta (alias só lê `"to"`); cosmético.
