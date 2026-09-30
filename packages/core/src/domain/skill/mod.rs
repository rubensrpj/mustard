//! `skill` — a leitura do cabeçalho de uma skill.
//!
//! Dono do tipo [`frontmatter::SkillFrontmatter`] e da leitura dele. O
//! binário lê uma skill por aqui: a busca de skill que casa uma tarefa com uma
//! skill, o pedido da onda e a lista do agente (os dois mostram a descrição da
//! skill) e o censo da branch (que reconhece a skill escrita pelo scan pelo
//! `source`).

pub mod frontmatter;

pub use frontmatter::{extract_frontmatter, parse, SkillFrontmatter, SkillFrontmatterError};
