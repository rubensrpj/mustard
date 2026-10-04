//! O contexto compartilhado, dividido pelo que cada parte responde. Quem usa
//! traz só a parte de que precisa:
//!
//! - [`config`] — a configuração do projeto, lida uma vez por versão do
//!   `mustard.json`;
//! - [`env`] — a raiz do projeto, para os comandos `run`;
//! - [`session`] — a sessão atual e a spec a que cada sessão está ligada;
//! - [`checkout`] — a spec da branch em que o checkout está.

pub mod checkout;
pub mod config;
pub mod env;
pub mod session;
