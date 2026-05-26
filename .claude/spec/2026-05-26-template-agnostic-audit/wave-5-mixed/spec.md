# wave-5-mixed — Refator i18n: split SupportedLocale vs UserLocale

### Parent: [[2026-05-26-template-agnostic-audit]]
### Stage: Plan
### Outcome: Active
### Flags:

## Resumo

Hoje `Locale` em `packages/core/src/i18n.rs` faz dois trabalhos misturados num único tipo:

1. **Catálogo de strings traduzíveis** — `translate(key, lang)` e `apply_tone(text, tone)` precisam que `lang` tenha tradução cadastrada. Hoje o catálogo do Mustard cobre apenas `pt-BR` e `en-US`. Adicionar nova exige criar strings junto.
2. **Idioma da spec/projeto** — `mustard.json#specLang` define em que idioma o user quer escrever specs. Conceitualmente aceita qualquer BCP-47 válido (`fr-FR`, `de-DE`, `en-GB`, ...), o user é livre. Mustard não precisa ter banner em FR pro user escrever spec em FR.

Hoje os dois conceitos compartilham o enum `Locale` (`PtBr | EnUs`), e `Locale::from_str("fr-FR")` retorna `LocaleError::Unknown` — bloqueia o user que queria specLang francês.

Esta wave separa os tipos sem mudar comportamento user-visível ainda.

## Network

- Parent: [[2026-05-26-template-agnostic-audit]]
- Depends on: [[wave-2-cli]] (CLAUDE.md meta-agnóstico documenta política de mustard.json)
- Blocks: [[wave-7-mixed]] (sweep + tone wire + audit precisa dos tipos corretos)
- Blocks: [[wave-6-mixed]] (recipe death pode tocar tests que dependem dos tipos)

## Modelo alvo

```rust
// Catálogo fechado — quais idiomas Mustard tem strings traduzidas.
// Adicionar uma nova exige criar entries em translate() + tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SupportedLocale {
    #[default]
    PtBr,
    EnUs,
}

// Aberto — qualquer BCP-47 válido sintaticamente (xx-YY).
// Não restringe valores; valida só shape ("pt-BR" ok, "pt" rejeita, "fr-FR" ok,
// "fooBar" rejeita).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UserLocale(String);

impl UserLocale {
    pub fn new(s: &str) -> Result<Self, UserLocaleError> { /* valida xx-YY */ }
    pub fn as_str(&self) -> &str { &self.0 }
    pub fn to_supported(&self) -> Option<SupportedLocale> { /* match strict por valor */ }
}

// translate / apply_tone só aceitam SupportedLocale — impossível pedir tradução
// pra idioma sem catálogo.
pub fn translate(key: &str, lang: SupportedLocale) -> &'static str { ... }
pub fn apply_tone(text: &str, tone: Tone) -> String { ... }

// I18n para banner rendering — Mustard só renderiza no que tem catálogo.
pub struct I18n {
    pub lang: SupportedLocale,
    pub tone: Tone,
}
```

Callsites decidem qual tipo cabe:

| Callsite | Tipo |
|---|---|
| `mustard.json#specLang` parsing | `UserLocale` |
| Header `### Lang:` em spec.md | `UserLocale` |
| Banner do `mustard-rt` | `SupportedLocale` (de `user.to_supported().unwrap_or_default()`) |
| Catálogo i18n (translate/apply_tone) | `SupportedLocale` (obrigatório por assinatura) |
| Reader SQLite — campo `lang` em events | `UserLocale` (gravado verbatim) |
| Comparação interna `if lang == "pt-BR"` | `SupportedLocale` (após conversão) |

## Arquivos

### Core (refator de tipos)
- `packages/core/src/i18n.rs` (MODIFY pesado — rename + add + tests)
- `packages/core/src/lib.rs` (MODIFY — re-exports)
- `packages/core/src/meta.rs` (MODIFY — normalise_lang já lida com BCP-47; ajustar para usar UserLocale; manter compat de leitura)
- `packages/core/src/spec/contract.rs` (MODIFY — assinaturas que tinham Locale escolhem entre os 2 novos)
- `packages/core/src/reader/sqlite.rs` (MODIFY — payload `lang` vira `UserLocale`; reader tolerante)
- `packages/core/src/projection/card.rs` (MODIFY)
- `packages/core/src/model/event.rs` (MODIFY — docstring)
- `packages/core/src/model/view/spec.rs` (MODIFY — `lang` field type)
- `packages/core/src/spec/mod.rs` (MODIFY — header parsing)

### Tests
- `packages/core/tests/reader_contract.rs` (MODIFY — fixtures + asserts)
- Outros tests inline em `packages/core/src/**` (MODIFY)

### NÃO TOCAR nesta wave
- `apps/rt/src/**` — só na W7 (sweep + wire + audit)
- `apps/dashboard/**` — só na W7
- `apps/cli/templates/**` — só na W7
- `mustard.json` files — só na W7
- Specs históricas em `.claude/spec/*/spec.md` — não migrar

## Tarefas

### Core Agent
- [ ] Ler `packages/core/src/i18n.rs` inteiro (atual ~830 linhas)
- [ ] **Rename** `Locale` → `SupportedLocale` (rename via Edit replace_all no arquivo + ripple para consumidores em packages/core/src/)
- [ ] **Add** struct `UserLocale(String)` + `UserLocaleError` enum
- [ ] `UserLocale::new(s)`: valida shape BCP-47 sintático (regex `^[a-z]{2,3}-[A-Z]{2,4}$`); rejeita short forms (`pt`, `en`) com erro distinto
- [ ] `UserLocale::as_str(&self) -> &str`
- [ ] `UserLocale::to_supported(&self) -> Option<SupportedLocale>` (match estrito por valor)
- [ ] `impl FromStr for UserLocale`
- [ ] `impl fmt::Display for UserLocale`
- [ ] Tests novos: `user_locale_accepts_bcp47` (pt-BR, en-US, fr-FR, en-GB), `user_locale_rejects_short` (pt, en), `user_locale_rejects_malformed` (pt_BR underscore, ptbr no separator, fooBar), `to_supported_maps_known` (pt-BR→Some(PtBr), en-US→Some(EnUs), fr-FR→None)
- [ ] **Migrate** callsites internos em `packages/core/src/`: cada `Locale::from_str` decidir se vira `SupportedLocale::from_str` (banner path, catálogo) ou `UserLocale::new` (spec/event path)
- [ ] `I18n` struct: `lang` field continua `SupportedLocale` (banner rendering)
- [ ] `reader/sqlite.rs`: campo `lang` em payloads de event vira `UserLocale` (storage/projection); banner que precisa renderizar faz `user.to_supported().unwrap_or_default()`
- [ ] `meta.rs`: `normalise_lang` agora retorna `UserLocale` (não `String`); aceita short forms legados emitindo warning + normalizando para BCP-47
- [ ] `spec/contract.rs` e `spec/mod.rs`: parsing de `### Lang:` retorna `UserLocale`
- [ ] `cargo build -p mustard-core` verde
- [ ] `cargo test -p mustard-core` verde
- [ ] `cargo build` (workspace) verde — verifica que rt/cli/dashboard ainda compilam (vão usar os tipos novos via re-export, mas as comparações `== "pt"` ainda existem; W7 sweepa essas)

### Validação cross-crate
- [ ] Documentar (em comentário no `i18n.rs` ou em ADR rascunho) que callsites em `apps/rt/` e `apps/dashboard/` que comparam `lang == "pt"` continuam corretos por enquanto (compara contra `UserLocale::as_str()` que devolve o BCP-47 verbatim) — W7 sweepa essas comparações pra `"pt-BR"`

## Critérios de Aceitação

- [ ] AC-W5-1: `UserLocale::new("pt-BR")` ok; `UserLocale::new("fr-FR")` ok; `UserLocale::new("pt")` rejeita; `UserLocale::new("fooBar")` rejeita — Command: `cargo test -p mustard-core i18n::tests::user_locale`
- [ ] AC-W5-2: `UserLocale::new("fr-FR").unwrap().to_supported()` retorna `None` — Command: `cargo test -p mustard-core i18n::tests::to_supported_maps_known`
- [ ] AC-W5-3: `translate(key, supported)` continua funcionando com SupportedLocale (assinatura impossível chamar com UserLocale) — Command: `cargo test -p mustard-core i18n::tests::translate_uses_supported_locale`
- [ ] AC-W5-4: `cargo build` workspace passa (rt/cli/dashboard ainda compilam — comparações legadas ainda funcionam contra UserLocale::as_str()) — Command: `cargo build`
- [ ] AC-W5-5: `cargo test -p mustard-core` passa — Command: `cargo test -p mustard-core`

## Limites

- MODIFY pesado: `packages/core/src/i18n.rs` (rename + add UserLocale)
- MODIFY ripple: `packages/core/src/{lib,meta,spec/contract,spec/mod,reader/sqlite,projection/card,model/event,model/view/spec}.rs`
- MODIFY tests: `packages/core/tests/reader_contract.rs` + inline
- FORA: apps/rt/, apps/dashboard/, apps/cli/templates/, configs, especs históricas — tudo W7
