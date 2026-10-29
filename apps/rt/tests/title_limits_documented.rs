//! Os dois tetos de título, ditos onde quem grava os lê: a ajuda do
//! `run write`. O de 70 é do nome de um item; o de 60 é do título de um
//! commit e de um pull request, e o título do pull request é o `title` do
//! objetivo ou, sem `title`, a primeira frase dele.

#[path = "support/binary.rs"]
mod binary;

/// O texto com cada sequência de espaços e quebras de linha numa só: a ajuda
/// quebra as linhas na largura do terminal.
fn flat(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A ajuda do `run write`, pelo binário, diz de que é cada teto: o de 70 é do
/// nome do item, e o de 60, do título de um commit e de um pull request, cujo
/// título é o `title` do objetivo ou, sem `title`, a primeira frase dele. Quem gravava um objetivo com `title` curto lia que o
/// teto era da primeira frase e ia encurtar a parte errada.
#[test]
fn the_help_says_what_each_title_limit_is_of() {
    let out = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt")).args(["run", "write", "--help"]).output().expect("run mustard-rt");
    assert!(out.status.success(), "run write --help failed: {}", String::from_utf8_lossy(&out.stderr));
    let help = flat(&String::from_utf8_lossy(&out.stdout));
    assert!(help.contains("the item's own name, up to 70 characters"), "the 70 is the item's: {help}");
    assert!(
        help.contains("the pull request title is the goal's `title` or, without one, the goal's first sentence"),
        "the 60 is the goal's title, or its first sentence without one: {help}",
    );
}
