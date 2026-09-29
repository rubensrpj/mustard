//! O membro escrito depois do objeto, sem chamada ali: a propriedade ou o
//! campo lido (`pedido.Total`, `self.total`, `@Model.Total`) ou escrito
//! (`pedido.Total = 3`). O nome ([`crate::model::Module::member_reads`]) liga
//! só a uma propriedade ou a um campo, e pelo mesmo caminho da chamada de
//! método escrita depois do mesmo objeto: o que vem escrito antes dele
//! estreita, e sem estreitar ele fica suspeito entre o que o arquivo tem à
//! vista.

use super::{Before, DeclId, Verdict};

/// Os tipos de declaração que um membro escrito depois do objeto alcança: o
/// que se lê e se escreve sem chamar.
pub(super) const KINDS: &[&str] = &["property", "field"];

/// O que o membro escrito depois do objeto alcança, entre as declarações com
/// esse nome de [`KINDS`]:
///
/// - nenhuma quando o nome é de fora do projeto (`not_ours`), como o
///   `DateTime.Now` da biblioteca;
/// - escrito depois do próprio objeto ou de um nome (`this.Total`,
///   `pedido.Total`), as que esse qualificador estreita (`narrowed`); sem
///   estreitar, as que o arquivo tem à vista (`seen`), sempre suspeitas, como
///   a chamada de método escrita depois do mesmo nome; na língua em que o
///   nome antes do separador só junta caminho (`path_only`), o nome que não
///   estreita é módulo ou tipo de fora, e não liga;
/// - escrito sozinho, as que o arquivo tem à vista, provada quando é uma só;
///   dentro da desestruturação de um objeto (`destructured`, na língua em que
///   o nome sozinho não é membro do próprio tipo), o membro é lido de um
///   objeto que o arquivo não nomeia, e fica suspeito como o escrito depois de
///   um nome que não estreita;
/// - escrito depois de um valor (`Outro().Total`, `pedido.total` na língua
///   cujo separador de membro só liga valor), nenhuma: sem saber o tipo do
///   valor, nada diz de quem é o membro.
///
/// Nunca a família inteira da língua, como a chamada que não acha nada à
/// vista: o membro lido só liga ao que o arquivo enxerga.
pub(super) fn verdict(
    not_ours: bool,
    before: &Before,
    seen: Vec<DeclId>,
    narrowed: impl FnOnce() -> Option<(Vec<DeclId>, bool)>,
    path_only: bool,
    destructured: bool,
    max_same_name: usize,
) -> Option<Verdict> {
    match before {
        _ if not_ours => None,
        Before::Nothing => Verdict::of(seen, !destructured, max_same_name),
        Before::Itself | Before::Name(_) => match narrowed() {
            Some((kept, provable)) => Verdict::of(kept, provable, max_same_name),
            None if matches!(before, Before::Name(_)) && path_only => None,
            None => Verdict::of(seen, false, max_same_name),
        },
        Before::Value | Before::Chain(_) => None,
    }
}
