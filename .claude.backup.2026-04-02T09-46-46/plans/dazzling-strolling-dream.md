# Plano: Memória Compartilhada Entre Agentes (Zero-Token no Parent)

## Context

Mustard delega todo trabalho via Task tool, criando agentes isolados. Hoje, a comunicação entre agentes passa **obrigatoriamente pelo parent** (orchestrator), que precisa ler resultados do agente anterior e incluí-los no prompt do próximo — gastando tokens no contexto principal.

**Oportunidade descoberta:** O hook `SubagentStart` pode injetar `additionalContext` diretamente no agente sem o parent gastar tokens. Hoje injeta apenas `"[Tracker] Agent registered..."`. Podemos usar esse canal para injetar **memórias de agentes anteriores**.

## Diagnóstico Atual

| Mecanismo | Como funciona | Custo no parent |
|-----------|--------------|-----------------|
| Spec files (checkboxes) | Parent lê spec, extrai tasks, passa no prompt | ~1-2K tokens |
| Entity registry | Parent faz Grep, passa `{entity_info}` | ~200 tokens |
| Pipeline state JSON | Parent lê para saber fase | ~100 tokens |
| Mudanças em disco | Agente lê arquivos que outro criou | 0 (agente lê direto) |
| **SubagentStart hook** | **Injeta context sem custo no parent** | **0 tokens** |

**Waste principal:** Quando o Backend agent cria endpoints e o Frontend agent precisa saber quais, o parent gasta tokens relaying essa info. Com memória via hook, o custo cai a zero no parent.

## Solução: Agent Memory Layer

### Arquitetura

```
Wave 1: DB + Backend agents (paralelo)
  ↓ agents completam
  ↓ orchestrator escreve memória via script (1 chamada bash)
  ↓ wave transitions
Wave 2: Frontend agent inicia
  ↓ SubagentStart hook dispara
  ↓ hook lê _index.json → encontra memórias da Wave 1
  ↓ hook injeta via additionalContext (0 tokens no parent)
  ↓ Frontend agent sabe: "Backend criou /api/v1/payments, DB adicionou tabela Payments"
```

### Estrutura de Arquivos

```
.claude/.agent-memory/
  _index.json                                    # Índice com summaries embutidos
  {session8}-{agent_type}-{timestamp}.json       # Entrada completa (details)
```

### Formato do _index.json

```json
[{
  "id": "abc12345-impl-1711583400",
  "file": "abc12345-templates-impl-1711583400.json",
  "agent_type": "templates-impl",
  "wave": 1,
  "pipeline": "2026-03-25-feature-name",
  "summary": "Criou PaymentService.cs e PaymentRepository.cs. Padrão CQRS. Endpoint /api/v1/payments.",
  "timestamp": "2026-03-27T22:30:00.000Z"
}]
```

**Decisão:** Summary embutido no índice — hook lê 1 arquivo, não N.

### Limites

| Recurso | Limite | Justificativa |
|---------|--------|---------------|
| Summary por memória | 300 chars | Cabe ~4-5 memórias no budget |
| Entries no índice | 20 max | ~4KB total, evict mais antigo |
| additionalContext total | 1500 chars | ~375 tokens, dentro do budget de 2-3K |
| Escopo | Por sessão | Cleanup automático no SessionEnd |

### Mecanismo de Escrita

**Quem escreve:** O orchestrator, via script Node.js após cada wave.
**Quando:** Após agentes completarem e spec ser atualizada.

```bash
echo '{"agent_type":"templates-impl","wave":1,"pipeline":"feat-name","summary":"...","details":{...}}' | node .claude/scripts/memory-write.js
```

O script:
1. Gera ID (session prefix + type + timestamp)
2. Escreve arquivo JSON completo
3. Atualiza `_index.json` (append + cap em 20)

### Mecanismo de Leitura (Zero-Token)

No `subagent-tracker.js`, função `handleStart()`:

```js
// Após lógica existente de queue...
try {
  const memories = loadRelevantMemories(projectDir, agentType);
  if (memories.length > 0) {
    context += '\n\n[Agent Memory] Findings from prior agents:\n' +
      memories.map(m => `- [${m.agent_type}] ${m.summary}`).join('\n');
  }
} catch {} // fail-open
```

**Seleção:** Filtra por pipeline atual, exclui próprio tipo, ordena por wave/timestamp, acumula até 1500 chars.

## Arquivos a Modificar

| # | Arquivo | Mudança |
|---|---------|---------|
| 1 | `templates/scripts/memory-write.js` | **CRIAR** — script de escrita (~80 linhas) |
| 2 | `templates/hooks/subagent-tracker.js` | **MODIFICAR** — adicionar `loadRelevantMemories()` no `handleStart()` |
| 3 | `templates/hooks/session-cleanup.js` | **MODIFICAR** — adicionar cleanup de `.agent-memory/` |
| 4 | `templates/commands/mustard/resume/SKILL.md` | **MODIFICAR** — instrução de memory-write após cada wave |
| 5 | `templates/commands/mustard/feature/SKILL.md` | **MODIFICAR** — memory-write no Light scope execute |
| 6 | `templates/hooks/__tests__/hooks.test.js` | **MODIFICAR** — testes para memory read/write/cleanup |
| 7 | `.gitignore` | **MODIFICAR** — adicionar `.claude/.agent-memory/` |

## Sequência de Implementação

1. **Fase 1 (paralelo):** `memory-write.js` + testes + `.gitignore`
2. **Fase 2 (paralelo):** Enhance `subagent-tracker.js` + testes
3. **Fase 3 (paralelo):** Extend `session-cleanup.js`
4. **Fase 4 (sequencial):** Update `/resume` e `/feature` SKILL.md
5. **Fase 5:** Validação end-to-end

## Compatibilidade

- Se `.agent-memory/` não existe → hook retorna mensagem original (fail-open)
- Se `_index.json` corrupto → hook ignora (try/catch)
- Se `memory-write.js` falha → pipeline continua normal (memória é advisory)
- Commands antigos sem memory-write → simplesmente não criam memórias
- Sem mudanças em `settings.json` (hooks já registrados)

## Verificação

1. Rodar `node --test hooks/__tests__/hooks.test.js` — todos os testes passam
2. Criar memory entry manualmente: `echo '...' | node scripts/memory-write.js`
3. Verificar que `_index.json` foi criado com entry
4. Simular SubagentStart com memory presente → verificar `additionalContext` contém summaries
5. Verificar cleanup: simular SessionEnd → `.agent-memory/` removido
6. Pipeline real: `/feature` com 2+ waves → verificar que Wave 2 recebe memórias da Wave 1

## Economia de Tokens Estimada

| Cenário | Antes (parent relay) | Depois (hook inject) | Economia |
|---------|---------------------|---------------------|----------|
| Pipeline 2 waves, 3 agents | ~2-3K tokens no parent | ~0 tokens no parent | ~2-3K |
| Pipeline 3 waves, 5 agents | ~4-5K tokens no parent | ~0 tokens no parent | ~4-5K |
| Custo fixo (memory-write bash) | 0 | ~100 tokens (1 bash call/wave) | -100 |
| **Net saving por pipeline** | | | **~2-5K tokens** |

O saving real está em **não poluir o contexto do parent** — que é limitado e caro (opus). Os ~375 tokens injetados no agente via hook são "grátis" do ponto de vista do parent.
