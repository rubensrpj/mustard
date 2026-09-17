//! O contexto compartilhado, dividido pelo que cada parte responde. Quem usa
//! traz só a parte de que precisa:
//!
//! - [`config`] — a configuração do projeto, lida uma vez por versão do
//!   `mustard.json`;
//! - [`env`] — a raiz do projeto, a pasta atual, a onda em curso e a pasta de
//!   spec que um argumento nomeia, para os comandos `run`;
//! - [`session`] — a sessão atual e a spec a que cada sessão está ligada;
//! - [`checkout`] — a spec da branch em que o checkout está;
//! - [`pending_branch`] — a branch de trabalho pendente de uma sessão;
//! - [`install`] — o modo de instalação e o arquivo de instruções das Guards.

pub mod checkout;
pub mod config;
pub mod env;
pub mod install;
pub mod pending_branch;
pub mod session;
