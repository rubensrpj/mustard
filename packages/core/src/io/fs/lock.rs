//! Travas de arquivo: a gravação que precisa ler, decidir e escrever sem que
//! outro processo grave no meio.
//!
//! A trava é a `File::lock` da biblioteca padrão, que funciona igual no Linux,
//! no macOS e no Windows. No Windows a trava é obrigatória e vale por
//! manipulador: enquanto um manipulador segura a trava, nenhum outro — nem do
//! mesmo processo — lê ou escreve o arquivo. Por isso quem trava lê e escreve
//! só pelo próprio [`LockedFile`], nunca pelas funções soltas de [`super`], e
//! quem só lê usa [`read_shared`], que espera a trava de escrita soltar.
//!
//! A troca de conteúdo põe um arquivo novo no caminho. Toda forma de travar
//! confere, depois de receber a trava, que o arquivo travado ainda é o que
//! mora no caminho, e abre o caminho de novo quando não é.
//!
//! Como [`super::real`], este módulo fala com o disco de verdade: uma trava só
//! existe num arquivo real, e não há dublê de teste para ela.

use std::fs::{File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::platform::error::{Error, Result};

/// O arquivo de trava do trabalho `kind` sobre `target`: na pasta temporária,
/// com o nome tirado do caminho inteiro de `target`, para não deixar arquivo
/// no projeto. O mesmo trabalho sobre o mesmo alvo dá sempre o mesmo arquivo.
#[must_use]
pub fn temp_lock_path(kind: &str, target: &Path) -> PathBuf {
    let whole = std::path::absolute(target).unwrap_or_else(|_| target.to_path_buf());
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    whole.hash(&mut hasher);
    std::env::temp_dir().join(format!("mustard-{kind}-{:016x}.lock", hasher.finish()))
}

/// Um arquivo aberto com a trava exclusiva. A trava solta quando o valor sai
/// de cena.
#[derive(Debug)]
pub struct LockedFile {
    file: File,
    /// O caminho travado: a troca de conteúdo põe o arquivo novo nele.
    path: PathBuf,
}

impl LockedFile {
    /// Abre `path` para ler e escrever, criando o arquivo e a pasta quando
    /// faltam, e espera a trava exclusiva.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] quando a pasta não pode ser criada, o arquivo não abre ou
    /// a trava falha.
    pub fn exclusive(path: &Path) -> Result<Self> {
        loop {
            let file = open_creating(path)?;
            file.lock()?;
            if still_at(&file, path)? {
                return Ok(Self { file, path: path.to_path_buf() });
            }
        }
    }

    /// Como [`LockedFile::exclusive`], mas sem esperar: `None` quando outro
    /// manipulador segura a trava agora.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] quando a pasta não pode ser criada, o arquivo não abre ou
    /// a trava falha por outro motivo que não estar presa.
    pub fn exclusive_if_free(path: &Path) -> Result<Option<Self>> {
        loop {
            let file = open_creating(path)?;
            match file.try_lock() {
                Ok(()) if still_at(&file, path)? => return Ok(Some(Self { file, path: path.to_path_buf() })),
                Ok(()) => {}
                Err(std::fs::TryLockError::WouldBlock) => return Ok(None),
                Err(std::fs::TryLockError::Error(e)) => return Err(e.into()),
            }
        }
    }

    /// Como [`LockedFile::exclusive_if_free`], mas espera a trava soltar por
    /// até `patience`: `None` só quando outro manipulador ainda a segura no
    /// fim da espera. Serve a quem grava algo curto e prefere esperar a
    /// gravação do lado terminar a perder a própria.
    ///
    /// # Errors
    ///
    /// Os mesmos de [`LockedFile::exclusive_if_free`].
    pub fn exclusive_within(path: &Path, patience: std::time::Duration) -> Result<Option<Self>> {
        let deadline = std::time::Instant::now() + patience;
        loop {
            if let Some(held) = Self::exclusive_if_free(path)? {
                return Ok(Some(held));
            }
            let now = std::time::Instant::now();
            if now >= deadline {
                return Ok(None);
            }
            std::thread::sleep(LOCK_POLL.min(deadline - now));
        }
    }

    /// Abre `path`, que precisa existir, para ler e escrever, e espera a trava
    /// exclusiva. Não cria arquivo nem pasta.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`] quando o arquivo não existe; [`Error::Io`] quando ele
    /// não abre ou a trava falha.
    pub fn existing(path: &Path) -> Result<Self> {
        loop {
            let file = open_existing(path, true)?;
            file.lock()?;
            if still_at(&file, path)? {
                return Ok(Self { file, path: path.to_path_buf() });
            }
        }
    }

    /// O conteúdo inteiro, lido pelo manipulador que segura a trava. Um byte
    /// que não é UTF-8 vira `U+FFFD`: a linha dele deixa de se entender, e o
    /// resto do arquivo continua legível.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] quando a leitura falha.
    pub fn read_to_string(&mut self) -> Result<String> {
        self.file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        self.file.read_to_end(&mut bytes)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Acrescenta `line` e um `\n` no fim, de uma vez, e só volta depois que o
    /// disco confirmou. Quem chama passa a linha sem o `\n`. Se a escrita ou a
    /// confirmação falha, o arquivo volta ao tamanho de antes, sem sobra de
    /// linha pela metade.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] quando a escrita falha.
    pub fn append_line(&mut self, line: &str) -> Result<()> {
        Ok(append_or_rollback(&mut self.file, line)?)
    }

    /// Troca o conteúdo inteiro por `contents`. O conteúdo novo vai inteiro
    /// para um arquivo ao lado, já com a trava, e só então toma o lugar do
    /// velho: o processo morto no meio da escrita deixa o arquivo de antes
    /// intacto, e quem esperava a trava no arquivo velho percebe a troca e
    /// trava o novo. O arquivo ao lado que um processo morto largou some na
    /// troca seguinte do mesmo caminho.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] quando a escrita, a confirmação ou a troca falham; o
    /// arquivo do caminho fica com o conteúdo de antes.
    pub fn replace(&mut self, contents: &[u8]) -> Result<()> {
        self.file = swap_in(&self.file, &self.path, contents)?;
        Ok(())
    }
}

impl Drop for LockedFile {
    fn drop(&mut self) {
        // Fechar o arquivo já solta a trava; soltar antes deixa claro quando.
        let _ = self.file.unlock();
    }
}

/// O intervalo entre as tentativas de quem espera a trava por tempo limitado.
const LOCK_POLL: std::time::Duration = std::time::Duration::from_millis(10);

/// Se `file`, já travado, ainda é o arquivo que mora em `path`. A troca de
/// conteúdo põe um arquivo novo no caminho; quem esperava a trava no velho
/// recebe `false` e abre o caminho de novo, em vez de ler e gravar num
/// arquivo que ninguém mais lê. No Unix a identidade é o dispositivo e o
/// número do arquivo.
#[cfg(unix)]
fn still_at(file: &File, path: &Path) -> Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let held = file.metadata()?;
    match std::fs::metadata(path) {
        Ok(now) => Ok(held.dev() == now.dev() && held.ino() == now.ino()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// Como a do Unix, com a identidade do Windows: o volume e o índice do
/// arquivo, que o sistema dá a cada manipulador aberto
/// (`GetFileInformationByHandle`).
#[cfg(windows)]
fn still_at(file: &File, path: &Path) -> Result<bool> {
    use winapi_util::file::information;
    let held = information(file)?;
    let now = match File::open(path) {
        Ok(now) => information(&now)?,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    Ok(held.volume_serial_number() == now.volume_serial_number() && held.file_index() == now.file_index())
}

/// A extensão do arquivo ao lado em que a troca escreve o conteúdo novo.
const SWAP_EXTENSION: &str = "swap";

/// O arquivo ao lado de `path` em que a troca escreve o conteúdo novo:
/// `.<nome>.<pid>.<hora>.swap`, o mesmo nome da gravação atômica de
/// [`super::real`] com outra extensão. A hora no nome deixa o processo que
/// herda o número de um morto livre do arquivo que o morto largou.
pub(crate) fn swap_path_for(path: &Path) -> PathBuf {
    super::real::temp_path_for(path, SWAP_EXTENSION)
}

/// Grava `contents` em [`swap_path_for`], com as permissões de `old`,
/// confirma no disco e só então o põe no lugar de `path`. O arquivo novo é
/// travado assim que nasce: volta já travado, e entre a troca e a volta
/// ninguém o pega. Se algo falha antes da troca, o arquivo ao lado some e
/// `path` fica como estava. Antes de escrever, apaga os arquivos ao lado que
/// trocas mortas no meio largaram.
fn swap_in(old: &File, path: &Path, contents: &[u8]) -> std::io::Result<File> {
    let side = swap_path_for(path);
    clear_dead_swaps(&side);
    let swapped = (|| {
        let mut file = OpenOptions::new().read(true).write(true).create_new(true).open(&side)?;
        file.lock()?;
        file.set_permissions(old.metadata()?.permissions())?;
        file.write_all(contents)?;
        if super::disk_sync() {
            file.sync_all()?;
        }
        #[cfg(test)]
        tests::reached(tests::Step::SideWritten);
        std::fs::rename(&side, path)?;
        Ok(file)
    })();
    if swapped.is_err() {
        let _ = std::fs::remove_file(&side);
    }
    swapped
}

/// Apaga, na pasta de `side`, os arquivos ao lado do mesmo caminho que uma
/// troca morta no meio largou. A troca em curso trava o seu desde que ele
/// nasce, e o sistema solta a trava de quem morre: o arquivo ao lado que se
/// deixa travar não tem mais dono. Vale também para o nome que a troca usava
/// antes de levar a hora (`.<nome>.<pid>-<n>.swap`). Uma falha aqui não
/// impede a troca: o arquivo largado espera a próxima.
fn clear_dead_swaps(side: &Path) {
    let Some(ours) = side.file_name().and_then(|name| name.to_str()) else { return };
    // `.<nome>.<pid>.<hora>.swap`: o começo, até o pid, é o mesmo em toda troca
    // do caminho.
    let Some(start) = ours.rsplitn(4, '.').nth(3) else { return };
    let suffix = format!(".{SWAP_EXTENSION}");
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    let dir = side.parent().filter(|dir| !dir.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(middle) = name.to_str().and_then(|name| name.strip_prefix(start)).and_then(|rest| rest.strip_prefix('.')).and_then(|rest| rest.strip_suffix(suffix.as_str())) else {
            continue;
        };
        if !middle.split_once(['.', '-']).is_some_and(|(pid, serial)| digits(pid) && digits(serial)) {
            continue;
        }
        let Ok(left) = File::open(entry.path()) else { continue };
        if left.try_lock().is_ok() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// O que a gravação de uma linha pede do destino: escrever, medir o fim,
/// cortar até um tamanho e confirmar no disco. O arquivo de verdade cumpre
/// tudo; o teste põe no lugar um destino que enche de propósito.
trait Appendable: Write + Seek {
    /// Corta o destino em `len` bytes.
    fn shrink_to(&mut self, len: u64) -> std::io::Result<()>;

    /// Espera o disco confirmar o que foi escrito.
    fn sync(&mut self) -> std::io::Result<()>;
}

impl Appendable for File {
    fn shrink_to(&mut self, len: u64) -> std::io::Result<()> {
        self.set_len(len)
    }

    fn sync(&mut self) -> std::io::Result<()> {
        if super::disk_sync() {
            self.sync_data()?;
        }
        Ok(())
    }
}

/// Acrescenta `line` e um `\n` ao fim de `target`, num só bloco, e deixa o
/// arquivo como estava se a escrita ou a confirmação falha: o disco cheio no
/// meio da linha não deixa meia linha para trás. O erro que volta é o da
/// escrita; se o corte também falha, ele não o esconde.
fn append_or_rollback<T: Appendable>(target: &mut T, line: &str) -> std::io::Result<()> {
    let start = target.seek(SeekFrom::End(0))?;
    let mut bytes = Vec::with_capacity(line.len() + 1);
    bytes.extend_from_slice(line.as_bytes());
    bytes.push(b'\n');
    let written = target.write_all(&bytes).and_then(|()| target.sync());
    if let Err(err) = written {
        let _ = target.shrink_to(start);
        return Err(err);
    }
    Ok(())
}

/// Abre `path` para ler e escrever, criando o arquivo e a pasta quando
/// faltam, sem mexer no conteúdo.
fn open_creating(path: &Path) -> Result<File> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)?;
    #[cfg(test)]
    tests::reached(tests::Step::Opened);
    Ok(file)
}

/// Abre `path`, que precisa existir, para ler e, com `write`, também
/// escrever.
fn open_existing(path: &Path, write: bool) -> Result<File> {
    let file = match OpenOptions::new().read(true).write(write).open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Err(Error::NotFound(path.display().to_string()));
        }
        Err(e) => return Err(e.into()),
    };
    #[cfg(test)]
    tests::reached(tests::Step::Opened);
    Ok(file)
}

/// Lê `path` inteiro com a trava compartilhada: espera uma gravação em curso
/// terminar, então nunca vê uma linha pela metade dela.
///
/// # Errors
///
/// [`Error::NotFound`] quando o arquivo não existe; [`Error::Io`] quando a
/// leitura ou a trava falham.
pub fn read_shared(path: &Path) -> Result<String> {
    let mut file = loop {
        let file = open_existing(path, false)?;
        file.lock_shared()?;
        if still_at(&file, path)? {
            break file;
        }
    };
    let mut bytes = Vec::new();
    let read = file.read_to_end(&mut bytes);
    let _ = file.unlock();
    read?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_locked_file_reads_and_appends_through_its_own_handle() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/b/log.ndjson");
        let mut file = LockedFile::exclusive(&path).unwrap();
        assert_eq!(file.read_to_string().unwrap(), "");
        file.append_line("um").unwrap();
        file.append_line("dois").unwrap();
        assert_eq!(file.read_to_string().unwrap(), "um\ndois\n");
        file.replace(b"tres\n").unwrap();
        drop(file);
        assert_eq!(read_shared(&path).unwrap(), "tres\n");
    }

    #[test]
    fn reading_a_missing_file_says_not_found() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(read_shared(&dir.path().join("x")), Err(Error::NotFound(_))));
    }

    #[test]
    fn locking_an_existing_file_never_creates_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/log.ndjson");
        assert!(matches!(LockedFile::existing(&path), Err(Error::NotFound(_))));
        assert!(!path.exists() && !path.parent().unwrap().exists(), "nothing was created");
        LockedFile::exclusive(&path).unwrap().append_line("um").unwrap();
        let mut file = LockedFile::existing(&path).unwrap();
        assert_eq!(file.read_to_string().unwrap(), "um\n");
    }

    /// A trava pedida sem esperar volta vazia enquanto outro manipulador a
    /// segura, sem esperar por ele, e volta presa depois que ele solta.
    #[test]
    fn the_lock_asked_without_waiting_is_empty_while_another_handle_holds_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/round.lock");
        let held = LockedFile::exclusive(&path).unwrap();
        assert!(LockedFile::exclusive_if_free(&path).unwrap().is_none(), "held elsewhere: nothing");
        drop(held);
        let free = LockedFile::exclusive_if_free(&path).unwrap();
        assert!(free.is_some(), "free: the lock is taken");
        assert!(LockedFile::exclusive_if_free(&path).unwrap().is_none(), "and now it is held by the first");
    }

    /// Um destino que aceita `room` bytes e então diz que o disco encheu, como
    /// o disco cheio de verdade no meio de uma linha. Guarda o tamanho de cada
    /// escrita para provar que a linha sai num bloco só.
    struct Full {
        data: Vec<u8>,
        pos: usize,
        room: usize,
        writes: Vec<usize>,
        sync_fails: bool,
        shrink_fails: bool,
    }

    impl Full {
        fn new(data: &str, room: usize) -> Self {
            Self { data: data.as_bytes().to_vec(), pos: 0, room, writes: Vec::new(), sync_fails: false, shrink_fails: false }
        }

        fn text(&self) -> String {
            String::from_utf8(self.data.clone()).unwrap()
        }
    }

    impl Write for Full {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if self.room == 0 {
                return Err(std::io::Error::from(ErrorKind::StorageFull));
            }
            let taken = buf.len().min(self.room);
            self.room -= taken;
            self.writes.push(taken);
            self.data.truncate(self.pos);
            self.data.extend_from_slice(&buf[..taken]);
            self.pos += taken;
            Ok(taken)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Seek for Full {
        fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
            self.pos = match to {
                SeekFrom::Start(n) => n as usize,
                SeekFrom::End(n) => self.data.len().saturating_add_signed(n as isize),
                SeekFrom::Current(n) => self.pos.saturating_add_signed(n as isize),
            };
            Ok(self.pos as u64)
        }
    }

    impl Appendable for Full {
        fn shrink_to(&mut self, len: u64) -> std::io::Result<()> {
            if self.shrink_fails {
                return Err(std::io::Error::from(ErrorKind::PermissionDenied));
            }
            self.data.truncate(len as usize);
            Ok(())
        }

        fn sync(&mut self) -> std::io::Result<()> {
            if self.sync_fails {
                return Err(std::io::Error::from(ErrorKind::StorageFull));
            }
            Ok(())
        }
    }

    /// O disco enche 5 bytes depois de a linha começar: o que já tinha 8 bytes
    /// termina com 8 bytes, o mesmo conteúdo, e o erro é o do disco cheio.
    #[test]
    fn a_line_cut_by_a_full_disk_leaves_the_file_as_it_was() {
        let mut target = Full::new("um\ndois\n", 5);
        let err = append_or_rollback(&mut target, "tres-quatro").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::StorageFull);
        assert_eq!(target.writes, [5], "five bytes went in before the disk filled");
        assert_eq!(target.data.len(), 8);
        assert_eq!(target.text(), "um\ndois\n");
    }

    /// A confirmação do disco que falha, com a linha inteira já escrita, também
    /// desfaz a linha: quem recebe o erro não fica com a gravação pela metade.
    #[test]
    fn a_failed_confirmation_takes_the_written_line_back() {
        let mut target = Full::new("um\n", 100);
        target.sync_fails = true;
        let err = append_or_rollback(&mut target, "dois").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::StorageFull);
        assert_eq!(target.text(), "um\n");
    }

    /// Se o corte também falha, o erro que volta continua sendo o da escrita.
    #[test]
    fn a_failed_cut_does_not_hide_the_write_error() {
        let mut target = Full::new("um\n", 2);
        target.shrink_fails = true;
        let err = append_or_rollback(&mut target, "dois").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::StorageFull, "not the cut's PermissionDenied");
    }

    /// A linha e o `\n` saem numa escrita só, com o tamanho da linha mais um.
    #[test]
    fn the_line_and_its_newline_go_out_in_one_write() {
        let mut target = Full::new("", 100);
        append_or_rollback(&mut target, "abcdef").unwrap();
        assert_eq!(target.writes, [7]);
        assert_eq!(target.text(), "abcdef\n");
    }

    /// No arquivo de verdade que só devolve `StorageFull` (o `/dev/full`), a
    /// gravação pela trava responde o erro do disco, mesmo com o corte
    /// impossível nele.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_locked_append_to_a_full_device_answers_the_disk_error() {
        let mut file = LockedFile::exclusive(Path::new("/dev/full")).unwrap();
        match file.append_line("um") {
            Err(Error::Io(e)) => assert_eq!(e.kind(), ErrorKind::StorageFull),
            other => panic!("expected the disk error, got {other:?}"),
        }
    }

    /// Uma troca de conteúdo que esbarra no limite de tamanho de arquivo do
    /// processo (o `ulimit -f`, o mesmo efeito do disco cheio) falha e deixa o
    /// arquivo com o conteúdo de antes, que continua se lendo inteiro.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_locked_replace_cut_by_the_size_limit_keeps_the_previous_content() {
        const FILE: &str = "MUSTARD_REPLACE_LIMIT_FILE";
        if let Ok(path) = std::env::var(FILE) {
            let mut file = LockedFile::exclusive(Path::new(&path)).unwrap();
            let err = file.replace(format!("{{\"events\":\"{}\"}}", "b".repeat(80 * 1024)).as_bytes()).unwrap_err();
            assert!(matches!(err, Error::Io(_)), "{err:?}");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("diagnostics.json");
        let before = format!("{{\"events\":\"{}\"}}", "a".repeat(40 * 1024));
        std::fs::write(&path, &before).unwrap();
        let test = "a_locked_replace_cut_by_the_size_limit_keeps_the_previous_content";
        let out = std::process::Command::new("bash")
            .args(["-c", "trap '' XFSZ; ulimit -f 64 && exec \"$0\" \"$@\""])
            .arg(std::env::current_exe().unwrap())
            .args([test, "--test-threads=1"])
            .env(FILE, &path)
            .output()
            .expect("bash runs the test binary under the size limit");
        assert!(out.status.success(), "the child failed: {}", String::from_utf8_lossy(&out.stdout));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before, "same content as before");
    }

    /// Os pontos em que um teste segura a linha que trava ou troca, para pôr a
    /// outra ponta ali sem pausa medida no relógio.
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(super) enum Step {
        /// O arquivo do caminho acabou de abrir, e a trava ainda não foi pedida.
        Opened,
        /// O conteúdo novo está inteiro no arquivo ao lado, ainda fora do lugar.
        SideWritten,
    }

    /// O que a linha atual pediu para rodar em cada ponto ([`on_step`]).
    type StepHook = Box<dyn FnMut(Step)>;

    thread_local! {
        static ON_STEP: std::cell::RefCell<Option<StepHook>> = const { std::cell::RefCell::new(None) };
    }

    /// Roda o que a linha atual pediu para `step`; nas outras linhas, nada.
    pub(super) fn reached(step: Step) {
        ON_STEP.with(|hook| {
            if let Some(hook) = hook.borrow_mut().as_mut() {
                hook(step);
            }
        });
    }

    /// Pede que a linha atual rode `hook` em cada ponto que alcançar.
    fn on_step(hook: impl FnMut(Step) + 'static) {
        ON_STEP.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
    }

    /// Põe "um\n" em `path`, segura a trava e roda `form` noutra linha, que
    /// para logo depois de abrir o arquivo pela primeira vez. Com o arquivo
    /// velho aberto lá, troca o conteúdo para "dois\n" e só então deixa `form`
    /// pedir a trava. Volta a trava, presa no arquivo novo, e a linha de `form`.
    fn swap_after_open<T: Send + 'static>(path: &Path, form: impl FnOnce(&Path) -> T + Send + 'static) -> (LockedFile, std::thread::JoinHandle<T>) {
        let mut held = LockedFile::exclusive(path).unwrap();
        held.replace(b"um\n").unwrap();
        let (opened, at_open) = std::sync::mpsc::channel::<()>();
        let (go, wait_go) = std::sync::mpsc::channel::<()>();
        let waiter = {
            let path = path.to_path_buf();
            std::thread::spawn(move || {
                let mut first = Some((opened, wait_go));
                on_step(move |step| {
                    if step == Step::Opened
                        && let Some((opened, wait_go)) = first.take()
                    {
                        opened.send(()).unwrap();
                        crate::wait_for::wait_for_message("a troca do conteúdo do arquivo", &wait_go).unwrap();
                    }
                });
                form(&path)
            })
        };
        crate::wait_for::wait_for_message("a primeira abertura do arquivo", &at_open).unwrap();
        held.replace(b"dois\n").unwrap();
        go.send(()).unwrap();
        (held, waiter)
    }

    /// Quem espera a trava exclusiva no arquivo que uma troca tira do caminho
    /// trava o arquivo que ficou no lugar: lê o conteúdo novo, e a linha que
    /// acrescenta fica no arquivo que todos leem.
    #[test]
    fn the_exclusive_lock_waited_across_a_swap_holds_the_file_left_at_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.json");
        let (held, waiter) = swap_after_open(&path, |path| {
            let mut file = LockedFile::exclusive(path).unwrap();
            let seen = file.read_to_string().unwrap();
            file.append_line("tres").unwrap();
            seen
        });
        drop(held);
        assert_eq!(waiter.join().unwrap(), "dois\n");
        assert_eq!(read_shared(&path).unwrap(), "dois\ntres\n");
    }

    /// A mesma regra na trava de um arquivo que precisa existir.
    #[test]
    fn the_lock_of_an_existing_file_waited_across_a_swap_holds_the_file_left_at_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.json");
        let (held, waiter) = swap_after_open(&path, |path| {
            let mut file = LockedFile::existing(path).unwrap();
            let seen = file.read_to_string().unwrap();
            file.append_line("tres").unwrap();
            seen
        });
        drop(held);
        assert_eq!(waiter.join().unwrap(), "dois\n");
        assert_eq!(read_shared(&path).unwrap(), "dois\ntres\n");
    }

    /// A trava pedida sem esperar, no arquivo que uma troca acaba de tirar do
    /// caminho, abre o caminho de novo: o arquivo que ficou lá está preso, e
    /// ela volta vazia em vez de travar o velho, que ninguém mais lê.
    #[test]
    fn the_lock_asked_without_waiting_after_a_swap_looks_at_the_file_left_at_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.json");
        let (held, waiter) = swap_after_open(&path, |path| LockedFile::exclusive_if_free(path).unwrap().map(|mut file| file.read_to_string().unwrap()));
        assert_eq!(waiter.join().unwrap(), None, "the file at the path is held");
        drop(held);
    }

    /// Quem lê com a trava compartilhada através de uma troca lê o conteúdo
    /// novo, do arquivo que ficou no caminho.
    #[test]
    fn a_shared_read_waited_across_a_swap_reads_the_file_left_at_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.json");
        let (held, waiter) = swap_after_open(&path, |path| read_shared(path).unwrap());
        drop(held);
        assert_eq!(waiter.join().unwrap(), "dois\n");
    }

    /// O que a troca no filho escreve na saída quando o conteúdo novo está
    /// inteiro no arquivo ao lado.
    const SIDE_WRITTEN: &str = "side-written";

    /// Os nomes da pasta `dir`, menos `but`, em ordem.
    fn names_besides(dir: &Path, but: &str) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).filter(|name| name != but).collect();
        names.sort_unstable();
        names
    }

    /// Um processo morto no meio de uma troca, com o conteúdo novo inteiro no
    /// arquivo ao lado e ainda fora do lugar, deixa o arquivo com o conteúdo
    /// de antes. A troca seguinte do mesmo caminho apaga o arquivo ao lado que
    /// ele largou, e também o do nome sem a hora, e deixa o de uma troca que
    /// ainda segura a trava dele.
    #[test]
    fn a_swap_killed_midway_keeps_the_previous_content_and_the_next_swap_clears_its_leftover() {
        const FILE: &str = "MUSTARD_SWAP_KILLED_FILE";
        if let Ok(path) = std::env::var(FILE) {
            on_step(|step| {
                if step == Step::SideWritten {
                    // Direto na saída, sem a captura do teste: o pai lê esta linha.
                    let mut out = std::io::stdout();
                    let _ = writeln!(out, "{SIDE_WRITTEN}").and_then(|()| out.flush());
                    loop {
                        std::thread::park();
                    }
                }
            });
            let mut file = LockedFile::exclusive(Path::new(&path)).unwrap();
            let _ = file.replace(b"dois\n");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.json");
        std::fs::write(&path, "um\n").unwrap();
        let test = "a_swap_killed_midway_keeps_the_previous_content_and_the_next_swap_clears_its_leftover";
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([test, "--test-threads=1"])
            .env(FILE, &path)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let out = std::io::BufReader::new(child.stdout.take().unwrap());
        let written = std::io::BufRead::lines(out).map_while(std::result::Result::ok).any(|line| line.contains(SIDE_WRITTEN));
        assert!(written, "the child stops with the new content written beside the file");
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "um\n", "same content as before");
        let left = names_besides(dir.path(), "index.json");
        assert_eq!(left.len(), 1, "the dead swap left its file: {left:?}");
        // O sistema solta a trava do processo morto; no Windows, até um
        // instante depois do fim dele.
        crate::wait_for::wait_for("o sistema soltar a trava do processo morto", || File::open(dir.path().join(&left[0])).unwrap().try_lock().is_ok().then_some(()));
        std::fs::write(dir.path().join(".index.json.4242-0.swap"), "x").unwrap();
        let live = swap_path_for(&path);
        std::fs::write(&live, "y").unwrap();
        let live_lock = File::open(&live).unwrap();
        live_lock.lock().unwrap();

        LockedFile::exclusive(&path).unwrap().replace(b"tres\n").unwrap();

        assert_eq!(read_shared(&path).unwrap(), "tres\n");
        let live_name = live.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(names_besides(dir.path(), "index.json"), [live_name], "only the swap still held is left");
    }

    /// Uma gravação pela trava que esbarra no limite de tamanho de arquivo do
    /// processo (o `ulimit -f`) grava os 40 bytes que cabem e falha: o arquivo
    /// tem que terminar com o tamanho e o conteúdo de antes. O limite vale para
    /// o processo inteiro, então o teste roda a si mesmo num filho com ele.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_locked_append_cut_by_the_size_limit_leaves_the_file_as_it_was() {
        const FILE: &str = "MUSTARD_APPEND_LIMIT_FILE";
        if let Ok(path) = std::env::var(FILE) {
            let mut file = LockedFile::exclusive(Path::new(&path)).unwrap();
            let err = file.append_line(&"x".repeat(100)).unwrap_err();
            assert!(matches!(err, Error::Io(_)), "{err:?}");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        // 64 KiB de limite; o arquivo já tem 40 bytes a menos que isso.
        let before = format!("{}\n", "a".repeat(64 * 1024 - 41));
        std::fs::write(&path, &before).unwrap();
        let test = "a_locked_append_cut_by_the_size_limit_leaves_the_file_as_it_was";
        let out = std::process::Command::new("bash")
            .args(["-c", "trap '' XFSZ; ulimit -f 64 && exec \"$0\" \"$@\""])
            .arg(std::env::current_exe().unwrap())
            .args([test, "--test-threads=1"])
            .env(FILE, &path)
            .output()
            .expect("bash runs the test binary under the size limit");
        assert!(out.status.success(), "the child failed: {}", String::from_utf8_lossy(&out.stdout));
        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(after.len(), 64 * 1024 - 40, "same size as before");
        assert_eq!(after, before, "same content as before");
    }
}
