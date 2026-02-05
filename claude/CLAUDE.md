# Mustard - Instruções para Claude

> Framework de agentes e pipeline para Claude Code.
> **Versão 2.4** - Auto-generated context, Memory MCP search in agents, improved CLI.

---

## 0. PIPELINE - VERIFICAR SEMPRE

> 🔍 **ANTES DE QUALQUER RESPOSTA:** Verificar se há pipeline ativo.

### Ao Iniciar Interação

```javascript
// SEMPRE executar no início
mcp__memory__search_nodes({ query: "pipeline phase" })
```

| Resultado | Ação |
|-----------|------|
| Nenhum pipeline | Análise livre, mas edições de código requerem /mtd-pipeline-feature ou /mtd-pipeline-bugfix |
| Pipeline em "explore" | Continuar exploração ou apresentar spec para aprovação |
| Pipeline em "implement" | Edições liberadas, seguir spec |

### Detecção Automática de Intenção

| Tipo de Solicitação | Pipeline Necessário? |
|---------------------|---------------------|
| "Como funciona X?" | NÃO - análise livre |
| "Onde está Y?" | NÃO - análise livre |
| "Explique Z" | NÃO - análise livre |
| "Adicione campo X" | SIM - /mtd-pipeline-feature |
| "Corrija erro Y" | SIM - /mtd-pipeline-bugfix |
| "Refatore Z" | SIM - /mtd-pipeline-feature |

---

## 1. ENFORCEMENT L0 - LEIA PRIMEIRO

> ⛔ **REGRA ABSOLUTA:** Claude principal NÃO implementa código. SEMPRE delega.

### Quando Receber Solicitação:

1. **IDENTIFICAR** tipo de tarefa
2. **SELECIONAR** agente/prompt apropriado
3. **DELEGAR** via Task tool com `subagent_type` nativo
4. **NUNCA** começar a escrever código diretamente

### Mapa de Delegação

| Solicitação | subagent_type | modelo | Prompt |
|-------------|---------------|--------|--------|
| Bug fix | `general-purpose` | opus | `prompts/mtd-pipeline-bugfix.md` |
| Nova feature | `general-purpose` | opus | `prompts/orchestrator.md` |
| Backend | `general-purpose` | opus | `prompts/backend.md` |
| Frontend | `general-purpose` | opus | `prompts/frontend.md` |
| Database | `general-purpose` | opus | `prompts/database.md` |
| QA/Revisão | `general-purpose` | opus | `prompts/review.md` |
| Explorar | `Explore` | haiku | (nativo) |
| Relatórios | `general-purpose` | sonnet | `prompts/report.md` |

### Auto-Verificação

**Antes de usar Write, Edit, ou Bash (para criar código):**

> Estou dentro de um agente (Task)?
> Se NÃO → PARE e delegue.

---

## 2. Tipos Nativos do Claude Code

O Claude Code aceita **apenas 4 tipos** de subagent_type:

| Tipo Nativo | Descrição | Uso no Mustard |
|-------------|-----------|----------------|
| `Explore` | Exploração rápida do codebase | Fase de análise |
| `Plan` | Planejamento de implementações | Specs complexas |
| `general-purpose` | Implementação, bug fixes, reviews | **PRINCIPAL** |
| `Bash` | Comandos de terminal | Git, builds |

### Como Funciona

Os "agentes" do Mustard são **prompts** que carregam instruções especializadas dentro de um `Task(general-purpose)`:

```javascript
// ANTES (não funciona)
Task({ subagent_type: "orchestrator", ... })  // ❌

// DEPOIS (funciona)
Task({
  subagent_type: "general-purpose",
  model: "opus",
  prompt: `
    # Você é o ORCHESTRATOR
    [conteúdo de prompts/orchestrator.md]

    # TAREFA
    ${descricao}
  `
})  // ✅
```

---

## 3. Agentes como Prompts

| Papel | subagent_type | Modelo | Arquivo de Prompt |
|-------|---------------|--------|-------------------|
| Orchestrator | `general-purpose` | opus | `prompts/orchestrator.md` |
| Explorer | `Explore` | haiku | (nativo - sem prompt) |
| Backend | `general-purpose` | opus | `prompts/backend.md` |
| Frontend | `general-purpose` | opus | `prompts/frontend.md` |
| Database | `general-purpose` | opus | `prompts/database.md` |
| Bugfix | `general-purpose` | opus | `prompts/mtd-pipeline-bugfix.md` |
| Review | `general-purpose` | opus | `prompts/review.md` |
| Report | `general-purpose` | sonnet | `prompts/report.md` |

---

## 4. Comandos Disponíveis

### Pipeline

| Comando | Descrição |
|---------|-----------|
| `/mtd-pipeline-feature <nome>` | Ponto único para features |
| `/mtd-pipeline-bugfix <erro>` | Ponto único para bugs |

### Pipeline (Novos)

| Comando | Descrição |
|---------|-----------|
| `/mtd-pipeline-approve` | Aprovar spec e liberar implementação |
| `/mtd-pipeline-complete` | Finalizar pipeline (após validação) |
| `/mtd-pipeline-resume` | Retomar pipeline ativo |

### Git

| Comando | Descrição |
|---------|-----------|
| `/mtd-git-commit` | Commit simples |
| `/mtd-git-push` | Commit e push |
| `/mtd-git-merge` | Merge para main |

### Validação

| Comando | Descrição |
|---------|-----------|
| `/mtd-validate-build` | Build + type-check |
| `/mtd-validate-status` | Status consolidado |
| `/mtd-scan-project` | Reconhecimento do projeto |

### Sync

| Comando | Descrição |
|---------|-----------|
| `/mtd-sync-registry` | Atualizar Entity Registry |
| `/sync-types` | Regenerar tipos TypeScript |
| `/mtd-sync-dependencies` | Instalar dependências |
| `/mtd-sync-context` | Carregar contexto do projeto |

### Relatórios

| Comando | Descrição |
|---------|-----------|
| `/mtd-report-daily` | Relatório diário de commits |
| `/mtd-report-weekly` | Relatório semanal consolidado |

---

## 5. Pipeline Único Obrigatório

```
┌─────────────────────────────────────────────────────────┐
│                    /mtd-pipeline-feature ou /mtd-pipeline-bugfix                   │
└───────────────────────────┬─────────────────────────────┘
                            ▼
┌─────────────────────────────────────────────────────────┐
│  FASE 0: CARREGAR CONTEXTO (auto, se > 24h)            │
│  Glob context/*.md, grepai patterns → memory MCP        │
└───────────────────────────┬─────────────────────────────┘
                            ▼
┌─────────────────────────────────────────────────────────┐
│  FASE 1: EXPLORAR                                       │
│  Task(Explore) → Analisa requisitos, mapeia arquivos    │
└───────────────────────────┬─────────────────────────────┘
                            ▼
┌─────────────────────────────────────────────────────────┐
│  FASE 2: SPEC                                           │
│  Salva plano em spec/active/{nome}/spec.md              │
│  Apresenta ao usuário para aprovação                    │
└───────────────────────────┬─────────────────────────────┘
                            │
              ┌─────────────┴─────────────┐
              ▼                           ▼
        [APROVADO]                   [ITERAR]
              │                           │
              ▼                    (volta FASE 1)
┌─────────────────────────────────────────────────────────┐
│  FASE 3: IMPLEMENTAR (paralelo quando possível)         │
│  Task(general-purpose) com prompts especializados       │
│  database → backend → frontend                          │
└─────────────────────────────────────────────────────────┘
                            ▼
┌─────────────────────────────────────────────────────────┐
│  FASE 4: REVIEW                                         │
│  Task(general-purpose) + prompts/review.md              │
└───────────────────────────┬─────────────────────────────┘
                            │
              ┌─────────────┴─────────────┐
              ▼                           ▼
        [APROVADO]                   [VOLTAR]
              │                           │
              ▼                    (volta FASE 3)
┌─────────────────────────────────────────────────────────┐
│  FASE 5: CONCLUIR                                       │
│  Atualiza registry, move spec para completed/           │
└─────────────────────────────────────────────────────────┘
```

---

## 6. Árvore de Decisão

```
Solicitação
    ↓
É bug? ──SIM──→ /mtd-pipeline-bugfix
    │
   NÃO
    ↓
É nova feature? ──SIM──→ /mtd-pipeline-feature
    │
   NÃO
    ↓
Task(general-purpose) com prompt específico
```

---

## 7. Enforcement Completo (L0-L9)

| Nível | Regra | Descrição |
|-------|-------|-----------|
| L0 | Delegação | Claude principal NÃO implementa código |
| L1 | grepai | Preferir grepai para busca semântica |
| L2 | Pipeline | Pipeline obrigatório para features/bugs |
| L3 | Padrões | Nomenclatura, soft delete, multi-tenancy |
| L4 | Type-check | Frontend deve passar type-check |
| L5 | Build | Backend deve compilar |
| L6 | Registry | Sync registry após criar entidades |
| L7 | DbContext | Service NÃO acessa DbContext direto |
| L8 | Repository | Service só injeta PRÓPRIO Repository |
| L9 | ISP | Preferir interfaces segregadas (SOLID) |

Ver detalhes em [core/enforcement.md](./core/enforcement.md).

---

## 8. Regras de Busca

**SEMPRE use grepai** para busca semântica:
```javascript
grepai_search({ query: "..." })
grepai_trace_callers({ symbol: "..." })
grepai_trace_callees({ symbol: "..." })
```

**SEMPRE use memory MCP** para contexto de pipeline:
```javascript
mcp__memory__search_nodes({ query: "pipeline phase" })
mcp__memory__open_nodes({ names: ["Pipeline:nome"] })
```

**⛔ PROIBIDO** usar Grep/Glob - hook `enforce-grepai.js` bloqueia automaticamente.

### Por que grepai?

| Ferramenta | Problema |
|------------|----------|
| Grep | Busca textual simples, muitos falsos positivos |
| Glob | Só encontra por nome de arquivo |
| grepai | Busca semântica, entende contexto e intenção |

---

## 9. Exemplo de Uso Correto

### Chamar Orchestrator para Feature

```javascript
Task({
  subagent_type: "general-purpose",
  model: "opus",
  description: "Orchestrate Invoice feature",
  prompt: `
# Você é o ORCHESTRATOR

## Identidade
Você coordena o pipeline de desenvolvimento. NÃO implementa código - delega.

## Pipeline Obrigatório
1. EXPLORAR: Use Task(subagent_type="Explore") para analisar
2. SPEC: Crie spec em spec/active/{nome}/spec.md
3. IMPLEMENTAR: Use Task(general-purpose) para cada camada
4. REVIEW: Use Task(general-purpose) com prompt de review
5. CONCLUIR: Atualize registry

## TAREFA
Implementar feature: Invoice
  `
})
```

### Chamar Explorer (nativo)

```javascript
Task({
  subagent_type: "Explore",
  model: "haiku",
  description: "Explore Invoice requirements",
  prompt: "Analisar requisitos para implementar entidade Invoice. Mapear arquivos existentes similares."
})
```

### Chamar Backend Specialist

```javascript
Task({
  subagent_type: "general-purpose",
  model: "opus",
  description: "Backend Invoice implementation",
  prompt: `
# Você é o BACKEND SPECIALIST

## Responsabilidades
- Implementar endpoints/APIs
- Criar serviços e lógica de negócio
- Seguir padrões do projeto

## Regras
- L7: Service NÃO acessa DbContext direto
- L8: Service só injeta PRÓPRIO Repository

## TAREFA
Implementar módulo backend para Invoice conforme spec.
  `
})
```

---

## 10. Project Context (v2.4)

### Contexto Auto-Gerado pelo CLI

O CLI gera automaticamente arquivos de contexto em `.claude/context/`:

```
.claude/context/
├── README.md             # Documentação da pasta
├── architecture.md       # AUTO: Tipo, stacks, layers
├── patterns.md           # AUTO: Padrões detectados
└── naming.md             # AUTO: Convenções de nomenclatura
```

### Arquivos do Usuário (Opcionais)

Você pode adicionar arquivos customizados (flat, sem subpastas):

```
.claude/context/
├── project-spec.md       # Especificação do projeto
├── business-rules.md     # Regras de negócio
├── tips.md               # Dicas para o Claude
├── service-example.md    # Exemplo de service
├── component-example.md  # Exemplo de component
└── hook-example.md       # Exemplo de hook
```

### Regras

| Regra | Descrição |
|-------|-----------|
| Markdown only | Apenas arquivos `.md` são carregados |
| Max 500 linhas | Arquivos maiores são truncados |
| Max 20 arquivos | Limite total de arquivos |
| Refresh 24h | Auto-refresh se contexto > 24h |

### Entity Types no Memory MCP

| Entity | Descrição |
|--------|-----------|
| `ProjectContext:current` | Metadados do projeto |
| `UserContext:{filename}` | Arquivos de context/ |
| `EntityRegistry:current` | Cache do entity-registry.json |
| `EnforcementRules:current` | Regras L0-L9 |
| `CodePattern:{type}` | Padrões descobertos via grepai |

### Usando Contexto (Agentes)

Todos os prompts de agentes agora buscam contexto automaticamente:

```javascript
// Buscar contexto antes de implementar
const context = await mcp__memory__search_nodes({
  query: "UserContext architecture CodePattern service"
});

// Abrir entidades específicas
if (context.entities?.length) {
  const details = await mcp__memory__open_nodes({
    names: context.entities.map(e => e.name)
  });
  // Usar exemplos e padrões encontrados
}
```

### Benefícios

| Métrica | Impacto |
|---------|---------|
| Tokens por feature | 📉 ~60% menos (menos exploração) |
| Retrabalho | 📉 Reduz (segue padrões) |
| Qualidade | 📈 Melhora (exemplos reais) |
| Consistência | 📈 Código uniforme |

---

## 11. Memory MCP - Persistência de Pipeline

O estado do pipeline é persistido via **memory MCP**, não via arquivos.

### Estrutura no Knowledge Graph

```
Pipeline:{nome}
├── type: "pipeline"
├── observations:
│   ├── "phase: explore|implement|completed"
│   ├── "started: {ISO_DATE}"
│   ├── "objetivo: {descrição}"
│   └── "arquivos: {lista}"
└── relations:
    └── has_spec → Spec:{nome}

Spec:{nome}
├── type: "spec"
└── observations:
    ├── "## Objetivo\n..."
    ├── "## Arquivos\n..."
    └── "## Checklist\n□ Backend □ Frontend"
```

### Operações Comuns

```javascript
// Criar pipeline (/mtd-pipeline-feature)
mcp__memory__create_entities({
  entities: [{
    name: "Pipeline:add-email",
    entityType: "pipeline",
    observations: [
      "phase: explore",
      "started: 2026-02-05",
      "objetivo: Adicionar email em Customer"
    ]
  }]
})

// Aprovar (/mtd-pipeline-approve)
mcp__memory__add_observations({
  observations: [{
    entityName: "Pipeline:add-email",
    contents: ["phase: implement", "approved: 2026-02-05"]
  }]
})

// Buscar ativo
mcp__memory__search_nodes({ query: "pipeline phase explore implement" })

// Finalizar (/mtd-pipeline-complete)
mcp__memory__delete_entities({
  entityNames: ["Pipeline:add-email", "Spec:add-email"]
})
```

---

## 12. Hooks de Enforcement

### enforce-pipeline.js (L0+L2)

- **Trigger:** Edit/Write em arquivos de código
- **Ação:** Pede confirmação, Claude verifica memory MCP
- **Exceções:** .md, .json, .yaml, .claude/, mustard/, spec/

### enforce-grepai.js (L1)

- **Trigger:** Grep/Glob
- **Ação:** BLOQUEIA com mensagem para usar grepai
- **Sem exceções**

---

## 13. Links

### Core

- [Enforcement L0-L9](./core/enforcement.md)
- [Naming Conventions](./core/naming-conventions.md)
- [Entity Registry Spec](./core/entity-registry-spec.md)
- [Pipeline](./core/pipeline.md)

### Prompts

- [Índice de Prompts](./prompts/_index.md)
- [Backend](./prompts/backend.md)
- [Frontend](./prompts/frontend.md)
- [Database](./prompts/database.md)

### Comandos - Pipeline

- [feature](./commands/mtd-pipeline-feature.md)
- [bugfix](./commands/mtd-pipeline-bugfix.md)
- [approve](./commands/mtd-pipeline-approve.md)
- [complete](./commands/mtd-pipeline-complete.md)
- [resume](./commands/mtd-pipeline-resume.md)

### Comandos - Outros

- [sync-registry](./commands/mtd-sync-registry.md)
- [install-deps](./commands/mtd-sync-dependencies.md)
- [load-context](./commands/mtd-sync-context.md)
- [daily-report](./commands/mtd-report-daily.md)
- [weekly-report](./commands/mtd-report-weekly.md)

### Context

- [context/README.md](./context/README.md)

### Hooks

- [enforce-pipeline.js](./hooks/enforce-pipeline.js)
- [enforce-grepai.js](./hooks/enforce-grepai.js)
