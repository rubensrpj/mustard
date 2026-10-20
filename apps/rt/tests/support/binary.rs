//! O programa que o teste roda, sem a pasta de configuração nem a sessão de
//! quem roda a suíte.
//!
//! A suíte costuma rodar de dentro de uma conversa do Claude Code. O ambiente
//! traz a sessão dessa conversa, e a pasta de configuração de verdade guarda
//! as conversas de todos os projetos da máquina: o programa que herda as duas
//! acha a conversa de verdade e lê as falas dela. Todo teste roda o programa
//! — ou o shell que o roda — por [`isolated`]. O teste que dá a sua pasta de
//! configuração, a sua pasta pessoal ou a sua sessão as dá depois, e a dele
//! vale. A pasta pessoal de quem roda fica: é por ela que o programa acha o
//! `cargo` que não está no `PATH`, e a de configuração já não sai dela.
//!
//! Um arquivo por pacote, como `manifest_dir.rs`: os testes da pasta `tests/`
//! o trazem pelo caminho.

use std::ffi::OsStr;
use std::process::Command;

/// As variáveis em que o programa lê a sessão de quem o chama.
const SESSION_VARS: [&str; 4] = ["MUSTARD_SESSION_ID", "CODEX_THREAD_ID", "CLAUDE_CODE_SESSION_ID", "CLAUDE_SESSION_ID"];

/// O comando que roda `program` sem a pasta de configuração do Claude Code de
/// quem roda a suíte e sem a sessão dele. A pasta de configuração fica dentro
/// do executável do próprio teste, que é um arquivo: ali nenhuma pasta existe
/// nem nasce, e nada se lê dela nem se grava nela.
pub fn isolated(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    command.env("CLAUDE_CONFIG_DIR", std::env::current_exe().expect("the test executable").join("claude-config"));
    for var in SESSION_VARS {
        command.env_remove(var);
    }
    // Sem a pasta do projeto da conversa: fora de um projeto, o programa
    // cairia nela e tomaria por sua a sessão mais nova de lá.
    command.env_remove("CLAUDE_PROJECT_DIR");
    command
}
