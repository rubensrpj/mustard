//! O que sobrou da montagem de texto do agente: as peças que a porta do pull
//! request, a lista de pendências e a página da spec ainda leem.
//!
//! O renderizador antigo do pedido de onda saiu com o comando que o chamava —
//! quem monta o pedido hoje é a rodada do fluxo, a partir da spec. Ficam:
//!
//! - [`prompt_ref`] — a chave FNV estável;
//! - [`skills`] — a prateleira de skills de um subprojeto.

// `pub(crate)` para o resumo da spec carimbar o documento com o mesmo
// `fnv1a64` que nomeia o arquivo de despacho — um hash estável só, no crate.
pub(crate) mod prompt_ref;
// `pub(crate)`: the PR review step hands the reviewer the shelf the
// IMPLEMENTER was dispatched with, which only stays true while both read it
// from here.
pub(crate) mod skills;
