---
id: spec.english-canonical-retrieval
---

# Recuperação em inglês canônico — o motor de busca passa a falar só inglês; português fica apenas na spec apresentada e na conversa

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria / Critério de Aceitação, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

O Mustard tem duas camadas que hoje misturam idiomas:

1. **A camada de recuperação** (o "motor de busca" do código): o `/scan` gera, para cada
   método, uma frase de *propósito* (`purpose`) no idioma do projeto (português, hoje), e a
   busca por intenção traduz a pergunta do usuário de português para inglês usando um glossário
   bilíngue (`pt-en.toml`) — o chamado *tier-4 lexicon* da escada de casamento de termos.
2. **A camada de apresentação**: a spec que o usuário lê/aprova, os avisos (banners), as
   perguntas — tudo no idioma configurado em `mustard.json` (`language`/`tone`).

Medimos (2026-06-25) que **propósito em inglês + pergunta traduzida para inglês recupera tão bem
ou melhor que a versão bilíngue** (8/10 no conceito central dos casos difíceis da sialia, contra
7/10 do português). A consequência: o glossário bilíngue e toda a maquinaria de "tier-4 lexicon"
viram peso morto — um inglês-contra-inglês resolve com os tiers que já existem (exato / dobra de
acento / radical), e a tradução da pergunta passa a ser responsabilidade de quem chama a busca
(o orquestrador ou o agente, que já são modelos de linguagem e emitem inglês direto).

**A política de língua que governa esta mudança** (decidida pelo usuário, registrada no roadmap):
o `mustard.json` (`language`/`tone`) rege **só duas coisas** — (1) a **narrativa da spec
apresentada** ao usuário e (2) a **forma de apresentar** (conversa, banners, perguntas,
explicação didática). **Todo o resto — absolutamente tudo — é inglês**: a camada de recuperação
(propósitos, perguntas, casamento de termos), o código, os logs, o esquema, e também os artefatos
de máquina (planos de onda, sub-specs, headings internos). *"O motor de busca é uma máquina: fala
inglês."* Isto reverte a regra de 2026-06-04 ("todo artefato gerado segue o idioma do
`mustard.json`"), que está morta para os artefatos de máquina.

Âncoras (do scan / da análise):
- apps/scan/src/matching.rs (tier-4 lexicon: `Ladder`, `Lexicon::bridges`, `parse_lexicon`)
- apps/scan/src/stemmers.rs (`LexiconSeed`, `project_lexicon`, seed `pt-en.toml`)
- apps/scan/src/digest.rs + apps/scan/src/purpose.rs (parâmetro `request_lang`, `Ladder::new`)
- apps/scan/src/main.rs (`--lang`, `request_lang()`)
- apps/rt/src/commands/enrich_purpose.rs (`resolve_lang`, campo `lang` da worklist)
- apps/rt/src/commands/lexicon_suggest.rs + lexicon_enrich.rs + lexicon_judge.rs (subsistema-ponte)
- apps/rt/src/commands/mod.rs (variantes `LexiconSuggest`/`LexiconEnrich`/`LexiconJudgeRender` + dispatch)
- apps/rt/src/commands/wave/wave_scaffold.rs (headings `## Tarefas`/`## Arquivos` lang-aware)
- packages/core/src/platform/i18n.rs (`gate.verdict.*`/`gate.signal.*` — máquina; `gate.askuser.*` — apresentação)
- apps/dashboard/src/lib/dashboard.ts + features/specs/{WaveMarkdownDrawer,SpecWavesTab} + components/page/Markdown (parseiam `## Tarefas`/`## Arquivos` em PT)
- apps/cli/templates/lexicons/pt-en.toml + apps/scan/lexicons/pt-en.toml + .claude/lexicons/pt-en.toml

Por que agora: a medição fechou a dúvida (inglês recupera igual ou melhor) e o glossário bilíngue
é dívida pura — mantê-lo só adiciona superfície de manutenção e contradiz o princípio de não
manter abstração por hipótese. E o dashboard **quebra** quando os headings virarem inglês (ele
parseia `## Tarefas`/`## Arquivos` literais), então a mudança precisa atravessar até a interface.

## Usuários/Stakeholders

- **Times em qualquer idioma** (o ganho principal): com a máquina 100% inglês, um único
  enriquecimento serve perguntas em qualquer língua — o Mustard fica poliglota de verdade
  (agnóstico de idioma de query), e só a fina camada de apresentação troca de idioma.
- **O mantenedor (Rubens)**: menos código para manter (um subsistema inteiro deletado), menos
  bugs de internacionalização espalhados por artefatos que nenhum humano lê de fato.
- **O usuário final do dashboard**: continua vendo a interface e a spec no idioma configurado;
  os planos de onda passam a aparecer em inglês, com a explicação em PT vindo do orquestrador.

## Métrica de sucesso

- **Recall preservado**: no benchmark público Medusa, `purposeRecall@5 ≥ 0.85` pela rota nova
  (propósito inglês + pergunta inglês), igual ou melhor que a baseline atual.
- **Superfície removida**: zero referências a `pt-en.toml` / `LexiconSuggest` / tier-4 lexicon
  no código após a mudança (grep limpo).
- **Dashboard íntegro**: os painéis de spec/onda renderizam corretamente headings em inglês
  (`## Tasks`/`## Files`) **e** os antigos em PT (compatibilidade com specs já no disco).
- **Build e testes verdes** em todo o workspace (Rust) e no dashboard (TypeScript).

## Não-Objetivos

- **Não** construir a camada de sinônimos em inglês (payout/paid, forecast/projects) — o resíduo
  de sinônimo fica aceito por ora (limite medido do Saleor 0.6@5); a camada EN embedding/WordNet
  é trabalho futuro, de outra forma.
- **Não** mexer na sialia (corpus privado, somente leitura) — a validação roda nos benchmarks
  públicos (Medusa/Saleor).
- **Não** resolver a latência da busca (re-parse do `grain.model.json` por chamada) — dívida
  arquitetural separada (scan-como-lib + índice em cache).
- **Não** mexer no seletor de idioma/tom das Configurações do dashboard — ele continua válido
  (rege a apresentação); no máximo o rótulo é esclarecido.
- **Não** remover o glossário de domínio voltado ao usuário (`glossary-*`) se ele servir à
  camada apresentada — avaliar caso a caso; o alvo da deleção é o **glossário-ponte PT→EN**.

## Critérios de Aceitação

- **AC-1** — Workspace Rust compila
  Command: `cargo build --workspace`

- **AC-2** — Subsistema-ponte PT→EN removido por completo (sem leitores residuais)
  Command: `grep -qrE "pt-en\.toml|LexiconSuggest|LexiconEnrich|LexiconJudgeRender|fn bridges" apps packages && exit 1 || exit 0`

- **AC-3** — Enrich emite diretriz de propósito em inglês (worklist não traz idioma do projeto)
  Command: `cargo test -p mustard-rt enrich_purpose`

- **AC-4** — Casamento de termos passa a ser inglês-intra-língua (escada sem tier-4 lexicon, testes verdes)
  Command: `cargo test -p mustard-scan matching`

- **AC-5** — Recall preservado na rota inglês (benchmark público Medusa)
  Command: `mustard-rt run recall-bench --labels benchmarks/medusa/labels-v2-distinct-files.ndjson --model benchmarks/medusa/grain.model.en.json`

- **AC-6** — Todos os testes Rust verdes
  Command: `cargo test --workspace`

- **AC-7** — Dashboard parseia headings em inglês e em PT (build + teste verdes)
  Command: `pnpm --filter @mustard/dashboard build`

<!-- PLAN -->

## Arquivos

**Onda 1 — núcleo de recuperação (apps/scan + packages/core):** tornar o propósito inglês
canônico e remover o tier-4 lexicon.
- apps/scan/src/matching.rs — remover tier-4 `lexicon` (`Lexicon`, `bridges`, `parse_lexicon`); `Ladder` sem par de idiomas (T1 exato / T2 dobra / T3 radical / T5 trigrama).
- apps/scan/src/stemmers.rs — remover `LexiconSeed`/`project_lexicon`; manter stemmer EN.
- apps/scan/src/digest.rs + purpose.rs — remover `request_lang` da assinatura/uso.
- apps/scan/src/main.rs — remover `--lang` e `request_lang()`.
- apps/scan/lexicons/pt-en.toml — deletar (seed).

**Onda 2 — comandos e geradores (apps/rt + templates):** deletar comandos-ponte, reverter
política i18n nos geradores de máquina, diretrizes de prompt em inglês.
- apps/rt/src/commands/{lexicon_suggest,lexicon_enrich,lexicon_judge}.rs — deletar.
- apps/rt/src/commands/mod.rs — remover variantes do enum + braços do dispatch + `pub mod`.
- apps/rt/src/commands/enrich_purpose.rs — `resolve_lang` → propósito sempre inglês; worklist sem idioma do projeto.
- apps/rt/src/commands/wave/wave_scaffold.rs — headings em inglês fixo (sem `effective_locale`/branch lang).
- packages/core/src/platform/i18n.rs — `gate.verdict.*`/`gate.signal.*` → inglês (máquina); `gate.askuser.*` e banners → mantêm config-lang.
- apps/cli/templates/lexicons/pt-en.toml — deletar (template).
- apps/cli/templates/commands/mustard/scan/SKILL.md + .claude/.../scan/SKILL.md — prompt de sumarização em inglês.
- apps/mcp/src/lib.rs — `find_by_intent`: contrato documenta termos de intenção em inglês.

**Onda 3 — dashboard (apps/dashboard):** parsear headings em inglês (aceitar ambos) + higiene.
- apps/dashboard/src/lib/dashboard.ts — aceitar `## Files`/`## Arquivos`.
- apps/dashboard/src/features/specs/{WaveMarkdownDrawer,SpecWavesTab}/index.tsx — `## Tasks`/`## Tarefas`, `## Files`/`## Arquivos`.
- apps/dashboard/src/components/page/Markdown/index.tsx — checklist `## Tasks`/`## Tarefas`.
- apps/dashboard/src/lib/i18n.ts — esclarecer rótulos; limpar chave órfã `nav.prd`.
- apps/dashboard/src/pages/Settings.tsx — (opcional) esclarecer escopo do seletor de idioma (rege apresentação).

**Limpeza:** .claude/lexicons/pt-en.toml (overlay runtime) e a spec aberta
`ranquear-candidatos-lexicon-enrich-por` ficam obsoletas — registrar/arquivar.

## Limites

IN: camada de recuperação (propósito/casamento/query) → inglês; deleção do subsistema-ponte
PT→EN; reversão da política i18n nos artefatos de máquina (planos de onda, sub-specs, headings);
parser do dashboard aceitando headings em inglês.

OUT: camada de sinônimos EN (futuro); latência da busca; seletor de idioma do dashboard;
glossário de domínio apresentado ao usuário; qualquer escrita na sialia.