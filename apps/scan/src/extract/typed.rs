//! O tipo dono de um membro quando o próprio arquivo o escreve, sem inferir
//! nada: o mapa só liga o membro lido depois de um objeto (`pedido.total`)
//! quando o objeto é o próprio tipo ou um nome que o estreita. Duas escritas
//! dizem o dono com todas as letras, e as duas vêm da consulta da língua:
//!
//! - A desestruturação (`@member.of`): o tipo escrito antes dos membros do
//!   mesmo pattern (`Piece::Block` em `Piece::Block { open, close, .. }`). O
//!   caminho do tipo pode ser `Tipo::Variante` ou `módulo::Tipo`, que se
//!   escrevem igual: os dois últimos trechos valem como dono, e o grafo fica
//!   com o que declara o membro.
//! - O objeto tipado pela assinatura (`@local.type`): o tipo escrito para um
//!   nome, no parâmetro ou na variável (`pedido: &Pedido`). O membro lido
//!   depois desse nome (`pedido.total`) é do tipo, até o nome ser ligado de
//!   novo na mesma declaração, com tipo ou sem ele.
//!
//! O dono vira o qualificador do membro no mapa, como se o arquivo tivesse
//! escrito `Tipo.total`, e o grafo o liga pela regra do nome qualificado.

use std::collections::{BTreeSet, HashMap};

use tree_sitter::Node;

use super::{
    before_separator, code_before, is_name, member_separators, opens_chain, qualifier_separators, simple_type_name,
    without_type_arguments, Spans,
};
use crate::model::Decl;

/// Uma ligação de um nome local, com o tipo que a assinatura escreve para ele.
struct Binding {
    line: usize,
    byte: usize,
    /// A última linha da declaração em volta (`usize::MAX` sem fim conhecido).
    last: usize,
    /// O tipo escrito para o nome, quando a ligação o escreve.
    ty: Option<String>,
}

/// As ligações de nomes locais do arquivo, pelo nome, com o tipo que cada uma
/// escreve.
#[derive(Default)]
pub(super) struct Bindings(HashMap<String, Vec<Binding>>);

impl Bindings {
    /// As ligações de `locals` (linha, byte e nome de cada uma), cada uma com
    /// o tipo de `types` (o número dela em `locals` e o tipo), dentro da
    /// declaração de `decls` que a contém. O nome que é o de uma declaração
    /// (`names_at`) não é ligação: é a própria declaração.
    pub(super) fn of(
        decls: &[Decl],
        locals: &[(usize, usize, String)],
        types: &[(usize, String)],
        names_at: &BTreeSet<usize>,
    ) -> Bindings {
        let typed: HashMap<usize, &str> = types.iter().map(|(at, ty)| (*at, ty.as_str())).collect();
        let mut by_name: HashMap<String, Vec<Binding>> = HashMap::new();
        for (at, (line, byte, name)) in locals.iter().enumerate() {
            if names_at.contains(byte) {
                continue;
            }
            let Some(di) = crate::graph::enclosing(decls, *line) else { continue };
            let last = match decls[di].end_line {
                0 => usize::MAX,
                end => end,
            };
            let ty = typed.get(&at).map(|ty| (*ty).to_string());
            by_name.entry(name.clone()).or_default().push(Binding { line: *line, byte: *byte, last, ty });
        }
        Bindings(by_name)
    }

    /// O tipo do nome `name` escrito na linha `line`, no byte `byte`: o da
    /// ligação mais recente antes dele, dentro da declaração em volta. `None`
    /// quando a ligação mais recente não escreve tipo, ou quando não há.
    fn type_of(&self, name: &str, line: usize, byte: usize) -> Option<&str> {
        self.0
            .get(name)?
            .iter()
            .filter(|b| b.byte < byte && b.line <= line && line <= b.last)
            .max_by_key(|b| (b.byte, b.ty.is_some()))?
            .ty
            .as_deref()
    }
}

/// O que o arquivo escreve sobre o dono dos membros: o tipo da desestruturação
/// de cada membro e as ligações tipadas.
#[derive(Default)]
pub(super) struct Owners {
    /// O texto do tipo escrito antes dos membros, pelo byte de cada membro.
    written: HashMap<usize, String>,
    bindings: Bindings,
}

impl Owners {
    pub(super) fn new(written: HashMap<usize, String>, bindings: Bindings) -> Owners {
        Owners { written, bindings }
    }

    /// Os nomes que o arquivo dá como dono do membro escrito no nó `node`: os
    /// do tipo da desestruturação ou o tipo do objeto que abre a cadeia antes
    /// dele. `None` quando nada diz o dono, e o membro segue como foi escrito;
    /// vazio quando o arquivo diz um dono que não se lê como nome, e o membro
    /// não liga a ninguém.
    pub(super) fn of(&self, node: Node, bytes: &[u8], comments: &Spans, lang: &str) -> Option<Vec<String>> {
        if let Some(text) = self.written.get(&node.start_byte()) {
            return Some(type_names(text, lang));
        }
        let object = member_object(node, bytes, comments, lang)?;
        let line = node.start_position().row + 1;
        let ty = self.bindings.type_of(&object, line, node.start_byte())?;
        Some(vec![ty.to_string()])
    }
}

/// Os nomes que o tipo escrito em `text` pode ter: os dois últimos trechos do
/// caminho, sem os argumentos de tipo. `Piece::Block` é o tipo `Block` de um
/// módulo `Piece` ou a variante `Block` do tipo `Piece`, e o texto não diz
/// qual.
fn type_names(text: &str, lang: &str) -> Vec<String> {
    let separators = qualifier_separators(lang);
    let path = without_type_arguments(&text.split_whitespace().collect::<String>(), separators);
    let mut names = Vec::new();
    let mut rest = path.as_str();
    for _ in 0..2 {
        let cut = separators.iter().filter_map(|sep| rest.rfind(sep).map(|at| (at, sep.len()))).max_by_key(|&(at, _)| at);
        let Some((at, len)) = cut else {
            names.extend(simple_type_name(rest));
            break;
        };
        names.extend(simple_type_name(&rest[at + len..]));
        rest = &rest[..at];
    }
    names
}

/// O nome escrito logo antes do separador que liga o nó ao objeto, quando ele
/// abre a cadeia (`pedido` em `pedido.total`, e não em `this.pedido.total`
/// nem em `criar().pedido.total`).
fn member_object(node: Node, bytes: &[u8], comments: &Spans, lang: &str) -> Option<String> {
    let before = code_before(bytes, comments, node.start_byte());
    let separators = || qualifier_separators(lang).iter().chain(member_separators(lang));
    let before = separators().find_map(|sep| before.strip_suffix(sep.as_bytes()))?;
    let before = before_separator(bytes, comments, before.len());
    let start = before.iter().rposition(|b| !(b.is_ascii_alphanumeric() || *b == b'_' || *b >= 0x80)).map_or(0, |i| i + 1);
    let name = std::str::from_utf8(&before[start..]).ok().filter(|name| is_name(name))?;
    opens_chain(node, bytes, comments, lang, name).then(|| name.to_string())
}
