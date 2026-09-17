//! O que sobrou da montagem de texto do agente: as peças que a porta do pull
//! request, a lista de pendências e a página da spec ainda leem.
//!
//! O renderizador antigo do pedido de onda saiu com o comando que o chamava —
//! quem monta o pedido hoje é a rodada do fluxo, a partir da spec. Ficam:
//!
//! - [`prompt_ref`] — o caminho determinístico e a chave FNV do despacho;
//! - [`reference`] — a leitura da seção de arquivos de uma spec;
//! - [`skills`] — a prateleira de skills de um subprojeto.

// `pub(crate)` para o resumo da spec carimbar o documento com o mesmo
// `fnv1a64` que nomeia o arquivo de despacho — um hash estável só, no crate.
pub(crate) mod prompt_ref;
// `pub(crate)` so the `/mustard:pr` door's review step reads a spec's declared
// files through the SAME parser the dispatch prompt uses — a second reader of
// `## Files` is a second spelling of the section, and the two would drift.
pub(crate) mod reference;
// `pub(crate)` for the same reason as `reference` above: the PR review step
// hands the reviewer the shelf the IMPLEMENTER was dispatched with, which only
// stays true while both read it from here.
pub(crate) mod skills;

pub use prompt_ref::PROMPT_REF_MARKER;
