//! As linhas do arquivo de imports da pasta (`imports_file`) que valem no
//! tipo dos arquivos da mesma língua da pasta dele e das de baixo: as de
//! `folder` (`@inject`, `@inherits`, `@namespace`). As de membro se somam, da
//! pasta de cima para a de baixo; das de cabeça e de base, vale a do arquivo
//! de imports mais perto, e a do próprio arquivo, com o mesmo marcador, vence
//! as herdadas. O resto herdado do marcador de `folder_path` ganha os nomes
//! das pastas entre a do arquivo de imports e a do arquivo.

use super::{LineKind, Markup, Piece};
use crate::graph::{folder_of, is_under};

impl Markup {
    /// As linhas herdadas pelo arquivo `path`, cujos trechos são `pieces`,
    /// dos arquivos de imports `folder` (o caminho e o texto de cada um, da
    /// pasta de cima para a de baixo), com o que cada uma é e o código dela.
    pub(super) fn inherited(&self, pieces: &[Piece], path: &str, folder: &[(&str, &str)]) -> Vec<(LineKind, String)> {
        if self.folder.is_empty() {
            return Vec::new();
        }
        let own: Vec<&str> = pieces
            .iter()
            .filter_map(|piece| match piece {
                Piece::Line { marker, .. } => Some(*marker),
                _ => None,
            })
            .collect();
        let [path_marker, joiner] = self.folder_path;
        let mut found: Vec<(&str, LineKind, String)> = Vec::new();
        for (imports, text) in folder {
            let below = folder_of(path).strip_prefix(folder_of(imports)).unwrap_or_default();
            let folders: Vec<&str> = below.split('/').filter(|name| !name.is_empty()).collect();
            for piece in self.pieces(text) {
                let Piece::Line { marker, argument, code, kind, .. } = piece else { continue };
                if !self.folder.contains(&marker) || own.contains(&marker) {
                    continue;
                }
                let code = if marker == path_marker && !folders.is_empty() {
                    let form = self.head.iter().chain(self.lines).find(|(m, _)| *m == marker).map_or("{}", |(_, form)| *form);
                    form.replacen("{}", &format!("{argument}{joiner}{}", folders.join(joiner)), 1)
                } else {
                    code
                };
                if !matches!(kind, LineKind::Member) {
                    found.retain(|(m, ..)| *m != marker);
                }
                if !found.iter().any(|(_, _, known)| *known == code) {
                    found.push((marker, kind, code));
                }
            }
        }
        found.into_iter().map(|(_, kind, code)| (kind, code)).collect()
    }
}

/// Os arquivos de imports da pasta que alcançam o arquivo `path` da língua
/// `language`, entre os `files` (o caminho, a língua e o texto de cada um):
/// os da mesma língua cuja pasta o contém, sem ele mesmo, da pasta de cima
/// para a de baixo, com o caminho e o texto.
pub(crate) fn imports_above<'a>(files: &'a [(String, String, String)], path: &str, language: &str) -> Vec<(&'a str, &'a str)> {
    let mut above: Vec<(&str, &str)> = files
        .iter()
        .filter(|(imports, lang, _)| lang == language && imports != path && is_under(path, folder_of(imports)))
        .map(|(imports, _, text)| (imports.as_str(), text.as_str()))
        .collect();
    above.sort_by_key(|(imports, _)| folder_of(imports).matches('/').count() + usize::from(!folder_of(imports).is_empty()));
    above
}
