//! A soma que escreve o caminho de uma chamada da tela, pedaço por pedaço
//! (`'/pedidos/' + id + '/itens'`). A consulta da regra dá cada soma
//! (`client.sum`), o sinal de somar dela (`client.sum.plus`) e o pedaço que é
//! texto escrito ali (`client.sum.text`); este módulo separa os pedaços pelos
//! sinais, da esquerda para a direita, sem nome de nó.

use std::collections::HashSet;

use tree_sitter::Node;

/// Até onde se segue uma soma dentro de outra; mais fundo que isso, o caminho
/// fica sem chamada.
const MAX_SUM_DEPTH: usize = 64;

/// As somas do arquivo, os sinais de somar e os pedaços que são texto, cada
/// um pelo trecho que o escreve.
#[derive(Default)]
pub(super) struct Sums {
    sums: HashSet<(usize, usize)>,
    plus: HashSet<(usize, usize)>,
    texts: HashSet<(usize, usize)>,
}

/// Um pedaço da soma: o texto escrito ali ou um valor.
enum Piece {
    Text(String),
    Value,
}

fn span(node: Node) -> (usize, usize) {
    (node.start_byte(), node.end_byte())
}

impl Sums {
    /// Guarda a soma escrita em `node`.
    pub(super) fn sum(&mut self, node: Node) {
        self.sums.insert(span(node));
    }

    /// Guarda o sinal de somar escrito em `node`.
    pub(super) fn plus(&mut self, node: Node) {
        self.plus.insert(span(node));
    }

    /// Guarda o pedaço de soma que é texto escrito em `node`.
    pub(super) fn text(&mut self, node: Node) {
        self.texts.insert(span(node));
    }

    /// O caminho que a soma escrita em `node` monta: o texto de cada pedaço
    /// que é texto (`literal`) e `param` no lugar de cada valor. `None` quando
    /// o primeiro pedaço que escreve algo é um valor — o caminho montado
    /// sobre um valor, como a base guardada numa variável — ou quando `node`
    /// não é uma soma que a consulta deu.
    pub(super) fn path(&self, node: Node, literal: &dyn Fn(Node) -> String, param: &str) -> Option<String> {
        let mut pieces = Vec::new();
        self.pieces(node, literal, &mut pieces, 0)?;
        let first = pieces.iter().find(|piece| !matches!(piece, Piece::Text(text) if text.is_empty()))?;
        if matches!(first, Piece::Value) {
            return None;
        }
        Some(
            pieces
                .into_iter()
                .map(|piece| match piece {
                    Piece::Text(text) => text,
                    Piece::Value => param.to_string(),
                })
                .collect(),
        )
    }

    /// Os pedaços da soma `node`, em `out`: o que fica entre dois sinais de
    /// somar é um pedaço. O pedaço feito de um nó só que é outra soma dá os
    /// pedaços dela; o que é texto, o texto; todo outro, um valor.
    fn pieces(&self, node: Node, literal: &dyn Fn(Node) -> String, out: &mut Vec<Piece>, depth: usize) -> Option<()> {
        if depth > MAX_SUM_DEPTH || !self.sums.contains(&span(node)) {
            return None;
        }
        let mut operand: Vec<Node> = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if self.plus.contains(&span(child)) {
                self.operand(&operand, literal, out, depth)?;
                operand.clear();
            } else if child.is_named() && !child.is_extra() {
                operand.push(child);
            }
        }
        self.operand(&operand, literal, out, depth)
    }

    /// O pedaço feito dos nós `nodes`, em `out`.
    fn operand(&self, nodes: &[Node], literal: &dyn Fn(Node) -> String, out: &mut Vec<Piece>, depth: usize) -> Option<()> {
        match nodes {
            [one] if self.sums.contains(&span(*one)) => self.pieces(*one, literal, out, depth + 1),
            [one] if self.texts.contains(&span(*one)) => {
                out.push(Piece::Text(literal(*one)));
                Some(())
            }
            _ => {
                out.push(Piece::Value);
                Some(())
            }
        }
    }
}
