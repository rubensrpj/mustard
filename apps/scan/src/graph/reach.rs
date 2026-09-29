//! O que uma chamada alcança sem que o arquivo tenha o alvo à vista: pelo
//! repasse do arquivo que o caminho nomeia, e pelo tipo do objeto que recebe a
//! chamada. As duas ligações têm resposta certa no que o mapa guarda, e nenhuma
//! liga por ser o único nome com aquele nome no projeto.
//!
//! - **Repasse.** O caminho escrito antes do nome (`mustard_core::translate(`)
//!   nomeia um arquivo que só repassa o nome (`pub use`, `export { x } from`);
//!   a declaração mora no arquivo que o repasse alcança. O nome de um pacote do
//!   projeto escrito sozinho antes do nome chamado é esse caminho, de uma parte
//!   só.
//! - **Tipo do receptor.** O objeto com tipo escrito (`ctx: &Ctx`) e a cadeia
//!   de campos depois dele (`ctx.config.language()`) dão o tipo do receptor:
//!   o tipo do objeto vem da assinatura, o de cada campo, da assinatura do
//!   campo no mapa. A chamada liga ao método desse tipo. Quando o tipo não sai
//!   assim — nome que dois tipos têm, campo que o mapa não guarda, tipo de
//!   fora do projeto —, a chamada segue como sempre seguiu.
//!
//! # Limites
//!
//! O tipo do receptor é lido só do que está escrito e do que o mapa guarda
//! por tipo e por nome. Três casos, por isso, não saem, e a chamada neles
//! fica onde ficava, suspeita entre as declarações de mesmo nome que o
//! arquivo tem à vista, nunca ligada a uma só:
//!
//! - **Método de extensão.** O receptor `pedido.Total()` tem o tipo `Pedido`,
//!   mas o `Total` mora numa classe estática à parte (`this Pedido`), e a
//!   chamada só liga ao método cujo dono é o tipo do receptor.
//! - **Campo de classe base.** O campo de um tipo é só o que o próprio tipo
//!   declara, no arquivo dele. `_repo` declarado em `Base` e chamado por
//!   `Filho : Base` não é campo do `Filho` para esta leitura, e o tipo do
//!   receptor não sai; o mapa não segue a herança para achar campo.
//! - **Nome repetido num módulo em linha.** Os tipos são indexados pelo nome,
//!   sem separar o módulo escrito dentro do arquivo. Dois tipos de mesmo nome
//!   no mesmo arquivo, um deles dentro de `mod interno { }`, empatam, e nem o
//!   tipo do objeto nem o dos campos dele saem; o módulo em linha não
//!   sombreia o nome de fora.
//!
//! O custo entra no tempo do scan: montar o índice dos tipos e ler o tipo de
//! cada receptor deixam a passada de 5 a 9 por cento mais lenta (medida de
//! quando a leitura entrou, não refeita desde então). E o que as
//! declarações guardam mudou com ela: o formato do bloco `decls` do mapa
//! ([`mustard_core::io::project_map::DECLS`]) é o da versão 11, o mapa gravado
//! na anterior perde o bloco na troca e o scan seguinte lê o projeto inteiro
//! de novo, uma vez.

use std::collections::{HashMap, HashSet};

use super::{DeclId, Resolver, TYPE_KINDS};
use crate::model::{Decl, Module};

/// Os arquivos de `files` e, para cada um que repassa `name`, os arquivos que
/// o declaram, seguidos os repasses ([`Resolver::declaring`]).
pub(super) fn declared_in(resolver: &Resolver, files: HashSet<String>, name: &str) -> HashSet<String> {
    let mut reached = files.clone();
    for file in &files {
        if let Some(&module) = resolver.by_path.get(file.as_str()) {
            reached.extend(resolver.declaring(module, name).into_iter().map(|(declaring, _)| declaring));
        }
    }
    reached
}

/// Os tipos de declaração que têm campos.
const FIELD_KINDS: &[&str] = &["field", "property"];

/// Os tipos do projeto, pelo nome, para ler o tipo de um receptor.
pub(super) struct Typing<'a> {
    modules: &'a [Module],
    types: HashMap<&'a str, Vec<DeclId>>,
}

impl<'a> Typing<'a> {
    pub(super) fn new(modules: &'a [Module]) -> Self {
        let mut types: HashMap<&str, Vec<DeclId>> = HashMap::new();
        for (mi, m) in modules.iter().enumerate() {
            for (di, d) in m.declarations.iter().enumerate() {
                if !d.name.is_empty() && TYPE_KINDS.contains(&d.kind.as_str()) {
                    types.entry(d.name.as_str()).or_default().push((mi, di));
                }
            }
        }
        Self { modules, types }
    }

    fn decl(&self, (mi, di): DeclId) -> &'a Decl {
        &self.modules[mi].declarations[di]
    }

    /// O tipo do receptor `chain` (`Ctx.config`, ou `this.repo`): o primeiro
    /// nome é o tipo do objeto, e cada um dos outros, um campo. `own` são os
    /// nomes dos tipos em que a chamada está escrita e `selves` os nomes que
    /// a língua dá ao próprio objeto; `sees` diz se o arquivo da chamada vê o
    /// arquivo `mi`. `None` quando algum passo não tem uma resposta só.
    pub(super) fn receiver_type(
        &self,
        chain: &str,
        src: usize,
        (own, selves): (&[&str], &[&str]),
        sees: &dyn Fn(usize) -> bool,
    ) -> Option<DeclId> {
        let mut parts = chain.split('.');
        let head = parts.next()?;
        let mut current = if selves.contains(&head) {
            let found: Vec<DeclId> = own
                .iter()
                .flat_map(|name| self.types.get(name).into_iter().flatten().copied())
                .filter(|&(mi, _)| mi == src)
                .collect();
            single(found)?
        } else {
            let found: Vec<DeclId> =
                self.types.get(head)?.iter().copied().filter(|&(mi, _)| sees(mi)).collect();
            single(found)?
        };
        for field in parts {
            current = self.field_type(current, field)?;
        }
        Some(current)
    }

    /// O tipo do receptor `chain` (`_logger`, ou `_ctx.config`) escrito pelo
    /// nome sozinho, sem tipo e sem ligação local: o primeiro nome é um campo
    /// do tipo em volta (`own`, dentro do arquivo `src`), e cada um dos
    /// outros, um campo do tipo do anterior. `None` quando algum passo não
    /// tem uma resposta só, e o nome pode ser, por exemplo, um tipo.
    pub(super) fn field_receiver(&self, chain: &str, src: usize, own: &[&str]) -> Option<DeclId> {
        let found: Vec<DeclId> = own
            .iter()
            .flat_map(|name| self.types.get(name).into_iter().flatten().copied())
            .filter(|&(mi, _)| mi == src)
            .collect();
        chain.split('.').try_fold(single(found)?, |current, field| self.field_type(current, field))
    }

    /// O tipo do campo `field` do tipo `ty`, lido da assinatura do campo.
    fn field_type(&self, ty: DeclId, field: &str) -> Option<DeclId> {
        let owner = self.decl(ty).name.as_str();
        let module = &self.modules[ty.0];
        let found: Vec<&Decl> = module
            .declarations
            .iter()
            .filter(|d| FIELD_KINDS.contains(&d.kind.as_str()) && d.name == field && d.owner.iter().any(|o| o == owner))
            .collect();
        let [declared] = found.as_slice() else { return None };
        let (path, name) = written_type(&declared.signature, field, |name| self.types.contains_key(name))?;
        let candidates = self.types.get(name.as_str())?;
        let by_path: Vec<DeclId> = candidates
            .iter()
            .copied()
            .filter(|&(mi, _)| path.iter().all(|part| names_part(&self.modules[mi].path, part)))
            .collect();
        // Sem o caminho, ou com ele e mais de um tipo, o do próprio arquivo do
        // campo e depois os que ele importa desempatam.
        let pool = if by_path.is_empty() { candidates.clone() } else { by_path };
        match pool.len() {
            1 => Some(pool[0]),
            _ => {
                let own: Vec<DeclId> = pool.iter().copied().filter(|&(mi, _)| mi == ty.0).collect();
                if own.len() == 1 {
                    return Some(own[0]);
                }
                let imported: Vec<DeclId> = pool
                    .iter()
                    .copied()
                    .filter(|&(mi, _)| module.deps.iter().any(|dep| *dep == self.modules[mi].path))
                    .collect();
                single(imported)
            }
        }
    }

    /// Se o tipo `ty` só tem uma declaração no projeto com o nome dele.
    pub(super) fn is_the_only(&self, ty: DeclId) -> bool {
        self.types.get(self.decl(ty).name.as_str()).is_some_and(|all| all.len() == 1)
    }
}

/// O único elemento de `found`, quando é um só.
fn single(found: Vec<DeclId>) -> Option<DeclId> {
    match found.as_slice() {
        [only] => Some(*only),
        _ => None,
    }
}

/// Se uma parte do caminho escrito de um tipo (`config` em
/// `crate::domain::config::ProjectConfig`) é o nome do arquivo ou de uma
/// pasta do caminho `file`.
fn names_part(file: &str, part: &str) -> bool {
    let mut parts = file.split('/');
    let name = parts.next_back().unwrap_or_default();
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    stem == part || parts.any(|dir| dir == part)
}

/// O tipo que a assinatura `signature` do campo `field` escreve, com o
/// caminho que vem antes do nome dele: o primeiro nome de tipo do projeto
/// (`is_type`) depois do campo (`config: crate::domain::ProjectConfig`), ou o
/// último antes dele (`private readonly IRepo repo`). O tipo embrulhado
/// (`Vec<Pedido>`, `Pedido[]`, `Pedido | Outro`) não vale: o campo é de outra
/// coisa. Os caminhos que só dizem de onde o nome vem (`crate`, `self`,
/// `super`) não contam.
fn written_type(signature: &str, field: &str, is_type: impl Fn(&str) -> bool) -> Option<(Vec<String>, String)> {
    let plain = without_generics(signature);
    let tokens = path_tokens(&plain);
    let at = tokens.iter().position(|token| token.path.len() == 1 && token.path[0] == field)?;
    let candidate = |token: &Token| token.path.last().is_some_and(|name| is_type(name));
    let after = tokens[at + 1..].iter().position(candidate).map(|found| at + 1 + found);
    let index = after.or_else(|| tokens[..at].iter().rposition(candidate))?;
    let token = &tokens[index];
    // A união (`A | B`) escrita depois do campo não é um tipo só.
    let union = index > at && plain[token.end..].trim_start().starts_with(['|', '&']);
    if !token.plain_end || union {
        return None;
    }
    let mut path = token.path.clone();
    let name = path.pop()?;
    path.retain(|part| !matches!(part.as_str(), "crate" | "self" | "super"));
    Some((path, name))
}

/// Um nome, ou caminho de nomes ligados por `::` ou `.`, escrito numa
/// assinatura.
struct Token {
    path: Vec<String>,
    /// Nada que embrulhe o tipo vem logo depois dele: colchete, barra, `&` ou
    /// `(`.
    plain_end: bool,
    end: usize,
}

/// Os nomes e caminhos da assinatura, na ordem do texto.
fn path_tokens(text: &str) -> Vec<Token> {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut tokens: Vec<Token> = Vec::new();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        if !is_word(chars[i].1) {
            i += 1;
            continue;
        }
        let mut path: Vec<String> = Vec::new();
        let mut end;
        loop {
            let start = chars[i].0;
            while i < chars.len() && is_word(chars[i].1) {
                i += 1;
            }
            end = chars.get(i).map_or(text.len(), |c| c.0);
            path.push(text[start..end].to_string());
            let rest = &text[end..];
            let step = if rest.starts_with("::") {
                2
            } else if rest.starts_with('.') {
                1
            } else {
                0
            };
            if step == 0 || !rest[step..].chars().next().is_some_and(is_word) {
                break;
            }
            while i < chars.len() && chars[i].0 < end + step {
                i += 1;
            }
        }
        let plain_end = !text[end..].trim_start().starts_with(['[', '(']);
        tokens.push(Token { path, plain_end, end });
    }
    tokens
}

/// O texto sem os argumentos de tipo (`<...>`, com os de dentro).
fn without_generics(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth = 0usize;
    let mut previous = ' ';
    for ch in text.chars() {
        match ch {
            '<' => depth += 1,
            '>' if depth > 0 && previous != '-' => depth -= 1,
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
        previous = ch;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_type(name: &str) -> bool {
        ["ProjectConfig", "IRepo", "Pedido", "Outro"].contains(&name)
    }

    #[test]
    fn the_type_is_read_after_the_field_or_before_it() {
        let read = |signature: &str, field: &str| written_type(signature, field, is_type);
        let name = |path: &[&str], ty: &str| Some((path.iter().map(|p| p.to_string()).collect::<Vec<_>>(), ty.to_string()));
        assert_eq!(read("pub config: crate::domain::config::ProjectConfig", "config"), name(&["domain", "config"], "ProjectConfig"));
        assert_eq!(read("private readonly IRepo _repo", "_repo"), name(&[], "IRepo"));
        assert_eq!(read("private repo: IRepo = new Repo();", "repo"), name(&[], "IRepo"));
        assert_eq!(read("pub config: &'a ProjectConfig", "config"), name(&[], "ProjectConfig"));
    }

    #[test]
    fn a_wrapped_type_is_not_the_type_of_the_field() {
        let read = |signature: &str, field: &str| written_type(signature, field, is_type);
        assert_eq!(read("pub items: Vec<Pedido>", "items"), None);
        assert_eq!(read("items: Pedido[]", "items"), None);
        assert_eq!(read("item: Pedido | Outro", "item"), None);
        assert_eq!(read("private List<IRepo> repos", "repos"), None);
        assert_eq!(read("pub name: String", "name"), None);
    }
}
