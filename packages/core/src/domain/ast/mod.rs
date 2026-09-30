//! `ast` — a única coisa que sobrou da camada de árvore de sintaxe: a
//! pergunta "este caminho é de teste?" e, sendo, "que nome ele testa?"; a
//! pergunta "este arquivo é o de entrada da pasta dele?"; e a pergunta "este
//! arquivo é um módulo que o outro declara dentro de si?".
//!
//! A camada nasceu em cima do `tree-sitter`, para o portão de regressão ler o
//! corpo das funções tocadas. Esse portão saiu, e com ele o carregador de
//! gramáticas, o parser, as consultas `.scm`, o extrator de assinaturas, o
//! extrator de entidades e o detector de esqueleto: ninguém mais os chamava.
//!
//! O que restou tem quatro chamadores vivos — o mapa de testes do scan, o
//! resumo do scan, o mapa do projeto e a prova de recuperação — e todos os
//! quatro pedem a mesma função. O mapa de testes pede ainda o nome testado e
//! os marcadores de teste dentro do arquivo, que saem dos mesmos dados de
//! convenção (`test-files.toml`, ao lado do módulo). A importação do scan e o
//! padrão do projeto pedem os nomes do arquivo que responde pela pasta em cada
//! língua, de outro arquivo de dados ao lado (`entry-files.toml`). A
//! conferência de ciclos de importação depois da onda pede se um arquivo é
//! módulo que o outro declara dentro de si, pelas línguas de um terceiro
//! arquivo de dados (`nested-modules.toml`). Por isso o módulo carrega só
//! essas seis, e nada mais: nenhuma dependência de
//! gramática, nenhum tipo público que ninguém constrói.

pub mod conventions;

pub use conventions::{entry_file_names, inline_test_markers, is_declared_child, is_entry_file, is_test_path, tested_name};

#[cfg(test)]
mod tests {
    /// A peça que sobrou não arrasta gramática nenhuma.
    ///
    /// Enquanto o carregador de gramáticas morava aqui, este pacote compilava
    /// sete gramáticas de linguagem e o carregador do tree-sitter só para
    /// responder se um caminho é de teste. O defeito que este teste pega é a
    /// dependência voltar sem que nada a use: a compilação continua passando,
    /// só fica cara, e o motivo de ela existir some do texto.
    #[test]
    fn o_pacote_nao_depende_mais_de_gramatica_nenhuma() {
        let manifesto = std::fs::read_to_string(
            crate::manifest_dir::manifest_dir().join("Cargo.toml"),
        )
        .expect("o Cargo.toml do próprio pacote precisa ser legível");
        for linha in manifesto.lines() {
            let declaracao = linha.trim();
            assert!(
                !declaracao.starts_with("tree-sitter"),
                "o pacote voltou a declarar '{declaracao}', e nada aqui usa gramática",
            );
        }
    }
}
