# Wave 3 — picker honesto (`active-specs`)

### Stage: Close
### Outcome: Completed
### Flags: 

## Contexto

Hoje `apps/rt/src/run/active_specs.rs:136-138` filtra qualquer spec sem `stage` E `outcome` no `meta.json` — resultado: spec malformada some do listing em vez de aparecer com sinal de alerta. Foi assim que a `dashboard-i18n-migration` ficou invisível pra mim na investigação inicial. Além disso, o estado `closed-followup` (Close+Active legítimo) é tratado como Close puro e excluído.

Esta wave torna o picker honesto: lista **todas** as specs com Stage∈{Plan,Execute} ou Outcome=Active ou em estado malformado, com sigla explícita.

## Tarefas

- [x] **T3.1** — Em `active_specs.rs:136-138`, substituir o filtro que descarta specs sem `stage`/`outcome` por inclusão com sigla `??` na coluna Estágio e flag `⚠ malformed` no Status. Spec aparece na lista — usuário decide o que fazer.
- [x] **T3.2** — Detectar combinação `Stage=Close + Outcome=Active` (que é `closed-followup` por mapping) e renderizar como sigla **`CLR→fu`** na coluna Estágio + texto `closed-followup` na coluna Status. Não excluir do listing.
- [x] **T3.3** — Atualizar a legenda das siglas (impressa no rodapé do picker) pra incluir `??` (malformed) e `CLR→fu` (closed-followup) — seguindo a memória [[feedback_siglas_always_with_legend]].
- [x] **T3.4** — Teste unitário: passar um diretório com 3 specs (uma válida em Plan, uma `closed-followup`, uma malformed) e verificar que as três aparecem na saída JSON, cada uma com sigla correta.

## Critérios de Aceitação

- **AC-W3.1** — `mustard-rt run active-specs --format json` retorna a `dashboard-i18n-migration` na lista, com `stage_code: "CLR→fu"` (ou equivalente). Command: `rtk mustard-rt run active-specs --format json | rtk node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{const j=JSON.parse(s);const f=j.specs.find(x=>x.name.includes('dashboard-i18n'));if(!f||!/CLR/.test(f.stage_code||''))process.exit(1)})"`
- **AC-W3.2** — Em uma fixture com `meta.json` ausente, o picker lista a spec com sigla `??` e flag `malformed`. Command: `rtk cargo test -p mustard-rt active_specs_includes_malformed`
- **AC-W3.3** — Saída texto do `active-specs` (formato tabela) inclui a legenda das siglas no rodapé, mencionando `??` e `CLR→fu`. Command: `rtk mustard-rt run active-specs | rtk node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{if(!/CLR.*fu/.test(s)||!/\?\?/.test(s))process.exit(1)})"`

## Limites

- **IN**: `apps/rt/src/run/active_specs.rs`.
- **OUT**: dashboard Tauri (consome JSON, vai funcionar sem mudança); nenhuma renomeação de sigla legada.
