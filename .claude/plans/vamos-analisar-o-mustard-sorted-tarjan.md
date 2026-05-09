# Mustard como plataforma de agentes corporativos

## Context

O Mustard hoje é um CLI Node.js que gera `.claude/` em projetos de código (prompts, skills, hooks, registry, pipelines). O user quer entender se dá pra viabilizá-lo como **plataforma de geração de agentes para empresas**, cobrindo: dev/eng, vertical específico (fintech/saúde), operacional não-código, e meta-agente (gera agente sob medida).

A pergunta certa não é "o Mustard consegue gerar X tipo de agente?". É: **qual posicionamento permite vender o que ele já faz, sem virar uma plataforma genérica que perde foco?** A resposta tem que respeitar três restrições que vi no exame do código:

1. Mustard é **arquiteturalmente agnóstico** (scanner plugin-based, hooks fail-open, sem deps externas, registry v4.0 SOLID, cluster discovery por padrões e não por techs). O acoplamento com "repo de código" está nos *exemplos* e *naming*, não no engine.
2. Mas **todo o IO assume filesystem de código** — `mustard init` espera achar arquivos pra escanear, hooks bloqueiam edits fora de zonas whitelist, pipelines presumem build/test runnables.
3. O dono é dev solo. **SaaS é distração**; consultoria escala mal; OSS+licença comercial é o sweet spot conhecido pra ferramentas dev.

## Tese central

**Mustard é "Terraform pro `.claude/`" — o standard layer corporativo do Claude Code.**

Empresas que já usam Claude Code têm um problema invisível: cada dev configura seu `.claude/` do jeito dele, prompts divergem, conhecimento não é versionado, registry não é compartilhado, hooks de compliance não existem. Mustard resolve isso *para times*, não pra projetos individuais. É essa a venda — e é o que o produto **já faz hoje**, só precisa ser posicionado.

A partir desse núcleo, as 4 categorias do user encaixam como camadas:

| Camada | Mustard hoje | Esforço pra viabilizar |
|--------|--------------|------------------------|
| **Dev/eng (já é)** | 100% | Empacotar e posicionar |
| **Vertical (fintech, saúde)** | ~70% | Skill-packs + registry pré-populado |
| **Meta-agente (descreva, gera)** | ~40% | Modo `mustard interview` |
| **Operacional não-código** | ~15% | Pivot maior — só após validar acima |

Vou tratar como **3 fases sequenciais**, cada uma destrava a próxima. Sem fase 1 funcionando, fase 2 não tem canal de venda.

## Decisor recomendado: CTO/Eng Manager de SaaS médio (10-50 devs)

Não vender pra CFO/COO no início. Claude Code ainda é percebido como ferramenta de dev — a venda flui natural com decisor técnico. Empresa-alvo:

- Já adotou ou está testando Claude Code em ≥3 devs
- Tem ≥2 repos/serviços (problema de inconsistência aparece)
- 10-50 devs (grande o suficiente pra dor, pequena pra decisão rápida)
- Setor com peso regulatório vira upsell natural pra fase 2 (fintech, saúde, jurídico)

Métrica de venda: **"reduza variância entre seus devs usando Claude Code; capture conhecimento da empresa em prompts versionáveis; bloqueie escapadas de PII/credentials por hook, não por code review"**.

## Fase 1 — Consolidar e posicionar (3-6 meses)

Objetivo: **vender Mustard como standard layer Claude Code para 5-10 empresas**, modelo OSS-core + consultoria de onboarding.

### Entregáveis técnicos (quase tudo já existe — só falta empacotar)

- `mustard init --org` — modo "shared registry": gera `.claude/` que aponta pra um registry+skills compartilhados (git submodule ou npm package privado). Permite a empresa ter "skills da empresa" reusáveis em N repos.
- `mustard sync` — comando novo, puxa atualizações do registry/skills compartilhados. Hoje o `update` reescreve infra do Mustard; precisa do dual: infra (Mustard upgrade) + org (skills da empresa).
- **Skill-pack template oficial**: documentar como uma empresa empacota seus próprios skills/hooks/registry como pacote redistribuível. Hoje é possível na prática mas não há doc.
- **Telemetria local opt-in** (já tem `metrics-tracker.js`): empacotar como dashboard simples (`mustard stats --html`) pra CTO ver adoção e economia de tokens.

### Entregáveis comerciais

- Landing page focada na dor real: *"Cada dev no seu time tem um `.claude/` diferente. Mustard padroniza."*
- Dois case studies (mesmo que sintéticos no início): "Time de 12 devs reduziu retrabalho em X%", "Empresa Y bloqueou Z vazamentos de PII via hook".
- Pacote de consultoria de 1-2 semanas: onboarding, criação de skill-pack interno, treinamento.

### Critério de saída

≥3 empresas pagantes (consultoria) OU ≥1000 instalações OSS de `mustard init`. Sem isso, não vale subir pra fase 2.

## Fase 2 — Verticais e meta-agente (6-12 meses, condicionado à fase 1)

Objetivo: **diferenciar Mustard de "qualquer um pode forkar templates" — vendendo skill-packs verticais comerciais e modo interview**.

### 2a. Skill-packs verticais (alavanca consultoria → produto)

Cada pack é um **bundle redistribuível**: skills + hooks + registry pré-populado + prompts especializados, instalável via `mustard install <pack>`. Começar com 2 e iterar com clientes:

- **fintech-pack**: registry pré-populado (Conta, Transação, KYC, Boleto, PIX), hooks que bloqueiam log de PII/PAN/CPF, prompts que rodam revisão LGPD/BACEN antes de aprovar PR.
- **saude-pack**: registry (Paciente, Prontuário, Consulta, Exame), hooks LGPD-saúde + CFM, prompt de revisão pra prontuários eletrônicos.

Por que funciona: a arquitetura **já é plugin-based** (`loadScanner`/`scanner-contract` em `templates/scripts/sync-registry.js`). Adicionar pack é "novo diretório `packs/<name>/`", não pivot. Modelo: licença anual por pack, OSS gratuito.

### 2b. `mustard interview` — meta-agente generativo

Modo conversacional novo: empresa descreve o que precisa em linguagem natural, Mustard entrevista (entidades, fluxos, critérios), gera `.claude/` customizado. Exemplo:

```
$ mustard interview
> Que tipo de agente você precisa criar?
< Um agente que revise contratos e bata com nossa política interna.
> Onde ficam suas políticas atuais?
< Em /docs/policies/ (markdown)
> Suas políticas mencionam quais entidades? (detectado: Contrato, Cláusula, Risco)
< Confirma. Adiciona "Aprovador".
> [...]
✓ Gerado .claude/ com 3 skills, 2 hooks, registry custom.
```

Implementação se apoia 100% no que existe — é só inverter o fluxo: hoje Mustard *escaneia* pra gerar registry, no modo interview ele *pergunta*. Reusa `sync-registry`/scanner-contract como output, não input.

### Critério de saída

≥1 pack vertical com ≥10 clientes pagantes. ≥3 cases de meta-agente públicos.

## Fase 3 — Operacional não-código (12+ meses, opcional)

**Só executar se fase 2 mostrar demanda real explícita.** Senão é distração — vira concorrente do Salesforce/Zapier e perde foco.

A boa notícia: a arquitetura **suporta isso por design**. O que falta:

- Scanner novo (`scanners/docs.js`) que entende markdown/PDF/CSV em vez de código (mesmo contrato `scan() → entities/patterns`).
- Hook `file-guard.js` precisa de modo "domínio-livre" (hoje whitelist é por extensão de código).
- Renomear no documento (não no código) "entity" → "domain object" pra não confundir adopters não-dev.

Mas a venda pra esse público (RH, atendimento, jurídico-ops) é **outra força de vendas** — não use o mesmo pitch da fase 1. Por isso é fase 3, não fase 1b.

## O que **não** fazer

- **Não construir SaaS no início**. Backend, billing, multi-tenancy, SOC2 = 12 meses de distração antes de validar mercado. Self-hosted OSS já entrega 100% do valor; SaaS é refinamento depois.
- **Não hardcodar verticais no engine**. Os packs são redistribuíveis e ficam em diretório próprio (`packs/`); o core continua agnóstico. Isso já é como sync-registry foi desenhado — manter.
- **Não virar low-code platform**. "Drag-and-drop pra criar agente" mata o diferencial: quem instala Mustard quer arquivos versionáveis em git, não UI.
- **Não acumular features novas competindo entre si**. Se fase 1 não bater critério de saída, *parar* e descobrir por quê — não acelerar pra fase 2 esperando que ela compense.

## Pontos críticos no codebase a confirmar antes da fase 1

1. `templates/scripts/sync-registry.js:26-29` — scanner-loader é mesmo plugin-based redistribuível? (sim pelo header SOLID, validar fluxo end-to-end)
2. `src/commands/init.ts` — qual o comportamento atual de `init` em diretório que **já tem** `.claude/` parcial? (precisa funcionar bem pra "init --org" reaproveitar parte)
3. `src/commands/update.ts:45-118` — `update` preserva o quê e sobrescreve o quê? Validar que skills da empresa não são apagadas no upgrade do Mustard.
4. `templates/hooks/file-guard.js` — quão acoplado a extensões de código? (relevante pra fase 3, não bloqueia fase 1)
5. Confirmar que `mustard.json` tem espaço pra apontar registry+skills compartilhados (campo `org` ou similar).

## Verificação por fase

**Fase 1**: 
- `mustard init --org` puxa registry remoto e gera `.claude/` consistente em ≥2 repos diferentes da mesma "org" simulada.
- Métricas via `metrics-tracker.js` aparecem agregáveis num dashboard local.
- Documentação de skill-pack permite alguém externo empacotar e reusar sem ler o código do Mustard.

**Fase 2**:
- `mustard install fintech-pack` em repo limpo gera registry com entidades+hooks corretos.
- `mustard interview` em diretório vazio com ≥3 markdowns gera `.claude/` que faz sentido sem edição manual.

**Fase 3**:
- `mustard init` em pasta sem código (só docs/) não quebra; gera registry de domínio.

## Resumo executivo

Mustard não precisa virar plataforma genérica. Precisa **se posicionar como standard layer corporativo do Claude Code** (fase 1), depois adicionar **skill-packs verticais + meta-agente generativo** como diferencial defensável (fase 2). Operacional não-código (fase 3) é viável arquiteturalmente mas é venda diferente — só puxar quando o canal técnico estiver provado. Modelo: OSS-core + licença comercial de packs + consultoria de onboarding. Decisor inicial: CTO/EM em SaaS médio. Não fazer SaaS, não hardcodar verticais, não virar low-code.
