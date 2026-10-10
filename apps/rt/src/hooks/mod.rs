//! Os ganchos do Mustard, atrás do contrato `Check` / `Observer` do núcleo.
//!
//! São agrupados pela família do evento:
//!
//! - `bash` — a trava de comandos (`command_guard`).
//! - `write` — o portão de escrita (`write_gate`).
//! - `observe` — a testemunha da aprovação (`approval_witness`), o sinal de
//!   vida da onda (`wave_alive_observer`) e a
//!   testemunha do glossário do mapa (`glossary_witness`), que marca a
//!   palavra da busca na declaração editada logo depois.
//! - `session` — a entrada da mensagem (`prompt_entry`), o início da sessão
//!   (`session_start_inject`), o conserto da barra de status
//!   (`statusline_heal_observer`), a faxina do fim da sessão
//!   (`session_cleanup_observer`), o aviso antes de compactar
//!   (`conversation_size::PrecompactNotice`), que em toda compactação injeta o
//!   bloco de retomada, e o aviso de tamanho da conversa
//!   (`conversation_size::SizeNotice`): a quem conduz, o de limpar ou
//!   compactar, com o bloco de retomada; ao agente de onda, no fim de cada
//!   tarefa, a ordem de seguir ou de entregar, e a recusa de toda ferramenta,
//!   menos gravar na spec e compilar, depois da ordem de entregar; ao agente
//!   que a reprovação de quem conduz tirou da onda, a recusa de tudo.
//! - `task` — a conferência do fim da resposta (`end_of_turn_check`, com as
//!   regras dela) e o pedido do subagente (`subagent_inject`).
//!
//! O registro (`crate::registry`) diz onde cada um roda.

pub mod observe;
pub mod session;
pub mod write;
pub mod task;
pub mod bash;
