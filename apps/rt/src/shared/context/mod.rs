//! O contexto compartilhado, dividido pelo que cada parte responde. Quem usa
//! traz só a parte de que precisa:
//!
//! - [`config`] — a configuração do projeto, lida uma vez por versão do
//!   `mustard.json`;
//! - [`env`] — a raiz do projeto, para os comandos `run`;
//! - [`session`] — a sessão atual e a spec a que cada sessão está ligada;
//! - [`checkout`] — a spec da branch em que o checkout está;
//! - [`pending_branch`] — a branch de trabalho pendente de uma sessão, hoje só
//!   armada pelos testes do portão de base.

pub mod checkout;
pub mod config;
pub mod env;
// Sem chamador na produção desde a refatoração que enxugou o runtime: quem
// arma o marcador são os testes do portão de base, guardado por decisão do
// usuário. Sai junto com o portão, quando ele for decidido.
#[cfg(test)]
pub mod pending_branch;
pub mod session;
