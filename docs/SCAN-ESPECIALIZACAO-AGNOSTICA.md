# Plano — Especialização tecnológica do `/scan` via pacote adquirido sob demanda

> **Status: PLANO. NÃO implementado.** Documento de design permanente.
> Invariante mestre: o mustard é **agnóstico** — não sabe a priori a linguagem/
> framework/arquitetura. Tudo específico é **adquirido sob demanda** (mesmo
> contrato dos grammars WASM) ou vem do `mustard.json`; **zero identificador de
> tecnologia hardcoded no core**. min-IA/max-Rust; IA só no `--enrich`.

## ✅ Decisão (2026-05-29) — caminho LOCAL-FIRST (menor atrito, melhores skills)

Aprovado: **ligar a pluggabilidade que já existe + tirar hardcodes, SEM construir a máquina
de aquisição remota ainda.** A aquisição (registry pinado, host, download, SHA, supply-chain)
é **distribuição** por cima de uma fundação quase idêntica — e que dá o ganho sozinha. Adiá-la
não custa arquitetura. Critérios de aceite em toda mudança: **SOLID · reúso · zero/quase-zero
hardcoded · agnóstico** (lógica do core genérica; específico vem de dado overridável, nunca em
`.rs`). Execução **por subagentes em série** (o build colide), gate verde por passo.

- **Increment 1 (em curso):** Fase 2 (accessor `architecture` + reúso no `guards_seed`) +
  ligar `detect_framework_signals` ao scan honrando override + gravar frameworks detectados em
  `_patterns.{stack}` (espelhando a fiação da arquitetura) e surfacá-los em guards.
- **Increment 2 (próximo, design próprio):** dirigir `cluster_discovery` por vocab/convenções
  (remover os gates `matches!(stack_id,…)`) + telemetria de savings por migração.
- **Futuro (só sob demanda de distribuir pacotes curados):** aquisição remota (§3.1–3.6).

## Contexto e confirmação empírica da fundação

A Fase 1 (desta sessão, commit `25b2173`) religou os geradores determinísticos ao
`/scan`: modo padrão zero-IA gera registry + `SKILL.md` por cluster + `stack.md` +
`guards.md`. O que falta é **especialização plugável** sem ferir o agnosticismo.

Confirmado no código (não re-investigar):

| Peça | Estado | Evidência |
|---|---|---|
| Aquisição WASM (contrato a espelhar) | pronto, feature `wasm-grammars` OFF | `core/domain/ast/wasm_acquire.rs`: `REGISTRY` pinado + `WASMS_PKG_VERSION`, download `ureq` (cap 16 MiB), SHA-256 (observado sempre gravado; pinado opcional), cache `~/.mustard/grammars/{lang}/{ver}/` + `manifest.json {version,sha256,source_url,abi}`, `read_valid_cache` revalida, **fail-open** (Option, nunca panic) |
| GrammarLoader 3 camadas | pronto | `ast/loader.rs`: in-crate → WASM sob demanda → floor textual |
| Vocab de arquitetura | **plugável por arquivo** | `vocabulary/architecture.rs::load(name, root)` lê `.claude/vocab/{name}.toml`, fallback ao `architecture_builtin.toml` embarcado; `classify_segment` (Aho + token-split) |
| Vocab de frameworks | plugável por arquivo, **mas com gap** | `vocabulary/frameworks.rs::load` lê `.claude/vocab/frameworks.toml`; **`detect_framework_signals()` usa `builtin()` só** — ignora override (gap a corrigir) |
| Motor Aho genérico | pronto, agnóstico | `vocabulary/aho.rs::KeyedAutomaton<K>` |
| **`architecture` no registry** | **JÁ LIGADO (premissa "fixo unknown" está desatualizada)** | `scan/mod.rs:307` chama `detect_subproject_architecture` no scan; `sync_entity_registry.rs:385-390` grava em `_patterns.{stack}.architecture` (só quando ≠ unknown, p/ byte-stability); honra `mustard.json#architecture` (`architecture.rs:195`) |
| `mustard.json` (`ProjectConfig`) | pronto p/ estender | `core/domain/config.rs`: tem `architecture`, `primaryExt`, `sourceExtensions`, `rolePatterns`, `#[serde(flatten)] extra` (chaves desconhecidas preservadas) |
| `core::economy::SavingsSource` | 6 variantes | `economy/model.rs:89`; receita exata p/ nova variante mapeada |

**Hardcodes de tecnologia que ferem o agnosticismo (alvos da Fase 3):**
- `cluster_discovery.rs`: gates de linguagem `matches!(stack_id, "typescript"|"python"|"java"|"kotlin")` (decorators), TS-only (base-class), TS/Python-only (fn-prefix); keywords de declaração `["class","function","def","interface","fun"]`.
- `scan/mod.rs:324` `STACK_SIGNALS` — lista **fechada** de stacks↔manifests, **sem override**.
- `project_conventions.rs` `primary_ext_for_stack` — fechado (mas há override `mustard.json#primaryExt`).
- vocabs embarcados (`*_builtin.toml`) — override existe por arquivo, mas não é adquirido.

---

## Fase 2 — Arquitetura: fechar o que falta (pequeno)

A detecção/escrita/override já estão feitos. Resta apenas:

1. **Accessor tipado** em `core/domain/entity_registry.rs`:
   ```rust
   pub fn architecture(&self, stack_id: &str) -> Option<&str>
   ```
   (hoje consumidores espetam `_patterns.{stack}.architecture` na mão).
2. **Refatorar `guards_seed.rs`** para usar o accessor em vez de `patterns().get(stack)…` (reúso/SRP).
3. **Verificação E2E** (já parcialmente validada nesta sessão): `_patterns.{stack}.architecture` populado; `guards.md` traz a regra de fronteira do estilo; override por `mustard.json#architecture`.

Custo baixo, risco baixo. Pode entrar no mesmo passe da Fase 3 (item 1 é pré-requisito de reúso).

---

## Fase 3 — Especialização por tecnologia como PACOTE ADQUIRIDO SOB DEMANDA

### 3.0 Dois tipos de pacote, perfis de risco distintos (ponto de atenção)

| Tipo | Conteúdo | Como é "executado" | Risco | Modelo |
|---|---|---|---|---|
| **Pacote de vocabulário** (novo) | dados declarativos: tokens de papéis, signals de framework, convenções (decorators/sufixos/prefixos) | **parseado** (Aho-Corasick/serde) — nunca executado | **baixo** (dado), mas *influencia* os docs gerados → pacote envenenado induz guard/skill enganoso | novo `vocab_acquire` |
| **Pacote de grammar** (já existe) | `.wasm` tree-sitter (ex.: SDL de GraphQL) | **executado** pelo `WasmStore` | **alto** (code-exec) | `wasm_acquire` (feature OFF, ABI, SHA) |

**Decisão:** compartilham o **mecanismo** (pin + SHA + cache `~/.mustard/` + manifest + fail-open), mas são **pacotes e decisões de confiança separados**. O pacote de vocabulário pode **referenciar** um grammar id (ex.: `graphql`), que é adquirido pela trilha WASM já existente — nunca embute o `.wasm`.

### 3.1 De onde baixar (a decisão de produto mais delicada — ponto de atenção)

Não existe "registry oficial de pacotes de vocabulário arquitetural". Portanto o
mustard **mantém um registry PINADO e CONTROLADO**, nunca um terceiro arbitrário:

- **Âncora de confiança = registry embarcado in-crate** (espelha o `REGISTRY` array do `wasm_acquire`): um `vocab_registry` que mapeia `tech-id → { version, source_url, sha256 }`, onde `source_url` aponta para um host **sob controle da Atiz/mustard** (GitHub Releases do próprio repo, ou CDN próprio). Bump de versão = release controlado e revisado.
- **Override por `mustard.json`**: organização pode apontar `registryUrl` próprio (self-host) e/ou pinar versão por tech, ou **opt-out** (cai no floor genérico).
- **Override local por arquivo** (já existe): `.claude/vocab/{name}.toml` continua valendo — pacote próprio sem rede.

Assim o supply-chain fica fechado: por padrão só baixa do registry pinado revisado; SHA verificado; conteúdo é **dado** (não código).

### 3.2 Formato do pacote de vocabulário

Bundle versionado (tarball ou JSON único) com `manifest.json {tech, version, sha256, formatVersion}` + os artefatos que **os loaders já sabem ler**:
- `frameworks.toml` → signals de framework/ORM/DI (alimenta `detect_framework_signals`).
- `architecture.toml` → tokens de papéis adicionais (ex.: GraphQL `resolvers`, `schema`).
- `conventions.json` → **as convenções específicas** que hoje são hardcoded: lista de decorators relevantes (`@Resolver`, `@Query`, `@Mutation`), sufixos (`Resolver`, `Input`, `Type`), prefixos de função, e se a tech tem sintaxe de decorator (substitui o `matches!(stack_id, …)`).
- `grammar.ref` (opcional) → id de grammar WASM a adquirir pela trilha existente (ex.: `graphql`).

O cache vive em `~/.mustard/vocab/{tech}/{version}/` com o mesmo `manifest.json {version, sha256, source_url, formatVersion}` do WASM.

### 3.3 Quando adquirir (gatilho — usando dados que o scan já lê)

No `sync-registry`, no passe por subprojeto, **depois** do `detect_stack` + leitura de manifests/deps (que `scan-structural` já faz): se um signal de tecnologia (uma **dependência**, um **manifesto** ou uma **extensão**) mapeia para um `tech-id` do `vocab_registry` que **ainda não está em cache** → adquire (fail-open). Idempotente (uma vez por tech+versão).

> **Exemplo GraphQL:** detectar `*.graphql`/`*.gql` OU dep `graphql`/`@nestjs/graphql`/`async-graphql` no manifesto → `tech-id = graphql` → adquire o pacote `graphql` (convenções `@Resolver/@Query/Resolver-suffix` + `grammar.ref = graphql` p/ a SDL).

### 3.4 Como o pacote em cache alimenta o pipeline (tudo ACIONADO no mesmo passo — sem órfão)

| Consumidor | Hoje | Com o pacote |
|---|---|---|
| `detect_framework_signals` | só `builtin()` (gap) | **builtin + pacote/override mesclados** — corrige o gap; e passa a ser **chamado no scan** (hoje só no gate de regressão — confirmar/ligar) |
| `cluster_discovery` (decorator/fn-prefix/base-class) | gate `matches!(stack_id,…)` + keywords hardcoded | extração **genérica fica no core**; *quais* decorators/sufixos importam vem do `conventions.json` do pacote → forma cluster `Resolver` etc. |
| `ArchitectureVocabulary` | builtin + `.claude/vocab` | builtin + pacote (mescla) → reconhece papéis da tech (ex.: `resolvers`) |
| skills/guards (`scan_skill_render`/`guards_seed`) | consomem clusters+arquitetura | **automaticamente** melhoram quando o pacote enriquece clusters/arquitetura |
| grammar (tree-sitter) | in-crate + WASM | `grammar.ref` dispara `wasm_acquire` da SDL → AST da tech |

> **Ponto cego do GraphQL (tipo↔resolver):** uma função resolver mapeia para um
> tipo da SDL. Com a SDL grammar (via `grammar.ref`) + a convenção
> `Resolver`/`@Resolver`→tipo do `conventions.json`, o `cluster_discovery` forma o
> cluster `Resolver` e a aresta tipo↔resolver pode ser emitida — hoje invisível.

### 3.5 Override / opt-out (`mustard.json`)

Estender `ProjectConfig` (chave nova, `#[serde(flatten)] extra` já preserva desconhecidas):
```jsonc
"specialization": {
  "registryUrl": "https://…",          // self-host do registry pinado (opcional)
  "packages": [{ "tech": "graphql", "version": "1.2.0" }],  // pin por tech
  "optOut": false                       // true → sem aquisição, floor genérico
}
```
`.claude/vocab/{name}.toml` local continua como override sem-rede.

### 3.6 Telemetria de savings (por migração)

Nova variante `SavingsSource::ScanSpecializationVocab` (receita exata: `model.rs` enum + `as_str`/`from_str_opt`; `writer.rs::savings_suffix`; fixture de regressão). Emite quando o pacote permite ao Rust gerar um skill/guard que, sem ele, exigiria o `--enrich` (IA). (A variante `ScanSkillRender`, do meu Phase 3 menor oferecido, entra junto — cada determinização LLM→Rust emite seu savings projetável no dashboard.)

---

## Feature-gating: vocabulário ≠ grammar

- **Grammar WASM**: mantém `wasm-grammars` OFF por padrão (puxa `wasmtime`, pesado).
- **Vocabulário**: aquisição é leve (`ureq` + serde, sem `wasmtime`). **Não** justifica feature de compilação que a desligue por padrão (senão nunca roda). Proposta: aquisição de vocab **sempre disponível**, mas **gated em runtime** pelo `mustard.json` (`optOut`) e pelo registry pinado — rede só quando há tech detectada sem cache, sempre fail-open. (Decidir na implementação se um `vocab-acquire` cargo-feature ON-por-padrão é desejável para builds 100% offline-only.)

## Riscos e mitigações

- **Segurança do download** (ponto de atenção): registry **pinado e controlado** (in-crate), nunca terceiro; SHA-256 verificado; vocab é **dado declarativo** (parseado, não executado). Pacote envenenado só induz docs enganosos → mitigado por pin+SHA+revisão de release. Grammar (`.wasm`) executável mantém o modelo de alto-risco existente (feature OFF, ABI, SHA).
- **byte-stability do registry v4**: clusters mudam quando o pacote entra → incluir `tech+version` no hash do `.cluster-cache.json` (já há tunables no hash) p/ invalidar determinístico; ordenação preservada.
- **format/ABI do pacote de vocab**: campo `formatVersion` no manifest; rejeitar incompatível (fail-open → floor).
- **Offline / indisponível / SHA-mismatch**: fail-open → vocab builtin + floor textual; scan completa com clusters genéricos, nunca quebra.
- **Não repetir o erro original**: cada consumidor (framework-signals, cluster_discovery, arquitetura, skills, guards) é **ligado no mesmo commit** em que a aquisição é escrita; PR não fecha sem o consumidor acionado + evento de savings.

## Verificação E2E (por fase, com `claude` fora do PATH)

- **Fase 2**: registry tem `architecture`; accessor o devolve; `guards.md` traz a fronteira do estilo; override `mustard.json#architecture` respeitado.
- **Fase 3 — online, tech não-cacheada**: projeto-alvo com GraphQL (sem cache) → scan detecta dep/ext → adquire pacote `graphql` (pin+SHA) → `cluster_discovery` forma cluster `Resolver` → `SKILL.md`/`guards.md` refletem a convenção → cache em `~/.mustard/vocab/graphql/{ver}/`; **0 chamadas claude**; evento de savings emitido.
- **Fase 3 — offline / SHA-mismatch / opt-out**: aquisição falha/recusada → **floor genérico** (builtin + textual) → scan completa com clusters genéricos, sem quebrar.
- **Idempotência**: 2ª rodada não re-baixa (cache hit por `tech+version`); skills/guards byte-idênticos.

## Arquivos críticos a tocar

**Fase 2:** `core/domain/entity_registry.rs` (accessor `architecture`), `apps/rt/.../scan/guards_seed.rs` (usar accessor).

**Fase 3:**
- **novo** `core/domain/vocabulary/vocab_acquire.rs` (espelha `wasm_acquire`: registry pinado in-crate, download+SHA, cache `~/.mustard/vocab/`, manifest, fail-open).
- `core/domain/vocabulary/frameworks.rs` (`detect_framework_signals` mescla builtin+adquirido; **confirmar/ligar a chamada no scan**).
- `core/domain/vocabulary/architecture.rs` (mesclar vocab adquirido).
- `apps/rt/.../scan/cluster_discovery.rs` (gates de linguagem e convenções vindo do pacote, não hardcoded; extração genérica permanece).
- `apps/rt/.../scan/mod.rs` (gatilho de aquisição no passe por subprojeto; extensibilidade do `STACK_SIGNALS` via pacote/`mustard.json`).
- `core/domain/config.rs` (`SpecializationConfig`).
- `core/domain/economy/{model,writer,reader}.rs` (+ fixture) — `ScanSpecializationVocab` (e `ScanSkillRender`).
- `core/domain/ast/wasm_acquire.rs` (`REGISTRY` ganha entradas referenciáveis por `grammar.ref`, ex.: `graphql`).

## Ordem de execução sugerida

1. **Fase 2** (accessor + guards_seed) — destrava reúso, baixo risco.
2. **Fase 3a** — `vocab_acquire` + `SpecializationConfig` + registry pinado vazio/mínimo + telemetria; **sem** nenhuma tech ainda (só o mecanismo, fail-open testado offline).
3. **Fase 3b** — ligar os consumidores (frameworks merge, cluster_discovery dirigido por convenções, arquitetura merge), cada um com seu E2E + savings.
4. **Fase 3c** — primeiro pacote real (GraphQL) publicado no registry pinado + `grammar.ref` SDL; E2E completo online/offline.
