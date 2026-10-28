//! O alcance de cada nome que o corpo de uma função liga (`@local`): de logo
//! depois do nome, onde a ligação o escreve, até o fim do trecho em que a
//! língua o deixa valer, tirado da árvore pelo que a consulta marca nela. O
//! próprio nome ligado não é uso do nome local: ali ele pode ser o campo que
//! a desestruturação lê (`const { total } = pedido`).
//!
//! - `@local.scope`: o nó que fecha o alcance dos nomes ligados dentro dele (o
//!   bloco, o braço, a closure, a função); vale o mais de dentro. Escrito no
//!   mesmo pattern do `@local`, fecha o alcance só desse nome, e pode ser um
//!   nó ao lado da ligação (o corpo que a gramática põe ao lado da
//!   assinatura).
//! - `@local.scope.function`: a função, que fecha também o alcance do nome
//!   ligado dentro de um `@local.hoist`.
//! - `@local.scope.whole`: o nó inteiro é o alcance dos nomes ligados dentro
//!   dele, também antes da ligação (a compreensão, que escreve o valor antes
//!   de ligar o nome).
//! - `@local.hoist`: o nome ligado dentro dele passa pelos blocos e vale até o
//!   fim da `@local.scope.function` em volta.
//! - `@local.outside`: o filho que fica fora do alcance dos nomes que o resto
//!   do nó pai liga (o valor que a ligação lê antes de ligar o nome, o `else`
//!   que a língua separa do padrão).
//! - `@variable`: o nome escrito como variável, que a língua separa da função
//!   e do tipo de mesmo nome: o nome local ligado como variável alcança só o
//!   nome escrito como variável, e o outro só o que não é.

use std::collections::{BTreeSet, HashMap, HashSet};

use tree_sitter::Node;

/// O que a consulta marcou na árvore para medir o alcance dos nomes locais,
/// juntado match a match.
#[derive(Default)]
pub(super) struct Marked<'t> {
    /// Cada nome ligado, com o nó que o próprio pattern dá como o fim do
    /// alcance dele, quando dá.
    bound: Vec<(Node<'t>, Option<Node<'t>>)>,
    scopes: HashSet<usize>,
    functions: HashSet<usize>,
    wholes: HashSet<usize>,
    hoists: HashSet<usize>,
    /// Os trechos fora do alcance, pelo nó pai de cada um.
    outside: HashMap<usize, Vec<(usize, usize)>>,
    variables: BTreeSet<usize>,
}

/// O jeito como um nó marcado fecha o alcance dos nomes ligados dentro dele.
#[derive(Clone, Copy)]
pub(super) enum Closes {
    Scope,
    Function,
    Whole,
    Hoist,
}

impl<'t> Marked<'t> {
    /// O nome ligado no nó `node`; `scope` é o nó que o mesmo pattern dá como
    /// o fim do alcance dele.
    pub(super) fn bind(&mut self, node: Node<'t>, scope: Option<Node<'t>>) {
        self.bound.push((node, scope));
    }

    /// O nó que fecha o alcance dos nomes ligados dentro dele, do jeito de
    /// `closes`.
    pub(super) fn closes(&mut self, node: Node, closes: Closes) {
        let into = match closes {
            Closes::Scope => &mut self.scopes,
            Closes::Function => &mut self.functions,
            Closes::Whole => &mut self.wholes,
            Closes::Hoist => &mut self.hoists,
        };
        into.insert(node.id());
    }

    /// O filho que fica fora do alcance dos nomes que o resto do pai liga.
    pub(super) fn outside(&mut self, node: Node) {
        if let Some(parent) = node.parent() {
            self.outside.entry(parent.id()).or_default().push((node.start_byte(), node.end_byte()));
        }
    }

    /// O nome escrito como variável.
    pub(super) fn variable(&mut self, node: Node) {
        self.variables.insert(node.start_byte());
    }

    /// O alcance de cada nome ligado, pelo texto dele em `bytes`. O nome ligado
    /// que é o nome de uma declaração (`names_at`) não conta: é a própria
    /// declaração. O nome sem nó marcado em volta que feche o alcance dele não
    /// alcança nada. Duas ligações no mesmo byte valem pela que o pattern
    /// fecha.
    pub(super) fn reaches(self, bytes: &[u8], names_at: &BTreeSet<usize>) -> Reaches {
        let mut by_byte: HashMap<usize, (Node, Option<Node>)> = HashMap::new();
        for (node, scope) in &self.bound {
            let entry = by_byte.entry(node.start_byte()).or_insert((*node, *scope));
            entry.1 = entry.1.or(*scope);
        }
        let mut by_name: HashMap<String, Vec<Reach>> = HashMap::new();
        for (byte, (node, scope)) in by_byte {
            if names_at.contains(&byte) {
                continue;
            }
            let Ok(name) = node.utf8_text(bytes) else { continue };
            if let Some(reach) = self.reach_of(node, scope) {
                by_name.entry(name.to_string()).or_default().push(reach);
            }
        }
        Reaches { by_name, variables: self.variables }
    }

    /// O alcance do nome ligado em `node`, subindo pela árvore até o nó que o
    /// fecha: `scope`, quando o pattern o dá, ou o primeiro marcado em volta.
    /// No caminho, junta os trechos que os pais deixam fora dele.
    fn reach_of(&self, node: Node, scope: Option<Node>) -> Option<Reach> {
        let byte = node.start_byte();
        let mut reach = Reach { from: node.end_byte(), to: 0, outside: Vec::new(), variable: self.variables.contains(&byte) };
        let mut hoisted = self.hoists.contains(&node.id());
        let mut at = node;
        while let Some(parent) = at.parent() {
            if let Some(scope) = scope
                && parent.start_byte() <= scope.start_byte()
                && scope.end_byte() <= parent.end_byte()
            {
                reach.to = scope.end_byte();
                return Some(reach);
            }
            let inside = |&(start, end): &(usize, usize)| !(start <= byte && byte < end);
            reach.outside.extend(self.outside.get(&parent.id()).into_iter().flatten().copied().filter(inside));
            let id = parent.id();
            hoisted |= self.hoists.contains(&id);
            if scope.is_none() {
                let whole = !hoisted && self.wholes.contains(&id);
                if self.functions.contains(&id) || whole || (!hoisted && self.scopes.contains(&id)) {
                    if whole {
                        reach.from = parent.start_byte();
                    }
                    reach.to = parent.end_byte();
                    return Some(reach);
                }
            }
            at = parent;
        }
        None
    }
}

/// Onde vale um nome ligado: os bytes de `from` até antes de `to`, menos os
/// trechos de `outside`, só no nome escrito como variável quando `variable`.
struct Reach {
    from: usize,
    to: usize,
    outside: Vec<(usize, usize)>,
    variable: bool,
}

/// O alcance de cada nome que o corpo de uma função do arquivo liga.
pub(super) struct Reaches {
    by_name: HashMap<String, Vec<Reach>>,
    variables: BTreeSet<usize>,
}

impl Reaches {
    /// Se o nome `name`, escrito sozinho no byte `byte`, está no alcance de
    /// uma ligação dele: aí ele é o valor ligado, e não a declaração do
    /// projeto de mesmo nome.
    pub(super) fn covers(&self, name: &str, byte: usize) -> bool {
        let variable = self.variables.contains(&byte);
        self.by_name.get(name).is_some_and(|reaches| {
            reaches.iter().any(|r| {
                r.variable == variable && r.from <= byte && byte < r.to && !r.outside.iter().any(|&(start, end)| start <= byte && byte < end)
            })
        })
    }
}
