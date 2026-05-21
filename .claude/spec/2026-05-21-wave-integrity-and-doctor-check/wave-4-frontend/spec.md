# wave-4-frontend

### Parent: [[2026-05-21-wave-integrity-and-doctor-check]]
### Status: queued
### Phase: PLAN

## Resumo

Fecha o loop UX. Wave 3 entrega o check `wave-integrity` no doctor, mas hoje o usuário só vê o resultado se lembrar de rodar `/mustard:maint doctor` manualmente. Esta wave faz o dashboard sugerir — adiciona uma flag `--json` ao doctor (output parseável), um Tauri command que invoca `mustard-rt run doctor --json` no mount do app, e um badge sutil no footer da Sidebar que muda de cor conforme severidade (verde OK / amarelo WARN com contagem / vermelho FAIL). Click no badge abre um tooltip com a lista de checks falhos + hint dos comandos de fix. Sem polling: refresh manual via botão no tooltip ou remontagem do app. Memory `feedback_no_permission_loops`: chamada do binário é via Tauri command, sem prompt de permissão por edit.

## Network

- Parent: [[2026-05-21-wave-integrity-and-doctor-check]]
- Depende de: [[wave-3-general]]

## Arquivos

```
apps/rt/src/run/doctor.rs                              — modify: nova flag --json (CheckResult → JSON via serde_json)
apps/dashboard/src-tauri/src/lib.rs                    — modify: register doctor_status como Tauri command
apps/dashboard/src-tauri/src/doctor.rs                 — new: invoke mustard-rt run doctor --json + parse + return struct
apps/dashboard/src/lib/doctor.ts                       — new: types + hook useDoctorStatus
apps/dashboard/src/components/DoctorBadge.tsx          — new: badge com tooltip
apps/dashboard/src/components/layout/Sidebar.tsx       — modify: render DoctorBadge no footer
```

## Tarefas

**Subtask A — `--json` flag no doctor:**

- [ ] Adicionar variant `--json` ao parser de `RunCmd::Doctor` em `apps/rt/src/run/mod.rs` (já existe a flag `--residue`; seguir mesmo padrão).
- [ ] Em `apps/rt/src/run/doctor.rs::run()`, ramificar: se `json=true`, em vez de chamar `render_report()`, serializar `results: Vec<CheckResult>` com `serde_json::to_string_pretty` e imprimir em stdout. Adicionar `#[derive(Serialize)]` em `CheckResult` e `Status` (com `#[serde(rename_all = "lowercase")]` em `Status`).
- [ ] Manter exit-code: 1 se algum `Fail`, 0 caso contrário — exatamente como hoje.
- [ ] Adicionar `#[test] fn doctor_json_emits_parseable_output()`: cria tempdir com `.claude/settings.json` válido, roda `run(false, true)` capturando stdout, valida `serde_json::from_str` retorna struct com campo `checks`.

**Subtask B — Tauri command:**

- [ ] Criar `apps/dashboard/src-tauri/src/doctor.rs` com `pub fn doctor_status(workspace: &Path) -> Result<DoctorStatus, String>`. Body: `Command::new("mustard-rt").args(["run","doctor","--json"]).current_dir(workspace).output()`, parse stdout via `serde_json::from_slice::<DoctorStatus>(&out.stdout)`. Struct: `{ checks: Vec<CheckEntry>, overall: String }` (overall = `"ok"|"warn"|"fail"` derivado dos checks).
- [ ] Registrar como Tauri command em `apps/dashboard/src-tauri/src/lib.rs`: `#[tauri::command] async fn doctor_status(state: tauri::State<AppState>) -> ...` no `invoke_handler` (seguir o pattern dos commands existentes em `spec_views.rs`).
- [ ] Fail-open: erro de spawn ou parse retorna `DoctorStatus { checks: [], overall: "unknown" }` em vez de propagar — o badge mostra ícone neutro quando não conseguir checar.

**Subtask C — Frontend:**

- [ ] Criar `apps/dashboard/src/lib/doctor.ts`: types `DoctorStatus`, `CheckEntry`; hook `useDoctorStatus()` que invoca o Tauri command via `invoke("doctor_status")` no mount, gerencia loading/data/error com `useState`. Expor função `refresh()` que re-invoca.
- [ ] Criar `apps/dashboard/src/components/DoctorBadge.tsx`: componente compacto (dot colorido + texto curto). Cor: verde `bg-emerald-500` (overall=ok), amarelo `bg-amber-500` (warn), vermelho `bg-rose-500` (fail), cinza `bg-zinc-500` (unknown). Texto: "doctor: ok" / "doctor: 2 warn" / "doctor: 1 fail". Tailwind, sem libs novas.
- [ ] No click, abrir tooltip/popover com lista de checks que não são OK, exibindo `name` + `status` + primeiro `detail` (truncado a 80 chars). Usar `<details>` nativo ou popover existente do dashboard — não introduzir nova biblioteca de UI.
- [ ] Botão "refresh" no tooltip que chama `refresh()` do hook.
- [ ] Editar `apps/dashboard/src/components/layout/Sidebar.tsx`: adicionar `<DoctorBadge />` no footer da sidebar (logo abaixo do bloco de Preferences se existir, ou no final do container do component). Discreto, não invadir o layout principal.

**Build/test:**

- [ ] `cargo build -p mustard-rt && cargo test -p mustard-rt -- doctor`
- [ ] `pnpm --filter mustard-dashboard build`
- [ ] Smoke manual (orquestrador roda em EXECUTE): abrir o dashboard, ver o badge no footer da Sidebar; criar um `wave-plan.md` com referência a wave inexistente; refresh do badge passa de verde para amarelo com count >= 1.

## Acceptance Criteria

- [ ] AC-1: `mustard-rt run doctor --json` retorna JSON parseável com array `checks` — Command: `node -e "const cp=require('child_process');const j=JSON.parse(cp.execSync('mustard-rt run doctor --json').toString());if(!Array.isArray(j.checks))throw new Error('checks not array');if(!j.checks.every(c=>c.name&&c.status))throw new Error('missing fields')"`
- [ ] AC-2: Dashboard builda incluindo DoctorBadge e Sidebar referencia o componente — Command: `bash -c 'pnpm --filter mustard-dashboard build && grep -q DoctorBadge apps/dashboard/src/components/layout/Sidebar.tsx'`
- [ ] AC-3: Tauri command `doctor_status` registrado — Command: `bash -c 'grep -q "doctor_status" apps/dashboard/src-tauri/src/lib.rs'`
- [ ] AC-4: Testes do rt continuam verdes incluindo o novo doctor_json — Command: `cargo test -p mustard-rt -- doctor`

## Limites

- `apps/rt/src/run/doctor.rs` (apenas nova flag `--json` + serde derive)
- `apps/rt/src/run/mod.rs` (apenas adicionar flag ao parser do Doctor)
- `apps/dashboard/src-tauri/src/{lib.rs,doctor.rs}` (novo command + arquivo isolado)
- `apps/dashboard/src/lib/doctor.ts` (new)
- `apps/dashboard/src/components/DoctorBadge.tsx` (new)
- `apps/dashboard/src/components/layout/Sidebar.tsx` (apenas inserção do badge no footer)

Out-of-boundary: nenhuma outra página do dashboard, nenhum outro componente, nenhum check novo no doctor (são exatamente os 5 que existem + `wave-integrity` da Wave 3), Topbar (mantém-se como está).
