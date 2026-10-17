//! O resumo do conteúdo dos ganchos do plugin: o que o anfitrião carrega das
//! pastas `hooks/` e `ui/` (os `.js` e os `.json`). Os ganchos importam a
//! `ui/` pelo caminho relativo, e por isso ela entra na conta.
//!
//! O número da versão não basta para dizer que o plugin instalado é o desta
//! compilação: o desenvolvimento repete o número enquanto o código muda, e a
//! cópia em cache do anfitrião fica com os ganchos velhos sob o mesmo número.
//! O resumo cobre o caminho e o conteúdo de cada arquivo, em ordem de caminho,
//! com o fim de linha `\r\n` lido como `\n`, para a cópia feita no Windows dar
//! o mesmo resumo.
//!
//! O arquivo só usa a `std` e o `sha256` ao lado dele: o `build.rs` do
//! `mustard-rt` o inclui por caminho para gravar na compilação o resumo dos
//! ganchos que ela entrega, e a conferência da instalação mede a cópia
//! instalada pela mesma conta. Por isso nada aqui usa o resto do núcleo, nem
//! recurso mais novo que a edição 2015.

use super::sha256::Sha256;

/// As pastas do plugin que o anfitrião carrega quando roda os ganchos.
pub const HOOK_FOLDERS: [&str; 2] = ["hooks", "ui"];

/// O resumo dos ganchos do plugin com raiz em `plugin`, ou `None` quando
/// nenhuma das pastas tem gancho nenhum. Uma pasta que falta só não entra na
/// conta: a cópia sem ela dá outro resumo que a compilação com ela.
#[must_use]
pub fn hooks_digest(plugin: &std::path::Path) -> Option<String> {
    let mut files = Vec::new();
    for folder in HOOK_FOLDERS {
        let Ok(entries) = std::fs::read_dir(plugin.join(folder)) else {
            continue;
        };
        for entry in entries {
            let path = entry.ok()?.path();
            let shipped = path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext == "js" || ext == "json");
            if shipped && path.is_file() {
                files.push(format!("{folder}/{}", path.file_name()?.to_str()?));
            }
        }
    }
    if files.is_empty() {
        return None;
    }
    files.sort();
    let mut hasher = Sha256::new();
    for name in &files {
        let body = std::fs::read(plugin.join(name)).ok()?;
        hasher.update(name.as_bytes());
        hasher.update(&[0]);
        let mut text = Vec::with_capacity(body.len());
        for (index, byte) in body.iter().enumerate() {
            if !(*byte == b'\r' && body.get(index + 1) == Some(&b'\n')) {
                text.push(*byte);
            }
        }
        hasher.update(&text);
        hasher.update(&[0]);
    }
    Some(hasher.hex_digest())
}
