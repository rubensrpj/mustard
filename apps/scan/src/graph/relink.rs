//! A ligação da passada que relê só alguns arquivos: as chamadas e as
//! citações que não podem ter mudado ficam com o que o mapa anterior ligou, e
//! só as outras se resolvem de novo. O mapa sai igual ao da passada que liga
//! o projeto inteiro.
//!
//! O que se resolve de novo:
//!
//! - toda chamada e toda citação de um arquivo lido agora, de um arquivo cujas
//!   importações resolvidas ou cujos trechos de teste mudaram, e de um arquivo
//!   que escreve um nome cujas declarações mudaram de tipo ou de dono em algum
//!   arquivo lido (o nome que passou a ser declarado ou deixou de ser conta);
//! - em todo arquivo, a chamada e a citação que procuram um desses nomes;
//! - em todo arquivo, a chamada que procura um nome que tinha chamada comum
//!   demais contada no mapa anterior, quando um arquivo que se resolve de novo
//!   podia procurá-lo: a contagem não diz de onde veio cada chamada, e se
//!   refaz inteira.
//!
//! O resto vem do mapa anterior: o uso de cada declaração escrito por um
//! arquivo que não se resolve de novo, com a linha de agora da declaração do
//! arquivo lido que só mudou de lugar, e o nome que cada declaração chama.
//!
//! A passada liga o projeto inteiro, como sem mapa anterior, quando o que um
//! arquivo lido muda alcança a resolução dos outros por um caminho que não é
//! um nome escrito: entrou ou saiu arquivo, mudaram os manifestos ou o teto do
//! nome comum, ou um arquivo lido mudou a língua, os namespaces, os repasses,
//! os imports globais, os tipos e os campos que dão o tipo de um receptor, ou
//! o nome que um repasse troca.

use std::collections::{BTreeSet, HashMap, HashSet};

use super::reach::FIELD_KINDS;
use super::TYPE_KINDS;
use crate::model::{CallSite, Decl, Manifest, Module, ProjectModel, UseSite};

/// O mapa anterior e os arquivos que a passada leu agora, que são os que
/// mudaram e os que a mudança fez reler.
pub(crate) struct Previous<'a> {
    pub(crate) model: &'a ProjectModel,
    pub(crate) read: &'a [String],
}

/// O que a passada liga de novo.
pub(super) struct Plan<'p> {
    /// Por posição em `modules`: o arquivo cujas chamadas e citações se
    /// resolvem todas de novo.
    sources: Vec<bool>,
    /// O caminho de cada um desses arquivos.
    source_paths: HashSet<String>,
    /// Os nomes cujas declarações recebem de novo todos os usos e cujas
    /// chamadas e citações se resolvem de novo em todo arquivo.
    renewed: HashSet<String>,
    /// Algum nome que um repasse troca está em `renewed`: o nome trazido
    /// escrito sozinho pode procurá-lo.
    renamed_renewed: bool,
    /// Cada declaração de um arquivo lido, pela posição, com a mesma
    /// declaração no mapa anterior, que guarda os usos que ela recebeu.
    carried: HashMap<(usize, usize), &'p Decl>,
    /// A linha de agora de cada declaração de um arquivo lido, pelo arquivo,
    /// pela linha do mapa anterior e pelo nome.
    moved: HashMap<(&'p str, usize, &'p str), usize>,
}

impl Plan<'_> {
    /// As chamadas e as citações do arquivo `src` se resolvem todas de novo.
    pub(super) fn whole(&self, src: usize) -> bool {
        self.sources[src]
    }

    /// O arquivo `m`, na posição `src`, tem alguma chamada ou citação a
    /// resolver de novo. Sem nenhuma, nem se prepara a leitura dele.
    pub(super) fn reads(&self, m: &Module, src: usize) -> bool {
        self.whole(src)
            || sites(m).any(|site| {
                self.renewed.contains(&site.name)
                    || (site.qualifier.is_empty()
                        && m.brought.values().filter_map(|names| names.get(&site.name)).any(|original| {
                            self.renamed_renewed || self.renewed.contains(original)
                        }))
            })
    }

    /// A chamada ou a citação do arquivo `src` que procura as declarações
    /// pelo nome `looked` se resolve de novo.
    pub(super) fn relinks(&self, src: usize, looked: &str) -> bool {
        self.whole(src) || self.renewed.contains(looked)
    }

    /// Os nomes que a declaração chama: os que o mapa anterior deu
    /// (`before`), tirados os que se resolveram de novo, e os de agora.
    pub(super) fn calls(&self, before: &[String], now: BTreeSet<String>) -> Vec<String> {
        let kept = before.iter().filter(|name| !self.renewed.contains(*name)).cloned();
        let all: BTreeSet<String> = kept.chain(now).collect();
        all.into_iter().collect()
    }

    /// O que a declaração `decl`, na posição `(mi, di)`, guarda do mapa
    /// anterior: os usos escritos por um arquivo cujas chamadas não se
    /// resolvem de novo, com a linha de agora de cada candidata, e as
    /// chamadas comuns demais contadas. `None` quando ela recebe de novo
    /// todos os usos.
    pub(super) fn kept(&self, (mi, di): (usize, usize), decl: &Decl) -> Option<(Vec<UseSite>, usize)> {
        if self.renewed.contains(&decl.name) {
            return None;
        }
        let before = self.carried.get(&(mi, di)).copied().unwrap_or(decl);
        let uses = before
            .used_by
            .iter()
            .filter(|site| !self.source_paths.contains(&site.file))
            .map(|site| {
                let mut site = site.clone();
                for at in &mut site.candidates {
                    if let Some(&line) = self.moved.get(&(at.file.as_str(), at.line, at.name.as_str())) {
                        at.line = line;
                    }
                }
                site.candidates.sort();
                site
            })
            .collect();
        Some((uses, before.common_calls))
    }
}

/// O que a passada liga de novo, dado o mapa anterior. `None` quando ela liga
/// o projeto inteiro.
pub(super) fn plan<'p>(
    modules: &[Module],
    manifests: &[Manifest],
    max_same_name: usize,
    previous: Previous<'p>,
) -> Option<Plan<'p>> {
    let prev = previous.model;
    if prev.state.max_same_name != max_same_name || prev.modules.len() != modules.len() {
        return None;
    }
    let before: HashMap<&str, &'p Module> = prev.modules.iter().map(|m| (m.path.as_str(), m)).collect();
    let old: Vec<&'p Module> = modules.iter().map(|m| before.get(m.path.as_str()).copied()).collect::<Option<_>>()?;
    if serde_json::to_value(&prev.manifests).ok()? != serde_json::to_value(manifests).ok()? {
        return None;
    }
    let read: HashSet<&str> = previous.read.iter().map(String::as_str).collect();
    // As famílias em que um repasse pode passar por um namespace, cuja
    // resolução lê os nomes que o arquivo usa e as declarações dos arquivos
    // do namespace.
    let namespaced: HashSet<&str> =
        modules.iter().filter(|m| !m.namespaces.is_empty()).map(|m| crate::extract::family(&m.language)).collect();
    let passing_namespaced: HashSet<&str> = modules
        .iter()
        .filter(|m| !m.reexports.is_empty())
        .map(|m| crate::extract::family(&m.language))
        .filter(|family| namespaced.contains(family))
        .collect();
    // O nome que um repasse oferece com outro nome: quem o importa procura a
    // declaração pelo nome de origem.
    let renamed_originals: HashSet<&str> = modules
        .iter()
        .flat_map(|m| m.reexports.values())
        .flat_map(|offered| offered.iter().filter(|(name, original)| name != original).map(|(_, original)| original.as_str()))
        .collect();

    let mut reshaped: HashSet<String> = HashSet::new();
    let mut carried: HashMap<(usize, usize), &'p Decl> = HashMap::new();
    let mut moved: HashMap<(&'p str, usize, &'p str), usize> = HashMap::new();
    for (mi, (now, was)) in modules.iter().zip(&old).enumerate().filter(|(_, (m, _))| read.contains(m.path.as_str())) {
        if !same_outline(was, now) || !same_types(was, now) {
            return None;
        }
        let (kept, differ) = paired(was, now);
        if passing_namespaced.contains(crate::extract::family(&now.language)) && (!now.reexports.is_empty() || !differ.is_empty()) {
            return None;
        }
        for (old_at, new_at) in kept {
            let decl = &was.declarations[old_at];
            carried.insert((mi, new_at), decl);
            moved.insert((was.path.as_str(), decl.line, decl.name.as_str()), now.declarations[new_at].line);
        }
        reshaped.extend(differ);
    }
    if reshaped.iter().any(|name| renamed_originals.contains(name.as_str())) {
        return None;
    }

    let sources: Vec<bool> = modules
        .iter()
        .zip(&old)
        .map(|(now, was)| {
            read.contains(now.path.as_str())
                || now.deps != was.deps
                || now.test_lines != was.test_lines
                || (!reshaped.is_empty() && mentions(now, &reshaped))
        })
        .collect();
    // A chamada comum demais que um arquivo que se resolve de novo fazia, e
    // que a contagem do mapa anterior guarda sem dizer de onde veio: o nome
    // que ela procurava se refaz em todo arquivo. A que ele faz agora só soma.
    // Sem contagem nenhuma do nome no mapa anterior, nenhuma veio dele.
    let counted: HashSet<&str> = prev
        .modules
        .iter()
        .flat_map(|m| &m.declarations)
        .filter(|d| d.common_calls > 0)
        .map(|d| d.name.as_str())
        .collect();
    let mut renewed = reshaped;
    for (was, _) in old.iter().zip(&sources).filter(|(_, whole)| **whole) {
        let mut asked: Vec<&str> = sites(was).map(|site| site.name.as_str()).collect();
        asked.extend(was.brought.values().flatten().flat_map(|(name, original)| [name.as_str(), original.as_str()]));
        if !was.brought.is_empty() {
            asked.extend(renamed_originals.iter().copied());
        }
        renewed.extend(asked.into_iter().filter(|name| counted.contains(name)).map(str::to_string));
    }
    let renamed_renewed = renamed_originals.iter().any(|name| renewed.contains(*name));
    let source_paths = modules.iter().zip(&sources).filter(|(_, whole)| **whole).map(|(m, _)| m.path.clone()).collect();
    Some(Plan { sources, source_paths, renewed, renamed_renewed, carried, moved })
}

/// As declarações de um arquivo lido que seguem as mesmas do mapa anterior,
/// cada par pela posição antes e agora, e os nomes cujas declarações mudaram
/// no que a resolução dos outros arquivos lê delas: o tipo, os donos e se
/// valem só dentro de uma função, na ordem em que estão escritas. O nome que
/// passou a ser declarado, ou deixou de ser, muda.
fn paired(was: &Module, now: &Module) -> (Vec<(usize, usize)>, BTreeSet<String>) {
    let (before, after) = (shapes(was), shapes(now));
    let mut kept = Vec::new();
    let mut differ = BTreeSet::new();
    for name in before.keys().chain(after.keys()) {
        match (before.get(name), after.get(name)) {
            (Some(old), Some(new)) if old.iter().map(|(_, shape)| shape).eq(new.iter().map(|(_, shape)| shape)) => {
                kept.extend(old.iter().zip(new).map(|((old_at, _), (new_at, _))| (*old_at, *new_at)));
            }
            _ => {
                differ.insert((*name).to_string());
            }
        }
    }
    kept.sort_unstable();
    kept.dedup();
    (kept, differ)
}

/// O jeito de uma declaração que a resolução dos outros arquivos lê: o tipo,
/// os donos e se vale só dentro de uma função.
type Shape<'d> = (&'d str, &'d [String], bool);

/// As declarações de `m` por nome, cada uma com a posição e o jeito, na
/// ordem em que estão escritas.
fn shapes(m: &Module) -> HashMap<&str, Vec<(usize, Shape<'_>)>> {
    let mut by_name: HashMap<&str, Vec<(usize, Shape<'_>)>> = HashMap::new();
    for (at, d) in m.declarations.iter().enumerate() {
        let local = !d.syntax["lexical_scope"].is_null();
        by_name.entry(d.name.as_str()).or_default().push((at, (d.kind.as_str(), d.owner.as_slice(), local)));
    }
    by_name
}

/// Cada nome que o arquivo escreve e que a ligação procura: o chamado, o
/// citado, o escrito onde vai um valor e o membro lido depois do objeto.
fn sites(m: &Module) -> impl Iterator<Item = &CallSite> {
    m.calls.iter().chain(&m.cites).chain(&m.value_uses).chain(&m.member_reads)
}

/// O que a resolução de um arquivo lê dos outros sem ser pelas declarações:
/// a língua, os namespaces, os repasses e os imports globais, com quantos
/// módulos do arquivo cada um tem em volta.
fn same_outline(was: &Module, now: &Module) -> bool {
    let depths = |m: &Module| -> Vec<(String, BTreeSet<usize>)> {
        m.reexports.keys().chain(&m.global_imports).map(|imp| (imp.clone(), m.import_depths(imp, false))).collect()
    };
    was.language == now.language
        && was.namespaces == now.namespaces
        && was.reexports == now.reexports
        && was.global_imports == now.global_imports
        && depths(was) == depths(now)
}

/// O que o tipo de um receptor lê do arquivo: os tipos, os campos com a
/// assinatura, quantos de cada, e, no arquivo que declara tipo, o que ele
/// importa.
fn same_types(was: &Module, now: &Module) -> bool {
    let types = |m: &Module| -> Vec<(String, String)> {
        let mut all: Vec<(String, String)> = m
            .declarations
            .iter()
            .filter(|d| TYPE_KINDS.contains(&d.kind.as_str()))
            .map(|d| (d.name.clone(), d.kind.clone()))
            .collect();
        all.sort();
        all
    };
    let fields = |m: &Module| -> Vec<(String, Vec<String>, String)> {
        let mut all: Vec<(String, Vec<String>, String)> = m
            .declarations
            .iter()
            .filter(|d| FIELD_KINDS.contains(&d.kind.as_str()))
            .map(|d| (d.name.clone(), d.owner.clone(), d.signature.clone()))
            .collect();
        all.sort();
        all
    };
    let (before, after) = (types(was), types(now));
    before == after && fields(was) == fields(now) && (after.is_empty() || was.deps == now.deps)
}

/// O arquivo escreve algum dos nomes `names`: chamado, citado, antes de outro
/// nome, abrindo um caminho, ou trazido por um import.
fn mentions(m: &Module, names: &HashSet<String>) -> bool {
    let has = |text: &str| text.split(|c: char| !(c.is_alphanumeric() || c == '_')).any(|word| names.contains(word));
    sites(m).any(|site| names.contains(&site.name) || has(&site.qualifier))
        || m.unbound_heads.iter().any(|head| names.contains(head))
        || m.brought.values().flatten().any(|(name, original)| names.contains(name) || names.contains(original))
        || m.call_paths.keys().chain(m.other_call_paths.keys()).any(|path| has(path))
}
