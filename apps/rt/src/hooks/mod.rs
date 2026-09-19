//! Os ganchos do Mustard, atrás do contrato `Check` / `Observer` do núcleo.
//!
//! São onze, cada um num arquivo, agrupados pela família do evento:
//!
//! - `bash` — a trava de comandos (`command_guard`).
//! - `write` — o portão de escrita (`write_gate`).
//! - `observe` — a testemunha da aprovação (`approval_witness`) e o sinal de
//!   vida da onda (`wave_alive_observer`).
//! - `session` — a entrada da mensagem (`prompt_entry`), o início da sessão
//!   (`session_start_inject`), o conserto da barra de status
//!   (`statusline_heal_observer`), a faxina do fim da sessão
//!   (`session_cleanup_observer`) e a pausa por tamanho da conversa
//!   (`conversation_size::WavePauseCheck`).
//! - `task` — a conferência do fim da resposta (`end_of_turn_check`, com as
//!   regras dela) e o pedido do subagente (`subagent_inject`).
//!
//! O registro (`crate::registry`) diz onde cada um roda.

pub mod observe;
pub mod session;
pub mod write;
pub mod task;
pub mod bash;
