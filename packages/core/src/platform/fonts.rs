//! As pastas de fonte da máquina: onde o diagnóstico procura uma Nerd Font e
//! onde o instalador de fonte confere se a família pedida já está instalada.
//! Uma lista só, para as duas perguntas nunca olharem pastas diferentes.
//!
//! A pasta de fontes do usuário parte da pasta pessoal lida por
//! [`home_dir`]: sem ela, a lista fica só com as pastas do sistema, nunca com
//! um caminho relativo à pasta em que o comando roda.

use std::path::PathBuf;

use super::harness::home_dir;

/// A pasta de fontes do usuário no Linux, sob a pasta pessoal: onde o
/// instalador grava a fonte baixada. `None` sem pasta pessoal.
#[must_use]
pub fn linux_user_font_dir() -> Option<PathBuf> {
    home_dir().map(|home| home.join(".local").join("share").join("fonts"))
}

/// As pastas de fonte da plataforma em que o programa roda: primeiro a do
/// usuário, depois a do sistema. No Windows, a pasta do usuário sai de
/// `LOCALAPPDATA`, e a variável vazia vale como ausente.
///
/// `cfg!()` em vez do atributo `#[cfg]` mantém os três ramos compilados em
/// toda plataforma, e um erro no ramo de outro sistema aparece em qualquer
/// compilação.
#[must_use]
pub fn font_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if cfg!(target_os = "windows") {
        if let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|dir| !dir.is_empty()) {
            dirs.push(PathBuf::from(local).join("Microsoft").join("Windows").join("Fonts"));
        }
        dirs.push(PathBuf::from("C:/Windows/Fonts"));
    } else if cfg!(target_os = "macos") {
        dirs.extend(home_dir().map(|home| home.join("Library").join("Fonts")));
        dirs.push(PathBuf::from("/Library/Fonts"));
    } else if cfg!(target_os = "linux") {
        dirs.extend(linux_user_font_dir());
        dirs.push(PathBuf::from("/usr/share/fonts"));
    }
    dirs
}
