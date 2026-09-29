//! As linhas do arquivo de imports da pasta (`imports_file`) que valem no
//! tipo dos arquivos da mesma língua da pasta dele e das de baixo: as de
//! `folder` (`@inject`, `@inherits`, `@namespace`). As de membro se somam, da
//! pasta de cima para a de baixo; das de cabeça e de base, vale a do arquivo
//! de imports mais perto, e a do próprio arquivo, com o mesmo marcador, vence
//! as herdadas. O resto herdado do marcador de `folder_path` ganha os nomes
//! das pastas entre a do arquivo de imports e a do arquivo. O projeto do
//! manifesto de `folder_root` escreve esse marcador com o namespace que o
//! manifesto declara (`namespace_pattern`) ou, sem ele, com o nome do projeto,
//! como um arquivo de imports na pasta do manifesto: sem `@namespace` na
//! página nem num arquivo de imports acima dela, a página de
//! `Web/Pages/Admin/` do `Web/Web.csproj` fica em `Web.Pages.Admin`, ou em
//! `Loja.Web.Pages.Admin` quando o manifesto declara `Loja.Web`.

use super::{LineKind, Markup, Piece};
use crate::graph::{folder_of, is_under};
use crate::model::Manifest;

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

    /// A linha que o projeto de cada manifesto de `manifests` do tipo de
    /// `folder_root` escreve na pasta dele, com o caminho do manifesto: o
    /// marcador de `folder_path` seguido do namespace que o manifesto declara
    /// ou, sem ele, do nome do projeto, cada parte dele com o que não é
    /// letra, algarismo nem `_` trocado por `_` (`@namespace Loja.Web` do
    /// `Loja.Web.csproj`, ou o declarado dentro dele). Vazio sem
    /// `folder_root`.
    pub(crate) fn project_lines(&self, manifests: &[Manifest]) -> Vec<(String, String)> {
        let [marker, joiner] = self.folder_path;
        if self.folder_root.is_empty() {
            return Vec::new();
        }
        let part = |text: &str| -> String { text.chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' }).collect() };
        manifests
            .iter()
            .filter(|manifest| manifest.kind == self.folder_root && !manifest.name.is_empty())
            .map(|manifest| {
                let declared = manifest.namespace.as_deref().filter(|namespace| !namespace.is_empty());
                let name: Vec<String> = declared.unwrap_or(&manifest.name).split(joiner).map(part).collect();
                (manifest.path.clone(), format!("{marker} {}", name.join(joiner)))
            })
            .collect()
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
