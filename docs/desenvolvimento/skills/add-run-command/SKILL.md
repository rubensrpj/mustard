---
name: add-run-command
description: Adicionar uma porta mustard-rt run mantendo CLI, despacho, documentação útil e provas de comportamento coerentes.
---
# Adicionar uma porta run

Localize a família no enum `RunCmd` e no despacho em `apps/rt/src/commands/mod.rs`, e leia a CLI da família. O formato da saída depende do contrato real: consultas de mapa/leitura podem retornar texto; ações que recusam usam o formato e exit code vigentes. Não imponha JSON a todas as portas nem copie um número fixo de registros.

1. Separe política pura, leitura/persistência e apresentação. Reuse `packages/core/src/domain`, `packages/core/src/io` e o executor compartilhado quando aplicável. Caminhos/comandos vêm da configuração e da raiz da execução, sem política específica de linguagem.
2. Adicione a variante, opções, despacho e registros da família. Use o próximo `display_order` vigente, localizado com `rg`. Regrave `apps/rt/tests/fixtures/run-surface.txt` a partir da árvore real quando a superfície mudar.
3. Documente na porta em que a ação é útil. As fontes distribuídas são `packages/core/templates/**`, `plugin/commands/**`, `plugin/output-styles/**` e o catálogo i18n. `apps/cli/templates/**` e `RUNTIME_WHITELIST` são referências antigas. Confira as justificativas atuais em `template_parity`; não acrescente uma instrução a todo agente apenas para satisfazer contagem de chamadores.
4. A gravação de estado usa `spec_events::write::record`, com fase, autoria, lock, aprovação testemunhada e recusas preservados. Pedido canônico e registro de leitura continuam obrigatórios. `--copy` é compatibilidade com aviso; gravação comum não sincroniza páginas.
5. Estado/painel consultam projeções locais. Exportação externa só por ação explícita, com versão, lista permitida e sucesso remoto distinto de preparação local. Não escreva conversa, código ou caminhos absolutos no snapshot público.
6. Prove a ação pela porta afetada e a recusa com estado preservado. Travas/concorrência/exit code exigem processos reais. Registre o módulo em `apps/rt/tests/it.rs`, pois a descoberta automática está desabilitada.
7. Execute checks pertinentes e paridade de textos/opções/registro. Para mudança distribuída, compile CLI/runtime/scan afetados e use o CLI deste checkout em pasta temporária vazia: `<checkout>/target/debug/mustard init --yes`. Preserve configuração pessoal, idiomas e fallback.

Julgamento semântico usa a interface de provedor com finalidade, evidência e versão. Não crie chamadas pagas para regras exatas, texto de relatório ou status; ausência de medição permanece desconhecida.
