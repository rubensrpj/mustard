# wave-2-general

## Resumo

Eliminar a causa raiz do bug "só wave-1 criada". Hoje o SKILL `/feature` (linhas 138-156) descreve em prosa: "o orquestrador monta um `plan.json` com a decomposição em waves". Isso joga a responsabilidade de montar o JSON pro LLM, que regularmente esquece de iterar todas as waves no array. Esta wave troca a prosa por uma chamada a um subcomando Rust determinístico: `mustard-rt run plan-from-spec --waves N --roles a,b,c --summaries 's1|s2|s3' --deps '2:1;3:1,2' --lang pt|en` emite JSON canônico com array completo. O SKILL passa a chamar `plan-from-spec | wave-scaffold` em vez de pedir ao LLM para escrever JSON na mão.

## Network

- Parent: [[2026-05-21-wave-integrity-and-doctor-check]]
- Depende de: [[wave-1-library]]

## Arquivos

```
apps/rt/src/run/plan_from_spec.rs                       — new: parse flags → emit Plan JSON
apps/rt/src/run/mod.rs                                  — modify: register PlanFromSpec variant + match arm
apps/rt/src/run/doctor.rs                               — modify: add "plan-from-spec" em KNOWN_RUN_SUBCOMMANDS
apps/cli/templates/commands/mustard/feature/SKILL.md    — modify: substituir prosa por chamada concreta a plan-from-spec
```

## Tarefas

- [ ] Criar `apps/rt/src/run/plan_from_spec.rs` com a função `pub fn run(opts: PlanFromSpecOptions)` que recebe flags parseadas pelo `clap` no nível `mod.rs` e emite o JSON. Struct mínima: `{ waves: u32, roles: Vec<String> (csv parsed), summaries: Option<Vec<String>> (pipe-separated), deps: Option<String> (formato "2:1;3:1,2"), lang: Option<String> (default "pt") }`.
- [ ] Implementar parser de `deps`: separa por `;`, cada item `N:csv` vira `(N, vec![csv])`. Ex: `"2:1;3:1,2"` → `{2: [1], 3: [1,2]}`. Converte `N` em nome de wave usando o role correspondente (`wave-{N}-{role}`).
- [ ] Emit JSON com formato canônico do `Plan` (mesmo shape que `wave_scaffold::Plan`): `{"waves": [{"n":1,"role":"general","summary":"","depends_on":[]}, ...], "total_waves": N, "lang": "pt"}`. Imprime via `serde_json::to_string_pretty` em stdout. Fail-open: parse errors viram `{"error": "..."}` + exit 0.
- [ ] Registrar a variant em `apps/rt/src/run/mod.rs`: novo `RunCmd::PlanFromSpec(PlanFromSpecOptions)` no enum, novo match arm que chama `plan_from_spec::run(opts)`. Seguir o pattern do `cli-command-pattern` skill.
- [ ] Adicionar `"plan-from-spec"` à lista `KNOWN_RUN_SUBCOMMANDS` em `apps/rt/src/run/doctor.rs` (linhas 93-129). Sem isso o `wiring` check do doctor reporta FAIL.
- [ ] Editar `apps/cli/templates/commands/mustard/feature/SKILL.md` seção "Wave Decomposition (mandatory for Full+deps)" (linhas 138-156). Substituir o parágrafo "Sinais derivam da análise … o orquestrador monta um `plan.json`" por: "Sinais derivam da análise; o orquestrador chama `mustard-rt run plan-from-spec --waves N --roles ... --summaries ... --deps ... --lang ... > plan.json` para emitir o JSON canônico, depois `mustard-rt run wave-scaffold --spec-dir <dir> --plan plan.json`. Nunca monte `plan.json` manualmente." Manter o bloco `Formato esperado do plan.json` como referência informativa.
- [ ] Adicionar `#[test] fn emits_canonical_plan_for_two_waves()` em `plan_from_spec.rs`: roda com `waves=2 roles=general,frontend deps="2:1" lang=pt`, valida shape do JSON resultante.
- [ ] Adicionar `#[test] fn missing_role_for_wave_n_errors_out()`: roda com `waves=3 roles=a,b` (count mismatch), valida que stdout contém campo `error`.
- [ ] `cargo build -p mustard-rt && cargo test -p mustard-rt -- plan_from_spec`

## Acceptance Criteria

- [ ] AC-1: `mustard-rt run plan-from-spec --waves 2 --roles general,frontend --lang pt` emite JSON parseável com 2 entries — Command: `node -e "const cp=require('child_process');const j=JSON.parse(cp.execSync('mustard-rt run plan-from-spec --waves 2 --roles general,frontend --lang pt').toString());if(j.waves.length!==2)throw new Error(JSON.stringify(j))"`
- [ ] AC-2: SKILL `/feature` referencia `plan-from-spec` — Command: `node -e "const t=require('fs').readFileSync('apps/cli/templates/commands/mustard/feature/SKILL.md','utf8');if(!t.includes('plan-from-spec'))throw new Error('missing plan-from-spec')"`
- [ ] AC-3: `wiring` check do doctor passa após registro — Command: `bash -c 'mustard-rt run doctor 2>&1 | grep -qE "OK\\s+wiring"'`

## Limites

- `apps/rt/src/run/plan_from_spec.rs` (novo)
- `apps/rt/src/run/mod.rs` (apenas register + match)
- `apps/rt/src/run/doctor.rs` (apenas `KNOWN_RUN_SUBCOMMANDS`)
- `apps/cli/templates/commands/mustard/feature/SKILL.md` (apenas seção Wave Decomposition)

Out-of-boundary: lógica do `wave_scaffold.rs` (já feito na wave-1), `doctor.rs` checks (wave-3), demais SKILLs, dashboard, core.
