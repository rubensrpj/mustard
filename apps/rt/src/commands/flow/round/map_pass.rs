//! A passada do mapa em segundo plano depois do commit da rodada: a rodada a
//! pede e segue, e a passada segura a trava própria e repete enquanto o mapa
//! estiver atrás do commit.

use std::path::Path;
#[cfg(not(test))]
use std::process::Stdio;

use mustard_core::domain::scan::ScanReport;
use mustard_core::io::fs::lock::LockedFile;

use super::commit::{installed_scan_format, refresh_map};

#[cfg(test)]
thread_local! {
    /// Os projetos cuja passada esta linha de execução pediu e que ainda não
    /// rodou ([`PassesAfterTheRound`]).
    static PENDING: std::cell::RefCell<Vec<std::path::PathBuf>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Nos testes da biblioteca, a passada que a rodada pediu roda quando a
/// rodada termina, com o scan da própria rodada (`mine`): é o mapa que a
/// passada em segundo plano deixaria pronto antes da rodada seguinte.
#[cfg(test)]
pub(super) struct PassesAfterTheRound<'a> {
    pub(super) mine: &'a dyn Fn(&Path, &Path) -> mustard_core::platform::error::Result<ScanReport>,
}

#[cfg(test)]
impl Drop for PassesAfterTheRound<'_> {
    fn drop(&mut self) {
        let mut roots = PENDING.with(|pending| pending.take());
        roots.dedup();
        for root in roots {
            refresh_while_behind_with(&root, self.mine, &installed_scan_format);
        }
    }
}

/// O ambiente que diz ao `run scan` solto por [`start_map_refresh`] que ele
/// é a passada em segundo plano da rodada ([`refresh_map_while_behind`]).
pub(crate) const BACKGROUND_PASS_ENV: &str = "MUSTARD_MAP_BEHIND";

/// O arquivo da trava da passada em segundo plano, na pasta das specs.
const MAP_PASS_LOCK_FILE: &str = "map-pass.lock";

/// Quantas passadas seguidas uma passada em segundo plano faz, no máximo,
/// enquanto o mapa segue atrás: um commit novo no meio da passada pede
/// outra, e um projeto que não para de mudar não a prende para sempre.
const MAP_PASSES: usize = 3;

/// Pede a passada do mapa do projeto `root` em segundo plano e segue sem
/// esperar: a passada custa de 20 a 30 s mesmo com um arquivo só, e a onda
/// seguinte não espera por ela. É um `run scan` solto deste mesmo programa,
/// que segura a trava própria da passada e repete enquanto o mapa estiver
/// atrás do commit ([`refresh_map_while_behind`]). Enquanto isso, o pedido
/// da onda não mostra linha do mapa de arquivo que o commit mudou. Nos
/// testes da biblioteca, a passada roda no fim da rodada
/// ([`PassesAfterTheRound`]).
pub(super) fn start_map_refresh(root: &Path) {
    #[cfg(test)]
    PENDING.with(|pending| pending.borrow_mut().push(root.to_path_buf()));
    #[cfg(not(test))]
    {
        let Ok(program) = std::env::current_exe() else { return };
        let mut command = std::process::Command::new(program);
        command.args(["run", "scan", "--root"]).arg(root).env(BACKGROUND_PASS_ENV, "1");
        command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        let _ = crate::shared::proc::spawn_detached(command, root);
    }
}

/// A conferência do mapa antes de montar os pedidos da rodada: quando ele
/// está atrás do commit ou do conteúdo de agora ([`refresh_map_if_stale`]),
/// só solta a passada em segundo plano ([`start_map_refresh`]), e nunca
/// espera por ela.
pub(super) fn start_map_refresh_if_stale(root: &Path) {
    if mustard_core::io::project_map::is_behind(root, &installed_scan_format) {
        start_map_refresh(root);
    }
}

/// A trava da passada do mapa de `root`, esperando a passada em segundo plano
/// que esteja rodando terminar. A rodada a segura da junção das voltas ao
/// commit: a passada nunca lê o disco com a entrega juntada e ainda não
/// comitada, e a conferência depois da onda nunca copia um mapa no meio da
/// gravação. `None` quando a trava não pode ser pega: a rodada segue sem ela.
pub(super) fn pass_lock(root: &Path) -> Option<LockedFile> {
    let paths = mustard_core::ClaudePaths::for_project(root).ok()?;
    LockedFile::exclusive(&paths.spec_dir().join(MAP_PASS_LOCK_FILE)).ok()
}

/// O mapa de `root` em dia agora, para quem não pode seguir sem ele — o
/// fechamento, cuja análise final lê o mapa logo depois do commit: espera a
/// passada em segundo plano que esteja rodando e, com o mapa ainda atrás,
/// relê na hora.
pub(crate) fn refresh_now(root: &Path) {
    let _held = pass_lock(root);
    if mustard_core::io::project_map::is_behind(root, &installed_scan_format) {
        refresh_map(root, &super::answer::scan_mine);
    }
}

/// A passada em segundo plano que [`start_map_refresh`] solta: com a trava
/// própria da passada livre, relê o mapa enquanto ele estiver atrás, até
/// [`MAP_PASSES`] vezes; com ela presa, sai sem ler nada, porque quem a tem
/// repete a passada. Depois de soltá-la, confere de novo: o commit que chegou
/// enquanto a outra soltava a trava ganha a passada dele.
pub(crate) fn refresh_map_while_behind(root: &Path) {
    refresh_while_behind_with(root, &super::answer::scan_mine, &installed_scan_format);
}

/// [`refresh_map_while_behind`] com quem relê o mapa (`mine`) e a marca de
/// formato do scan (`format`) recebidos.
fn refresh_while_behind_with(
    root: &Path,
    mine: &dyn Fn(&Path, &Path) -> mustard_core::platform::error::Result<ScanReport>,
    format: &dyn Fn() -> Option<String>,
) {
    let Ok(paths) = mustard_core::ClaudePaths::for_project(root) else { return };
    let lock = paths.spec_dir().join(MAP_PASS_LOCK_FILE);
    let mut passes = 0;
    while passes < MAP_PASSES {
        let Some(held) = LockedFile::exclusive_if_free(&lock).ok().flatten() else { return };
        while passes < MAP_PASSES && mustard_core::io::project_map::is_behind(root, format) {
            passes += 1;
            if !refresh_map(root, mine) {
                return;
            }
        }
        drop(held);
        if !mustard_core::io::project_map::is_behind(root, format) {
            return;
        }
    }
}


#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use mustard_core::io::project_map;

    use super::*;

    /// O scan de mentira que grava o mapa do commit e do conteúdo de agora,
    /// contando as passadas.
    fn writing(calls: &Cell<usize>) -> impl Fn(&Path, &Path) -> mustard_core::platform::error::Result<ScanReport> + '_ {
        move |root, _| {
            calls.set(calls.get() + 1);
            let now = project_map::listing(root).expect("inside git");
            let state =
                format!(r#"{{"state": {{"head": "{}", "listing": "{}", "base": "{}", "base_tip": "{}"}}}}"#, now.head, now.digest(), now.base.name, now.base.tip);
            project_map::write_text(root, &state).expect("the map is written");
            Ok(ScanReport::default())
        }
    }

    /// A passada em segundo plano relê o mapa atrás do commit uma vez e para
    /// com ele em dia; com a trava da passada presa por outra, sai sem ler
    /// nada, porque a outra repete; e o scan que falha não a prende.
    #[test]
    fn the_background_pass_reads_while_behind_and_leaves_the_lock_holder_to_repeat() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        crate::shared::test_fixture::seeded_repo(root, &[("a.txt", "x")]);
        let calls = Cell::new(0);
        let no_format = || None;

        let held = LockedFile::exclusive(&mustard_core::ClaudePaths::for_project(root).unwrap().spec_dir().join(MAP_PASS_LOCK_FILE)).unwrap();
        refresh_while_behind_with(root, &writing(&calls), &no_format);
        assert_eq!(calls.get(), 0, "another pass holds the lock and repeats");
        drop(held);

        refresh_while_behind_with(root, &writing(&calls), &no_format);
        assert_eq!(calls.get(), 1, "the map behind is read once");
        refresh_while_behind_with(root, &writing(&calls), &no_format);
        assert_eq!(calls.get(), 1, "the map in step is not read again");

        std::fs::write(root.join("a.txt"), "y").unwrap();
        let failing = |_: &Path, _: &Path| {
            calls.set(calls.get() + 1);
            Err(mustard_core::platform::error::Error::check_failed("scan: not found"))
        };
        refresh_while_behind_with(root, &failing, &no_format);
        assert_eq!(calls.get(), 2, "a failing scan stops the pass");
    }
}
