# Execução autônoma — Refatoração FUNCIONAL do mustard (min-IA / max-Rust)

> Cole este bloco como primeira mensagem numa sessão nova em `C:\Atiz\mustard`
> (ou continue na atual). Ele roda o backlog funcional inteiro sozinho, com
> subagentes, gate verde a cada commit. Pode dormir — volte com tudo pronto.

---

Continue a refatoração FUNCIONAL do workspace Rust mustard (C:\Atiz\mustard, branch
`dev_rubens`). Trabalhe de forma **AUTÔNOMA até o fim do backlog, sem pedir confirmação** —
todas as decisões de design já estão tomadas e gravadas. **Use SUBAGENTES** para implementar.
Commit a cada passo/fase **somente com a árvore verde**.

## ANTES DE TUDO (leia)
1. Memória (auto-carregada via MEMORY.md): **`project-mustard-functional-refactor`** (8
   decisões, 6 fases, **## PROGRESSO** com os commits já feitos), **`feedback-mustard-i18n-agnostic`**,
   `feedback-no-facade-consolidation`, `feedback-engineer-decide-verify`.
2. Plano completo: **`C:\Users\ruben\.claude\plans\synchronous-sauteeing-spindle.md`**
   (a seção **## Progresso** no topo diz o que já está commitado e o que falta).

## REGRAS INEGOCIÁVEIS
- **Agnosticismo (invariante):** nada hardcoda linguagem nem arquitetura. Floor textual
  universal; tree-sitter é precisão plugável; o configurável vai p/ `mustard.json`.
  Arquitetura (Layered/Hexagonal/Clean/DDD/SOLID) é **detectada**, não assumida.
- **i18n:** `mustard.json {language, tone}` na raiz. `language` SÓ na spec voltada ao
  usuário; `tone` na spec-usuário + respostas ao usuário; **todo o resto em INGLÊS** (código,
  artefatos internos, headings, marcadores). Chaves canônicas EN.
- **`.events`/`.blob` por spec PRESERVADOS** (fonte do dashboard). meta.json fonte única é
  só p/ estado de lifecycle, NÃO p/ eventos. Todo dado novo é emitido como evento.
- **Telemetria:** cada migração LLM→Rust emite o *savings* via `core::economy` (baseline
  LLM − custo Rust). Critério de aceite por fase.
- Sem facade/wrapper delegador; imports módulo-qualificados (`use crate::x; x::fn()`);
  **ao tocar um arquivo: rename à convenção + dedup + SOLID**.
- `unsafe_code` forbid; `unwrap_used` deny (exceto `#[cfg(test)]`).
- **`rtk` SEMPRE** (build/test/git/gh, inclusive em cadeias `&&`).

## EXECUÇÃO (subagentes em SÉRIE)
- Implemente via **subagentes, UM por passo coeso, EM SÉRIE**: o build do workspace colide
  entre subagentes concorrentes e a árvore Rust precisa compilar entre commits. (Mapear/ler
  pode ser paralelo; editar+buildar é serial.)
- Por passo: despache 1 subagente com prompt **auto-suficiente** (contexto + tarefa + regras
  + gate + "NÃO commite"). Ao retornar, **verifique empiricamente** (revise o `--stat`/diff;
  rode o gate), e se verde **commite 1 commit coeso**; senão mande corrigir.
- **GATE por commit:** `rtk cargo build -p mustard-core` && `rtk cargo build -p mustard-rt
  --bin mustard-rt` && `rtk cargo test`. **Nunca commite com gate vermelho.** Falhas
  **AMBIENTAIS que NÃO contam:** `gate_regression_check::wave_7_review_w6_fixture`,
  `touched_functions::ac_a_15…`, `spec_invariants` (fixtures `.claude/spec` ausentes), e
  `io::events::reader::bench_stream_10k` (floco sob carga).
- Commit termina com: `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.
- Ao concluir CADA fase: atualize `project-mustard-functional-refactor` (## PROGRESSO) + o
  plano + a task.
- Se um passo travar (gate não fica verde após tentar): **pule p/ o próximo passo
  independente, deixando a árvore no último commit verde**, e registre o bloqueio na memória.
  **Nunca deixe a árvore vermelha.**
- NÃO commite os arquivos pré-existentes do working tree não relacionados (CLAUDE.md,
  NEXT-SESSION*.md, REFACTOR-AUDIT.md, `.claude/settings*`, `src/dashboard/vite.config.*`) —
  faça `git add` por caminho explícito.

## BACKLOG (ordem; detalhe de cada item no plano)

**Resto da F0** (acopla ao scan):
- **F0-e** agnosticismo dos gates: `scan/project_conventions.rs::primary_ext_for_stack`,
  `scan/file_utils.rs::SOURCE_EXTENSIONS`, `wave/wave_lib.rs::detect_role` → fallback
  agnóstico (nada zera p/ stack desconhecido) + override `mustard.json` (extensões,
  role_patterns).
- **F0-b** aquisição WASM: feature `wasm`/`wasmtime` **atrás de feature flag** (não pesar o
  build padrão) + `acquire_wasm(lang)` (baixa `.wasm` pinado de fonte versionada p/ cache
  `~/.mustard/grammars/{lang}/{version}/` + manifest version+sha256; valida ABI ≤ 0.26);
  integra no `GrammarLoader`/`TreeSitterParser` (WasmStore por parser; `language.is_wasm()`);
  fail-open → floor textual. Testes sem rede (`#[ignore]` no teste de rede).

**F1** Scan/entity-registry: wire `ast::EntityExtractor` no `InterpretedScanner`/
`sync_entity_registry` (entities/enums/campos/edges; popular `properties/decorators/table_name`;
conectar `pluralize`; **remover** o `entity_extractor.rs` duplicado do scan); `patternsOverlay`
determinístico; **detecção de arquitetura** (via `detect_framework_signals` + direção do
import-graph → campo `architecture`); `interpret::call_model` → fallback opt-in default-OFF +
emite savings; unificar os 2 walks de subprojeto.

**F2** Grafo/wirelinks: popular `.claude/graph` (nós+edges) via extração da F1; unificar os 2
resolvers `[[...]]` (id+filename); dedup scanners; separar `build_index` do write de
`index.md`; ativar `infer_applied_edges`/`merge_edges`; gate pré-write de `[[id]]`;
marcadores internos → EN.

**F3** Skills híbridos + injeção de contexto + agentes: `scan-skill-render` (Rust gera corpo
+examples+`appliesTo`; 1 call LLM só descrição; cap qtd/tamanho; top-K; bug case `applies_to`);
injetar `{{clustersBlock}}`/`{{samplesBlock}}`; preencher `{entity_info}`/`{reference_files}`/
`{context_extras}`; memory-match via Aho-Corasick + unificar cópias; agentes `.claude/agents`
ricos + dispatch via `subagent_type` nativo + corrigir `build_role_block`.

**F4** Orquestrador/controle/conclusão: `dispatch-plan` (Rust ordena waves; LLM relay);
meta.json fonte única (remover Stage/Outcome dos headers; aposentar `spec_status_backfill`;
remover writes `.pipeline-states`) com testes de leitura legada ANTES de remover headers;
conclusão determinística (auto-emit `wave.complete`; `close_orchestrate` encadeia;
`epic_fold` via NDJSON; `verify-emit` auto); limite hard de specs ativos; convergir os 2
renderizadores de wave-plan.md; auto-abertura por tipo (re-wave/wave/epic-fold automáticos;
tf/followup semi via payload estruturado); refatorar `resume_bootstrap` (SRP).

**F5** Lifecycle + i18n: `scope_decompose` com sinais do scan/registry; `prd_build::entity_present`
lookup exato; `bugfix_cache` rootCauseHash determinístico; `/task` via `agent-prompt-render`;
PRD_SECTIONS chaves EN; `language`+`tone` só na spec-facing (`mustard.json`); AC command e VCS
do `mustard.json` (fim de `rtk cargo build`/`git` hardcoded); artefatos internos → EN.

## AO TERMINAR
Atualize `project-mustard-functional-refactor` + o plano (## Progresso) com tudo feito, deixe
a árvore verde, e dê um resumo dos commits por fase.
