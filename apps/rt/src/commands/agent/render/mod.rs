//! O que sobrou da montagem de texto do agente: a chave que a lista de
//! pendências e a rodada ainda leem.
//!
//! O renderizador antigo do pedido de onda saiu com o comando que o chamava —
//! quem monta o pedido hoje é a rodada do fluxo, a partir da spec. Fica
//! [`prompt_ref`], a chave FNV estável.

// `pub(crate)` para o resumo da spec carimbar o documento com o mesmo
// `fnv1a64` que nomeia o arquivo de despacho — um hash estável só, no crate.
pub(crate) mod prompt_ref;
