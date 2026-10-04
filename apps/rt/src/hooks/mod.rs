//! Os ganchos do Mustard, atrás do contrato `Check` / `Observer` do núcleo.
//!
//! São quinze, cada um num arquivo, agrupados pela família do evento:
//!
//! - `bash` — a trava de comandos (`command_guard`).
//! - `write` — o portão de escrita (`write_gate`).
//! - `observe` — a testemunha da aprovação (`approval_witness`), a da cópia
//!   da página da spec (`copy_witness`), que grava a cópia quando o último
//!   lote volta do banco, o sinal de vida da onda (`wave_alive_observer`) e a
//!   testemunha do glossário do mapa (`glossary_witness`), que marca a
//!   palavra da busca na declaração editada logo depois.
//! - `session` — a entrada da mensagem (`prompt_entry`), o início da sessão
//!   (`session_start_inject`), o conserto da barra de status
//!   (`statusline_heal_observer`), a faxina do fim da sessão
//!   (`session_cleanup_observer`), o aviso antes de compactar
//!   (`conversation_size::PrecompactNotice`), que em toda compactação injeta o
//!   bloco de retomada, e o aviso de tamanho da conversa
//!   (`conversation_size::SizeNotice`): a quem conduz, o de limpar ou
//!   compactar, com o bloco de retomada; ao agente de onda, o de parar no
//!   limite da conversa e gravar o que falta; e o aviso de releitura
//!   (`reread::RereadGuard`), que diz ao agente de onda, antes da leitura, que
//!   as linhas que ele pede de novo e que não mudaram estão acima na conversa.
//! - `task` — a conferência do fim da resposta (`end_of_turn_check`, com as
//!   regras dela) e o pedido do subagente (`subagent_inject`).
//!
//! O registro (`crate::registry`) diz onde cada um roda.

pub mod observe;
pub mod session;
pub mod write;
pub mod task;
pub mod bash;
