---
id: wave.every-finding-must-have-declared.1-core
---

# wave-1-core

## Summary

O achado ganha modelo: um item que ou tem destino declarado, ou esta aberto — espelhando ChecklistItem.

## Network

- Parent: [[spec.every-finding-must-have-declared]]

## Tasks

- [ ] Em packages/core/src/domain/spec/contract.rs, criar FindingItem espelhando ChecklistItem: `id`, `source` (revisor ou ledger de provas), `statement`, e `routed: Option<FindingRoute>`. Serde aditivo: `#[serde(default, skip_serializing_if = "Option::is_none")]` no campo opcional, para que sidecar historico volte byte-identico.
- [ ] Criar FindingRoute com os quatro destinos que a conversa fixou — criterio, pedido de mudanca, trabalho enfileirado, descartado — cada um carregando o motivo por extenso. Um destino sem motivo nao e destino.
- [ ] Criar FindingState { Open, Routed } e FindingItem::is_open(), com o MESMO racional documentado em ChecklistItem::is_open() (contract.rs:177): um achado descartado com motivo NAO esta aberto, e um `!routed` cru contaria a decisao deliberada como esquecimento.
- [ ] Em packages/core/src/domain/meta.rs, adicionar `findings: Vec<FindingItem>` ao lado de `checklist` (meta.rs:123), com o mesmo contrato aditivo: `#[serde(default, skip_serializing_if = "Vec::is_empty")]`.
- [ ] Testes nomeados `finding_item_*`: round-trip com destino e motivo; meta.json historico sem a chave le como lista vazia e volta sem inventar a chave; is_open() distingue aberto de descartado-com-motivo.

## Files

- `packages/core/src/domain/spec/contract.rs`
- `packages/core/src/domain/meta.rs`
