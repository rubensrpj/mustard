# Configuração de idioma e tom do Mustard

### DependsOn: 2026-05-24-meta-sidecar

## PRD

## Contexto

Hoje, quando o Mustard fala com você — seja num aviso do terminal, num banner do dashboard ou numa descrição de skill — o texto sai como quem escreveu pensou. Dev escreveu pra dev. Aparecem siglas sem explicação (AC = critério de aceitação, QA = fase de testes, Wave = onda do pipeline, RTK = ferramenta interna que economiza tokens), frases técnicas no meio do caminho, e a única configuração de idioma que existe hoje só vale pra parte narrativa de uma spec.

O resultado é direto: alguém que não conhece o código do Mustard trava nos textos. Mesmo um dev sênior, lendo um banner de erro, fica em dúvida sobre o que fazer em seguida.

Esta spec resolve isso. Cria duas opções no `mustard.json` de cada projeto: **idioma** (pt-BR ou en-US) e **tom** (didático, técnico ou caveman). Todo lugar onde o Mustard escreve algo pra um humano ler vai passar a respeitar essas duas escolhas.

Esta spec **depende** da `2026-05-24-meta-sidecar`. Aquela move os metadados de spec dos headers `### X:` pra um `meta.json` lateral, o que elimina o risco de o `tone` quebrar o parser. Esta aqui só pode entrar em `Execute` depois daquela fechar.

## Usuários/Stakeholders

- Pessoa que roda `mustard init` num projeto novo e nunca leu o código fonte.
- Dev acompanhando os avisos dos hooks no terminal durante o pipeline.
- Quem abre o dashboard para ver o estado dos projetos.
- Equipe não-técnica que precisa ler uma spec para entender o que vai acontecer.

## Métrica de sucesso

Quando alguém abrir um banner do `mustard-rt` num projeto configurado com idioma pt-BR e tom didático, vai entender em uma leitura: o que aconteceu, por que aconteceu, e o que precisa fazer agora.

## Não-Objetivos

- Traduzir código, comentários, identificadores ou caminhos de arquivo. Tudo fica em inglês.
- Mexer no idioma visual do dashboard (sidebar, menus, botões). Essa configuração já existe no Preferences global e continua separada — Preferences controla a aparência da interface; esta spec controla o conteúdo gerado para cada projeto.
- Traduzir os prompts internos que o orquestrador envia para subagentes. Esses textos são para a IA ler, não para humano.
- Suportar pt-PT, en-GB ou outras variantes regionais. Nesta primeira versão, só pt-BR e en-US.
- Criar um tom intermediário "direto". As três opções (didático, técnico, caveman) já cobrem o espectro.
- **Aplicar `tone` em estruturas parseáveis.** O tom só transforma prosa narrativa, banners, labels do dashboard e descrições de skill. Nunca toca em: headings de seção (`## Contexto`, `## Plano`, etc.), AC IDs (`AC-1`, `AC-2`...), metadados em `meta.json`, valores enumerados de stage/outcome/phase/scope, caminhos de arquivo, comandos shell em blocos de código, nomes de skill/recipe/entidade.

## Critérios de Aceitação

- [ ] **AC-1.** Quando rodo `mustard init` num projeto novo, o `mustard.json` é criado com `lang: "pt-BR"` e `tone: "didactic"` sem me perguntar nada. Command: `node -e "const fs=require('fs'); const m=JSON.parse(fs.readFileSync('apps/cli/templates/mustard.json','utf8')); if(m.lang!=='pt-BR'||m.tone!=='didactic') process.exit(1)"`

- [ ] **AC-2.** Existe um módulo central em `mustard-core` (pacote compartilhado por CLI e RT) que lê `lang` e `tone` do `mustard.json` e expõe pros consumidores. Command: `node -e "const fs=require('fs'); const txt=fs.readFileSync('packages/core/src/i18n.rs','utf8'); if(!/pub\s+fn\s+lang/.test(txt)||!/pub\s+fn\s+tone/.test(txt)) process.exit(1)"`

- [ ] **AC-3.** Quando troco `lang` para `"en-US"` no `mustard.json` e rodo qualquer comando do Mustard depois, os banners do `mustard-rt` E os outputs dos comandos do `mustard-cli` saem em inglês. Command: `node -e "const fs=require('fs'); const t=fs.readFileSync('packages/core/src/i18n.rs','utf8'); if(!t.includes('en-US')||!t.includes('pt-BR')) process.exit(1)"`

- [ ] **AC-4.** Quando troco `tone` para `"caveman"`, os mesmos banners ficam ultra-curtos, sem artigo, sem "claro!", sem frases de cortesia. Command: `node -e "const fs=require('fs'); const t=fs.readFileSync('packages/core/src/i18n.rs','utf8'); if(!/Tone::Caveman/.test(t)) process.exit(1)"`

- [ ] **AC-5.** No dashboard, abrindo a página Settings com um projeto selecionado, vejo dois seletores: idioma (pt-BR | en-US) e tom (didático | técnico | caveman). Mudar qualquer um grava no `mustard.json` daquele projeto. Command: `node -e "const fs=require('fs'); const t=fs.readFileSync('apps/dashboard/src/pages/Settings.tsx','utf8'); if(!t.includes('lang')||!t.includes('tone')||!t.includes('caveman')) process.exit(1)"`

- [ ] **AC-6.** A página Preferences continua existindo e segue controlando só os textos visuais da interface. Trocar lá não muda o conteúdo dos banners do `mustard-rt` nem o conteúdo das specs. Command: `node -e "const fs=require('fs'); const t=fs.readFileSync('apps/dashboard/src/pages/Preferences.tsx','utf8'); if(t.includes('mustard.json')||/tone\s*[:=]/.test(t)) process.exit(1)"`

- [ ] **AC-7.** Se um projeto antigo tem `specLang: "pt"` no `mustard.json`, esse campo vira `lang: "pt-BR"` automaticamente na primeira execução do `mustard-rt` — sem pedir confirmação, sem perder a escolha original. Command: `node -e "const fs=require('fs'); const t=fs.readFileSync('packages/core/src/i18n.rs','utf8'); if(!/specLang/.test(t)||!/migrate/.test(t)) process.exit(1)"`

- [ ] **AC-8.** Os componentes do dashboard que mostram número de onda usam o mesmo formato — chega de `W3` num componente e `onda 3` no irmão. Command: `node -e "const fs=require('fs'); const pcl=fs.readFileSync('apps/dashboard/src/features/workspace/LivePipelineCard/index.tsx','utf8'); if(/[\`'\"]W\d/.test(pcl)) process.exit(1)"`

- [ ] **AC-9.** Quando uma spec nova é criada num projeto com `lang: "pt-BR"`, o slug do diretório sai em português (com acentos normalizados). Quando o projeto está com `lang: "en-US"`, o slug sai em inglês. Command: `node -e "const fs=require('fs'); const t=fs.readFileSync('apps/rt/src/run/spec_slug.rs','utf8'); if(!/pt-BR/.test(t)||!/en-US/.test(t)||!/slugify/.test(t)) process.exit(1)"`

- [ ] **AC-10.** O `tone` nunca afeta estruturas parseáveis. Verifica criando uma spec com `tone: "caveman"` ativo e confirmando que o `meta.json` (de B) tem `phase: "PLAN"` literal, e o `.md` ainda parseia. Command: `node -e "const fs=require('fs'); const t=fs.readFileSync('packages/core/src/i18n.rs','utf8'); if(!/fn apply_tone/.test(t)||!/preserve_structured/.test(t)) process.exit(1)"`

## Plano

| Onda | Papel | O que faz | Depende |
|---|---|---|---|
| 1 | general | Adicionar `lang` e `tone` ao template do `mustard.json`. Criar módulo `i18n` em `packages/core` (compartilhado entre CLI e RT) — schema, leitor, tom-transform, gerador de slug por idioma, migração `specLang`→`lang`. Injetar no `SessionStart`. | meta-sidecar (B) fechada |
| 2 | general | Refatorar banners e mensagens pra usar `i18n` do `mustard-core` em duas frentes: (a) `apps/rt/src/{hooks,run,mcp,report,dispatch}.rs` e (b) `apps/cli/src/commands/**/*.rs` (init, update, config, add, review, git_flow, install_nerd_font). | Onda 1 |
| 3 | ui | Página Settings ganha dois seletores. Preferences ganha nota de escopo. Componentes do workspace consomem `lang` do projeto. Padronizar `W3` vs `onda 3`. Descrições de skill bilíngues. | Onda 1 |

Ondas 2 e 3 podem rodar em paralelo depois da Onda 1.

## Cobertura

| Preocupação levantada na conversa | Onde resolve |
|---|---|
| Config por projeto, não global. | AC-1, AC-5, AC-6 — Onda 1 + 3. |
| Settings = per-projeto; Preferences = só visual do dashboard. | AC-5, AC-6 — Onda 3. |
| Três tons, incluindo caveman. | AC-4 — Onda 1 + 2. |
| Defaults pt-BR + didático, sem perguntar em `mustard init`. | AC-1 — Onda 1. |
| Migração `specLang` → `lang`. | AC-7 — Onda 1. |
| Padronizar `W3` vs `onda 3`. | AC-8 — Onda 3. |
| Spec em pt-BR didática, sem jargão. | Esta própria spec. |
| ACs como frases observáveis. | Todos os AC começam com "Quando..." ou descrevem comportamento. |
| Token economy preservada. | Banners curtos na Onda 2, labels diretas na Onda 3. |
| Caveman como tom oficial, não skill paralela. | AC-4 — Onda 1 + 2. |
| Tone não pode quebrar parser nem estruturas. | AC-10 + Não-Objetivos. Onda 1 implementa a regra `preserve_structured`. |
| CLI inteiro estava fora do escopo (82 ocorrências de println!). | AC-3 — Onda 2 expandida cobre `apps/cli/src/commands/**`. |
| Slug das specs deve respeitar lang. | AC-9 — Onda 1 inclui gerador de slug por idioma. |
| Dependência cruzada com meta-sidecar (B). | Declarada como `### DependsOn:` no header e na coluna "Depende" do Plano. |

## Limites

Arquivos que esta spec encosta:

- `apps/cli/templates/mustard.json` — adiciona `lang` e `tone`.
- `apps/cli/src/commands/init.rs`, `update.rs` — defaults e preservação.
- `apps/cli/src/commands/{add,review,git_flow,install_nerd_font,config}.rs` — outputs consomem `i18n`.
- `packages/core/src/i18n.rs` (novo) — schema, leitor, tom-transform, slug.
- `apps/rt/src/hooks/session_start.rs` — injeta `lang`+`tone` no contexto.
- `apps/rt/src/hooks/**/*.rs` — banners consomem `i18n`.
- `apps/rt/src/run/**/*.rs` — outputs consomem `i18n`.
- `apps/rt/src/mcp/**/*.rs`, `apps/rt/src/report/**/*.rs`, `apps/rt/src/dispatch.rs` — outputs consomem `i18n`.
- `apps/rt/src/run/spec_slug.rs` (novo ou ampliado) — gerador respeita lang.
- `apps/dashboard/src/pages/Settings.tsx` — dois seletores.
- `apps/dashboard/src/pages/Preferences.tsx` — nota informativa.
- `apps/dashboard/src/features/workspace/{LivePipelineCard,AggregateOverview,SpecTrackRow}/index.tsx` — consomem `lang` do projeto.
- `apps/cli/templates/commands/mustard/**/SKILL.md` — frontmatter bilíngue.

Fora de limites:

- `apps/dashboard/src/pages/Preferences.tsx` (lógica) — só a nota é adicionada; toda lógica do toggle global do dashboard fica intocada.
- `apps/dashboard/src/i18n.ts` — estrutura existente do dashboard, sem mudança estrutural.
- Código-fonte, comentários, doc-comments, identificadores, paths — sempre EN.
- Tudo que `meta-sidecar` já resolve (parser, headers, schema) — herdado, não duplicado.
