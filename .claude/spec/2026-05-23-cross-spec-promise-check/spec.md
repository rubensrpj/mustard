# Enhancement: cross-spec promise check

## PRD

## Contexto

O `dependency-precheck` recém-entregue valida que cada símbolo referenciado por uma spec existe no codebase via grep. Quando algo está faltando, ele diz **o que** falta, mas não **por que** — não distingue dois cenários muito diferentes: (a) uma wave anterior prometeu entregar o símbolo e não cumpriu (bug de implementação na wave passada), ou (b) nenhuma wave anterior nunca prometeu criar o símbolo (bug de premissa na spec atual). O caso real motivador desta extensão foi a Wave 5 do `dashboard-design-system`: a spec afirma "Wave 2 entregou `EditorialBand`, `KpiValue`, etc." mas a Wave 2 explicitamente excluiu esses primitives do escopo (linhas 57-60 de `wave-2-ui/spec.md`). O gate atual reportou os símbolos como missing sem flagrar essa contradição entre specs — o `AskUserQuestion` resultante teria a mesma forma nos dois cenários, embora a ação correta seja diferente (reabrir wave passada vs. corrigir premissa da wave atual).

## Métrica de sucesso

Rodar `mustard-rt run dependency-precheck --spec .claude/spec/2026-05-23-dashboard-design-system/wave-5-ui/spec.md` retorna, além do `missing` atual, um campo novo `promise_violations` com cada símbolo missing classificado: `parent_promised` (qual wave anterior listou o símbolo no `## Arquivos`) e `actually_delivered` (boolean). Para EditorialBand/KpiValue/etc., todos saem como `no_parent_promised_this: true` — sinalizando o bug de premissa, não de implementação.

## Critérios de Aceitação

- [x] AC-1: cargo build + cargo test verdes — Command: `cargo build -p mustard-rt && cargo test -p mustard-rt dependency_precheck`
- [x] AC-2: fixture wave-plan onde wave-2 promete mas não entregou aparece como `parent_promised:"wave-2-ui", actually_delivered:false` — Command: `node -e "const out=require('child_process').execSync('cargo run -q -p mustard-rt -- run dependency-precheck --spec apps/rt/tests/fixtures/dependency_precheck/promise_violation/wave-3-ui/spec.md',{encoding:'utf8'});const r=JSON.parse(out);const pv=(r.promise_violations||[]).find(p=>p.symbol==='PromisedButMissing');if(!pv){console.error('expected promise_violations entry for PromisedButMissing, got:',JSON.stringify(r.promise_violations));process.exit(1)}if(pv.parent_promised!=='wave-2-ui'||pv.actually_delivered!==false){console.error('wrong fields:',JSON.stringify(pv));process.exit(1)}console.log('ok')"`
- [x] AC-3: fixture wave-plan onde current wave usa símbolos que nenhum parent prometeu (bug de premissa, estilo Wave 5) — `NeverPromisedA` e `NeverPromisedB` saem como `no_parent_promised_this:true` — Command: `node -e "const out=require('child_process').execSync('cargo run -q -p mustard-rt -- run dependency-precheck --spec apps/rt/tests/fixtures/dependency_precheck/promise_violation/wave-3-ui/spec.md',{encoding:'utf8'});const r=JSON.parse(out);const pv=r.promise_violations||[];for(const sym of ['NeverPromisedA','NeverPromisedB']){const e=pv.find(p=>p.symbol===sym);if(!e){console.error('missing promise_violations entry for',sym,'— got:',JSON.stringify(pv));process.exit(1)}if(e.no_parent_promised_this!==true){console.error('wrong classification for',sym,':',JSON.stringify(e));process.exit(1)}}console.log('ok')"`

## Plano

## Summary

Estender `dependency_precheck.rs` com função `extract_parent_wave_promises(spec_path) -> HashMap<String, String>` (símbolo → wave que prometeu) que: (1) localiza `wave-plan.md` no spec dir; (2) parseia tabela de waves identificando deps; (3) para cada parent wave, lê `wave-N-{role}/spec.md` e extrai `## Arquivos`; (4) deriva símbolo PascalCase de cada path. Cruzar com `missing` atual; emitir `promise_violations` no JSON. Sem mudança em SKILL.md — campo é puramente aditivo (orquestrador opcional consome).

## Checklist

### rt-impl Agent

- [x] Adicionar em `apps/rt/src/run/dependency_precheck.rs`:
  - `fn find_wave_plan(spec_path: &Path) -> Option<PathBuf>` — sobe do spec_path até achar `wave-plan.md` (max 2 níveis: spec_path geralmente é `.claude/spec/{slug}/wave-N-role/spec.md`, wave-plan em `../wave-plan.md`)
  - `fn parse_wave_plan_deps(plan_text: &str, current_wave: u32) -> Vec<u32>` — parseia tabela markdown (linhas `| N | ... | Depende de` ou `Depends on`); extrai números de wave da coluna deps da current_wave; suporta formato `[[N]]`, `[[wave-N-role]]`, `N`, `wave-N`
  - `fn extract_wave_number_from_spec_path(spec_path: &Path) -> Option<u32>` — match em `wave-(\d+)-` no parent dir name
  - `fn parent_wave_promises(spec_dir: &Path, parent_wave_nums: &[u32]) -> HashMap<String, u32>` — para cada parent wave, glob `{spec_dir}/wave-{N}-*/spec.md`, ler, extrair `## Arquivos`, derivar símbolos (reusar `parse_self_created` que já faz isso); map símbolo → wave que prometeu
  - Em `run()`, após calcular `missing`, computar `promise_violations`:
    ```
    [{
      "symbol": "X",
      "parent_promised": "wave-2-ui" (Some) | null,
      "actually_delivered": false (sempre — está em missing),
      "no_parent_promised_this": true | omit
    }]
    ```
    Lógica: se símbolo missing está no map → entry com `parent_promised`/`actually_delivered:false`; se não está em nenhum parent map → entry com `no_parent_promised_this:true`.
  - Output JSON: adicionar `promise_violations` na lista alfabética de keys (entre `mode` e `spec`? Sim — alfabeticamente vai depois de `mode` e antes de `ok`). Atualizar ordem.
  - Fallback: se `find_wave_plan` retorna None (single-spec, não wave-plan), `promise_violations: []` — não pula, só fica vazio.

- [x] Criar fixtures em `apps/rt/tests/fixtures/dependency_precheck/promise_violation/`:
  - `wave-plan.md`:
    ```
    # Wave Plan
    ### Stage: Plan
    ### Outcome: Active
    ### Lang: en

    | Wave | Role | Depende de | Resumo |
    |------|------|------------|--------|
    | 1 | general | — | foundation |
    | 2 | ui | [[1]] | primitives |
    | 3 | ui | [[2]] | pages — uses PromisedButMissing + NeverPromised |
    ```
  - `wave-1-general/spec.md`: header mínimo + `## Files\n- apps/rt/src/foundation.rs\n`
  - `wave-2-ui/spec.md`: header mínimo + `## Files\n- apps/dashboard/src/components/page/PromisedButMissing/index.tsx\n` (promete mas o arquivo NÃO existe no disco — promise violada)
  - `wave-3-ui/spec.md`: header mínimo + `## Files\n- apps/dashboard/src/pages/Demo.tsx\n` + corpo: `<PromisedButMissing>` (promise violada — wave-2 prometeu, arquivo não existe) + `<NeverPromisedA>` e `<NeverPromisedB>` (sem promessa parental nenhuma — estilo Wave 5)

- [x] Unit tests em `#[cfg(test)] mod tests`:
  - `parse_wave_plan_deps_extracts_wikilink` — `[[2]]` e `[[wave-2-ui]]` viram `2`
  - `parent_wave_promises_collects_symbols` — fixture inline com 2 parent specs declarando símbolos diferentes
  - `promise_violations_classifies_correctly` — integração: missing+promise→`actually_delivered:false`; missing sem promessa→`no_parent_promised_this:true`
  - `no_wave_plan_returns_empty` — single-spec mode → `promise_violations: []`

- [x] `cargo test -p mustard-rt dependency_precheck` verde.

## Files (~3)

- `apps/rt/src/run/dependency_precheck.rs` (extensão ~100 LOC)
- `apps/rt/tests/fixtures/dependency_precheck/promise_violation/{wave-plan.md, wave-1-general/spec.md, wave-2-ui/spec.md, wave-3-ui/spec.md}` (4 fixtures novos)

## Limites

Editar dentro de:
- `apps/rt/src/run/dependency_precheck.rs`
- `apps/rt/tests/fixtures/dependency_precheck/promise_violation/**` (novo dir)

**Não tocar:**
- `apps/rt/src/run/mod.rs` (subcomando já registrado)
- SKILL.md (mudança puramente aditiva no JSON; orchestrator opcional consome)
- Qualquer outro arquivo fora de `apps/rt/`
