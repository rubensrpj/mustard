# Tactical Fix: spec picker perf + SQLite backfill multi-dev

## Contexto

Tactical fix derivado de [[2026-05-23-tf-unify-spec-command]].

O SKILL `/mustard:spec` (criado pela TF parent que unificou `/approve` + `/resume` num picker único) faz toda a descoberta de specs ativas no próprio LLM: glob de `.claude/spec/*/spec.md` + `.claude/spec/*/wave-plan.md`, grep dos headers `### Stage:` / `### Outcome:` / `### Scope:` / `### Parent:` em todas as specs do repo, e leitura individual das primeiras linhas de cada spec ativa para extrair Resumo. Em projetos com muitas specs arquivadas (o próprio Mustard tem ~95 specs hoje, a maioria Closed/Completed), a operação consome >60k tokens só para imprimir uma tabela de 9 linhas. O crescimento é linear no total de specs (não nas ativas), inviável para projetos longevos.

Diagnóstico medido nesta sessão (2026-05-23):

| Operação | Tokens aprox |
|---|---|
| `Grep ^### (Stage\|Outcome\|Scope\|Parent):` em `.claude/spec/` | ~67k (768 linhas matched, persistidas em arquivo e re-lidas em 2 páginas) |
| Re-Read das 9 specs ativas para extrair `## Resumo` (1ª frase ≤70 chars) | ~10k |
| `event-projections --view active-pipelines` + `sync-registry` | ~2k |

Toda essa lógica é determinística: globar, parsear header de markdown, contar waves filhas, achar a primeira frase de `## Resumo` ou fallback `## Contexto`, resolver alias curto pra parents. Não precisa de LLM. Deveria viver num subcomando do `mustard-rt` que devolve a tabela já pronta — o LLM só imprime.

**Segundo problema descoberto pelo usuário:** em uso multi-dev, quando alguém faz `git pull` e recebe specs criadas por outro programador, a `spec.md` aparece no disco mas não há eventos `pipeline.stage` / `pipeline.status` no SQLite local (eventos são persistidos por máquina, não commitados). Resultado: `event-projections --view active-pipelines` não enxerga essas specs, e por consequência o dashboard também não. O picker novo (que vai depender do filesystem, não da projeção) ainda enxergaria — mas o dashboard continuaria cego, e qualquer outro projetor cross-machine teria o mesmo gap. Precisa backfill no SQLite a partir do header da spec.md quando o evento estiver ausente.

## Critérios de Aceitação

Todos os ACs rodam cross-shell (Windows cmd.exe + bash) via `node -e` ou comando único — sem `for`/`test`/`$()`/`[ ]`, conforme convenção do projeto.

- **AC-1** — subcomando existe: `rtk mustard-rt run active-specs --help` retorna exit 0 e a saída contém `--format`.
  ```bash
  rtk mustard-rt run active-specs --help
  ```

- **AC-2** — descoberta filesystem-canônica + filtro Plan/Execute/Active: rodando `rtk mustard-rt run active-specs --format json` no repo Mustard, a saída JSON contém pelo menos as 9 specs identificadas hoje (`2026-05-23-dashboard-design-system`, `2026-05-23-tf-dashboard-ds-tokens-remap`, `2026-05-23-tf-dashboard-eslint-baseline`, `2026-05-23-tf-dashboard-page-primitives`, `2026-05-22-project-profiler`, `2026-05-21-tf-skill-mirror`, `2026-05-21-wave-integrity-and-doctor-check`, `2026-05-21-mustard-v1-installer-and-update`, `2026-04-09-pipeline-gates-bundle-SUPERSEDED`) e ZERO specs com `Stage=Close`.
  ```bash
  node -e "const r=JSON.parse(require('child_process').execSync('rtk mustard-rt run active-specs --format json').toString()); const want=['2026-05-23-dashboard-design-system','2026-05-23-tf-dashboard-ds-tokens-remap','2026-05-23-tf-dashboard-eslint-baseline','2026-05-23-tf-dashboard-page-primitives','2026-05-22-project-profiler','2026-05-21-tf-skill-mirror','2026-05-21-wave-integrity-and-doctor-check','2026-05-21-mustard-v1-installer-and-update','2026-04-09-pipeline-gates-bundle-SUPERSEDED']; const got=new Set(r.specs.map(s=>s.name)); const miss=want.filter(w=>!got.has(w)); if(miss.length) throw new Error('miss: '+miss.join(',')); if(r.specs.some(s=>s.stage==='Close')) throw new Error('Close vazou')"
  ```

- **AC-3** — backfill SQLite idempotente: após rodar `active-specs`, toda spec listada tem pelo menos UM evento `pipeline.stage` OU `pipeline.status` no SQLite. Rodar o comando duas vezes não duplica eventos (a 2ª execução deve achar todos presentes e emitir 0).
  ```bash
  node -e "const cp=require('child_process'); cp.execSync('rtk mustard-rt run active-specs --format json'); const before=JSON.parse(cp.execSync('rtk mustard-rt run event-projections --view active-pipelines --format json').toString()).pipelines.length; cp.execSync('rtk mustard-rt run active-specs --format json'); const after=JSON.parse(cp.execSync('rtk mustard-rt run event-projections --view active-pipelines --format json').toString()).pipelines.length; if(before!==after) throw new Error('non-idempotent: '+before+' vs '+after)"
  ```

- **AC-4** — saída `--format table` é markdown válido pronto pra imprimir: contém header `| #` e pelo menos uma linha começando com `| a `.
  ```bash
  node -e "const out=require('child_process').execSync('rtk mustard-rt run active-specs --format table').toString(); if(!out.includes('| #')) throw new Error('header faltando'); if(!/^\| a /m.test(out)) throw new Error('linha a faltando')"
  ```

- **AC-5** — SKILL atualizada não faz mais glob/grep no LLM: `templates/commands/mustard/spec/SKILL.md` e `.claude/commands/mustard/spec/SKILL.md` NÃO contêm as strings `Glob ` nem `^### (Stage` nem `Step 2: Discovery` na seção `## Action`.
  ```bash
  node -e "const fs=require('fs'); for(const p of ['apps/cli/templates/commands/mustard/spec/SKILL.md','.claude/commands/mustard/spec/SKILL.md']){const s=fs.readFileSync(p,'utf8'); if(s.includes('Step 2: Discovery')) throw new Error(p+': discovery step ainda presente'); if(s.includes('Glob `.claude/spec')) throw new Error(p+': glob LLM ainda presente');}"
  ```

- **AC-6** — `--format json` é parseável e tem schema esperado: cada item tem `name`, `stage`, `outcome`, `scope`, `resumo`, `letter` e opcionalmente `parent`, `parentAlias`, `progress: {done, total}`.
  ```bash
  node -e "const r=JSON.parse(require('child_process').execSync('rtk mustard-rt run active-specs --format json').toString()); for(const s of r.specs){if(!s.name||!s.stage||!s.outcome||!s.letter) throw new Error('schema miss em '+JSON.stringify(s));}"
  ```

## Arquivos

- `apps/rt/src/run/active_specs.rs` (NEW) — implementação do subcomando: glob, parse de header, filtro, contagem de waves, extração de resumo, resolução de alias, backfill SQLite, renderização table/json
- `apps/rt/src/run/mod.rs` (EDIT) — `pub mod active_specs;`
- `apps/rt/src/main.rs` ou `apps/rt/src/dispatch.rs` (EDIT) — registrar subcomando no clap + roteador `run`
- `apps/cli/templates/commands/mustard/spec/SKILL.md` (EDIT — fonte canônica) — substitui Steps 2 e 3 por chamadas ao binário; mantém Step 4 (auditoria) e Step 5+ (parsing de input e roteamento)
- `.claude/commands/mustard/spec/SKILL.md` (EDIT — cópia instalada no repo Mustard, conforme [[feedback_mustard_self_scripts_stale]]) — mirror byte-equivalente do templates/
- `apps/rt/tests/active_specs.rs` ou inline `#[cfg(test)]` (NEW) — testes unitários: parse de header válido/inválido, filtro Plan+Execute, contagem de waves (4/6, 0/5), extração de resumo (## Resumo prioridade, fallback ## Contexto, trunca em 70), alias de parent (TF→ds), backfill SQLite idempotente

## Plano de implementação (resumido)

1. **`active_specs.rs`** — módulo principal:
   - `discover_root_specs() -> Vec<SpecCandidate>`: glob `.claude/spec/*/spec.md` + `.claude/spec/*/wave-plan.md`, exclui `wave-N-*/`, `review/`, `qa/` no path
   - `parse_header(path) -> SpecHeader`: lê só os primeiros ~25 bytes via `BufReader::take`, parsea `### Stage:`, `### Outcome:`, `### Scope:`, `### Parent:` por regex linha-a-linha; para na 1ª linha vazia após o último `###`
   - `count_wave_progress(spec_dir) -> Option<(done, total)>`: glob `<spec_dir>/wave-N-*/spec.md`, lê header de cada, conta `Stage=Close`+`Outcome=Completed` vs total
   - `extract_resumo(path) -> String`: busca `## Resumo` ou fallback `## Contexto` no body, pega 1ª frase (até `.` ou `\n\n`), trunca em 70 chars com `…`
   - `resolve_parent_alias(parents: &[String]) -> HashMap<String, String>`: gera alias curto (2-3 chars do slug final) único por parent. Ex.: `dashboard-design-system` → `ds`, `flatten-spec-layout-and-multi-collab` → `fl`. Em colisão, vai estendendo até 1 char extra.
   - `backfill_sqlite(specs: &[Spec], store: &EventStore) -> usize`: para cada spec, query `SELECT 1 FROM events WHERE spec = ?1 AND kind IN ('pipeline.stage','pipeline.status') LIMIT 1`; se ausente, emite `pipeline.stage` + `pipeline.status` derivados do header atual. Retorna count de backfills realizados.
   - `render_table(specs: &[Spec]) -> String`: gera markdown da tabela já formatada com letras a-z + colunas + alinhamento
   - `render_json(specs: &[Spec]) -> String`: serde_json com schema do AC-6

2. **Backfill detail** — eventos sintéticos devem ter `source: "backfill-from-filesystem"` no payload pra serem distinguíveis de eventos reais; timestamp = `Checkpoint:` do header se disponível, senão `mtime` do `spec.md`.

3. **SKILL update** — `templates/commands/mustard/spec/SKILL.md` (e mirror):
   - Step 1 (sync) mantém
   - Step 2/3 viram: `rtk mustard-rt run active-specs --format table` + imprimir saída verbatim
   - Step 4 (Siglas/Modo) movido pra **bloco estático embutido no SKILL** (não muda nunca, então não precisa render dinâmica)
   - Step 5+ (parsing letra+r, roteamento PLAN/EXEC) inalterado

## Não-Objetivos

- **Não** muda projeções de event-projections além do backfill (o view `active-pipelines` continua funcionando como está, agora só com dados mais completos)
- **Não** muda o dashboard nesta TF — ele já lê `active-pipelines`; ganha as specs faltantes "de graça" pelo backfill
- **Não** persiste cache do scan no disco (overhead desnecessário pra ~95 specs; bench mostra <50ms cold em Rust)
- **Não** muda Steps 5-8 do SKILL (parsing de input + roteamento aprovar/continuar). Esses já são determinísticos no LLM e baratos.

## Riscos eliminados por design

- **Backfill incorreto sobrescrevendo histórico**: só emite quando ausente (`SELECT 1 LIMIT 1`); nunca UPDATE; idempotente.
- **Alias colidindo**: algoritmo cresce o sufixo até ser único; verificado em teste com 10+ parents próximos.
- **SKILL mirror dessincronizando**: o `## Arquivos` lista os 2 paths explicitamente; AC-5 valida ambos; CLOSE-gate da pipeline pega drift.
