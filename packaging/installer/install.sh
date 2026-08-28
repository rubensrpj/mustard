#!/usr/bin/env sh
# ============================================================================
# Mustard — instalador completo (Ubuntu / Debian)
#
# Instala o pacote .deb que traz TUDO: os binários do CLI (mustard, mustard-rt,
# mustard-mcp, scan, rtk) E o mustard-dashboard, o servidor HTTP que serve o
# painel no navegador. Usa `apt`.
#
# Layout instalado (gerenciado pelo apt, removível com `sudo apt remove mustard`):
#   /usr/lib/mustard/bin/        binários reais (CLI + mustard-dashboard)
#   /usr/lib/mustard/bin/dist/   os arquivos da tela que o servidor serve
#   /usr/lib/mustard/templates/  a carga do `mustard init`
#   /usr/bin/mustard, …          symlinks no PATH (criados pelo pacote)
#   atalho "Mustard Dashboard" no menu de aplicativos (inicia o servidor)
#
# Uso:
#   curl -fsSL https://github.com/rubensrpj/mustard/releases/latest/download/install.sh | sh
#                                 # instala baixando o .deb do último Release
#   ./install.sh                  # instala tudo
#   ./install.sh /caminho/projeto # também roda `mustard init` nesse projeto
#   ./install.sh --dry-run        # só mostra o que seria instalado; não instala
#                                 # (sai != 0 se não conseguir descobrir o pacote)
#   MUSTARD_VERSION=0.1.35 ./install.sh   # fixa a versão em vez do último Release
#   curl -fsSL https://github.com/rubensrpj/mustard/releases/latest/download/install.sh | MUSTARD_VERSION=0.1.35 sh
#                                 # o mesmo, na forma de uma linha só
#
# Troque o 0.1.35 pelo número que a página de Releases mostrar. O número vai
# LITERAL na linha: escrever <versao> não é um espaço em branco para preencher,
# é um redirecionamento de entrada — o shell zera a variável, tenta abrir um
# arquivo chamado `versao` e responde `sh: versao: No such file or directory`.
#
# O .deb ao lado deste script é a fonte preferida; quando não há nenhum (é o caso
# do `curl … | sh`, que não deixa arquivo nenhum em disco) o instalador baixa o
# pacote do Release do GitHub.
# ============================================================================
set -eu

# POSIX decides bracket-range membership ([A-Za-z0-9], [0-9]*) by the CURRENT
# locale's collating sequence, not by ASCII — under some UTF-8 locales a
# character valid_version() promises to reject collates inside the range and
# passes, and $VERSION lands in a filesystem path and in a URL. The C locale
# collates by byte, which makes every `case` and every `sort` below mean exactly
# what it reads like. Side effect, and it is the right trade: apt's own messages
# come out in English instead of the user's language.
LC_ALL=C
export LC_ALL

GITHUB_REPO="rubensrpj/mustard"
LATEST_URL="https://github.com/$GITHUB_REPO/releases/latest"

DRY_RUN=0
TARGET=""
SUDO=""
DEB=""
DEB_URL=""
TMP_DIR=""
VERSION="${MUSTARD_VERSION:-}"
VERSION="${VERSION#v}"

# The ONE place that says what a version string may look like: it starts with a
# digit and carries nothing outside [A-Za-z0-9._-]. Both callers go through here
# — the tag resolved from the network AND $MUSTARD_VERSION, which is user input
# that lands in a filesystem path ($DEB) and in a URL ($DEB_URL). Without the
# charset gate, MUSTARD_VERSION=../../../etc/passwd escapes $TMP_DIR, and the
# cleanup trap (which only removes $TMP_DIR) leaves the file behind.
valid_version() {
  case "$1" in
    *[!A-Za-z0-9._-]*) return 1 ;;
    [0-9]*)            return 0 ;;
    *)                 return 1 ;;
  esac
}

if [ -n "$VERSION" ] && ! valid_version "$VERSION"; then
  # The rejected value is deliberately NOT echoed: it is untrusted input and
  # this message is what a reader sees; the expected shape is what helps.
  echo "erro: MUSTARD_VERSION inválida." >&2
  echo "      esperado o número da versão, como 0.1.35 (o 'v' é opcional):" >&2
  echo "      começa por dígito e aceita só letras, dígitos, ponto, hífen e _." >&2
  exit 1
fi

# Drop the download directory on every exit path, failures and signals included.
# $TMP_DIR is the ONLY thing this script creates on disk: the tag probe used to
# park wget's headers in a mktemp file too, but that assignment happened inside
# the `$(resolve_latest_tag)` command-substitution SUBSHELL, so the parent's copy
# of the variable stayed empty and this trap could never see it — the guard was
# always false and a Ctrl-C during the probe leaked the file. The probe now keeps
# the headers in a variable, so there is no second path to clean up.
cleanup() {
  if [ -n "$TMP_DIR" ] && [ -d "$TMP_DIR" ]; then
    rm -rf "$TMP_DIR"
  fi
}
trap cleanup EXIT
trap 'cleanup; exit 130' INT TERM HUP

# --- argumentos: [--dry-run] [caminho-do-projeto] ---------------------------
# The project path is positional (as it always was); flags start with a dash, so
# the two can never be confused.
for arg in "$@"; do
  case "$arg" in
    --dry-run)
      DRY_RUN=1
      ;;
    -*)
      echo "erro: opção desconhecida: $arg" >&2
      echo "      uso: install.sh [--dry-run] [caminho-do-projeto]" >&2
      exit 1
      ;;
    *)
      if [ -n "$TARGET" ]; then
        echo "erro: aceito um único caminho de projeto (recebi '$TARGET' e '$arg')." >&2
        exit 1
      fi
      # Checked HERE, before anything is installed. The check used to sit after
      # `apt-get install`, so a typo in the path ran the whole root install and
      # only then aborted — the user read a non-zero exit and concluded, wrongly,
      # that nothing had been installed. --dry-run gets the same check for free.
      if [ ! -d "$arg" ]; then
        echo "erro: projeto-alvo não existe: $arg" >&2
        echo "      passe o caminho de um diretório já existente." >&2
        exit 1
      fi
      TARGET=$(CDPATH= cd -- "$arg" && pwd) || {
        echo "erro: não consegui entrar no projeto-alvo: $arg" >&2
        exit 1
      }
      ;;
  esac
done

# --- precisa de apt (Ubuntu/Debian) -----------------------------------------
# --dry-run only resolves and prints, so it must not demand apt.
if [ "$DRY_RUN" -eq 0 ] && ! command -v apt-get >/dev/null 2>&1; then
  echo "erro: este instalador usa apt (Ubuntu/Debian). Não encontrei o apt-get." >&2
  exit 1
fi

# --- arquitetura: só existe pacote amd64 publicado --------------------------
# O nome do asset é fixo (mustard_<versao>_amd64.deb) porque é o único que o
# Release traz — não há URL arm64 a construir. Sem esta recusa, um arm64
# (Raspberry Pi, Graviton, VM no Apple Silicon) resolve a tag, BAIXA um .deb
# perfeitamente válido — o `dpkg-deb --info` aprova, ele só é de outra máquina —,
# digita a senha do sudo, espera o `apt-get update` e só então morre com
# "package architecture (amd64) does not match system (arm64)". Por isso a
# verificação mora aqui, junto com as outras duas de "esta máquina serve?": ANTES
# de qualquer ida à rede, ANTES do download e ANTES da senha do sudo.
#
# --dry-run não instala nada e não é barrado aqui — ele só resolve e descreve o
# pacote, e precisa continuar respondendo isso em qualquer máquina (é assim que
# um critério consegue exercitar a resolução sem apt e sem root).
if [ "$DRY_RUN" -eq 0 ]; then
  ARCH=""
  if command -v dpkg >/dev/null 2>&1; then
    ARCH=$(dpkg --print-architecture 2>/dev/null) || ARCH=""
  fi
  if [ -z "$ARCH" ]; then
    echo "erro: não sei a arquitetura desta máquina — o dpkg não está aqui, ou não" >&2
    echo "      respondeu. Como só existe pacote amd64 publicado, não vou instalar" >&2
    echo "      no escuro." >&2
    echo "      num Debian/Ubuntu o dpkg sempre existe; sem ele o próprio apt não" >&2
    echo "      instalaria o pacote. Confira a máquina, ou baixe o .deb à mão da" >&2
    echo "      página https://github.com/$GITHUB_REPO/releases." >&2
    exit 1
  fi
  if [ "$ARCH" != "amd64" ]; then
    echo "erro: arquitetura não suportada: $ARCH." >&2
    echo "      o Release publica só o pacote amd64 (mustard_<versao>_amd64.deb);" >&2
    echo "      não existe build $ARCH para baixar, e instalar o amd64 aqui falharia" >&2
    echo "      no dpkg depois de pedir sua senha." >&2
    echo "      saída: compilar do código-fonte — https://github.com/$GITHUB_REPO" >&2
    exit 1
  fi
fi

# --- sudo só quando não-root ------------------------------------------------
# Verificado ANTES de qualquer ida à rede: resolver a versão custa até 60s, e
# quem não é root nem tem sudo não pode instalar de jeito nenhum — dizer isso na
# primeira linha é mais honesto do que dizer depois da espera. Não depende de
# nada resolvido adiante. --dry-run não instala, então não exige sudo.
if [ "$DRY_RUN" -eq 0 ] && [ "$(id -u)" -ne 0 ]; then
  if command -v sudo >/dev/null 2>&1; then
    SUDO="sudo"
  else
    echo "erro: não sou root e não há sudo. Rode como root ou instale o sudo." >&2
    exit 1
  fi
fi

# --- ferramenta de download (curl ou wget) ----------------------------------
DOWNLOADER=""
if command -v curl >/dev/null 2>&1; then
  DOWNLOADER="curl"
elif command -v wget >/dev/null 2>&1; then
  DOWNLOADER="wget"
fi

# Follows the /releases/latest redirect and prints the tag it lands on (v0.1.35).
# The asset name carries the version, so there is no stable "latest" URL for the
# .deb; and the unauthenticated GitHub API is rate-limited, hence the redirect.
resolve_latest_tag() {
  _url=""
  case "$DOWNLOADER" in
    curl)
      # -I (HEAD) yields the same %{url_effective} as a GET for a few hundred
      # bytes; a GET here downloads the whole /releases/tag/vX page only to throw
      # it away — real waste on the slow links this path exists to serve. But a
      # corporate proxy that answers 405/403 to HEAD would then kill the whole
      # one-liner, blaming the network for something a plain GET resolves fine —
      # so the GET is the second attempt, not the absent one.
      _url=$(curl -fsSLI --connect-timeout 10 --max-time 60 \
               -o /dev/null -w '%{url_effective}' "$LATEST_URL" 2>/dev/null) \
        || _url=$(curl -fsSL --connect-timeout 10 --max-time 60 \
               -o /dev/null -w '%{url_effective}' "$LATEST_URL" 2>/dev/null) \
        || return 1
      ;;
    wget)
      # `wget … | sed | head` took its status from head, which is always 0: DNS
      # failure, TLS rejection, 404 and "answered, but no Location" all collapsed
      # into an empty $_url and one generic message about the network. POSIX sh
      # has no `pipefail`, so the output is captured whole and read twice — once
      # for the Location, once to tell "never reached GitHub" from "reached it and
      # it answered no tag". wget's own status cannot be that verdict:
      # --max-redirect=0 makes it exit non-zero on the SUCCESS path too, since
      # the redirect it is told not to follow IS the answer being asked for.
      #
      # The capture is a VARIABLE, not a temp file: this function runs inside a
      # command substitution, so anything it created on disk would be untrackable
      # by the parent's cleanup trap (see cleanup()). A response header block is
      # a few hundred bytes — there is nothing to gain by putting it on disk.
      # -q would also silence -S, so the headers are read from the captured stderr.
      _hdr=$(wget -S --max-redirect=0 --tries=1 --timeout=20 -O /dev/null \
               "$LATEST_URL" 2>&1 || :)
      _url=$(printf '%s\n' "$_hdr" \
               | sed -n 's/^[[:space:]]*Location:[[:space:]]*\([^[:space:]]*\).*/\1/p' \
               | head -1)
      if [ -z "$_url" ]; then
        if printf '%s\n' "$_hdr" | grep -q '^[[:space:]]*HTTP/'; then
          echo "aviso: o GitHub respondeu, mas não apontou nenhuma versão em" >&2
          echo "       $LATEST_URL — nenhum Release publicado, ou todos em rascunho." >&2
        else
          echo "aviso: não consegui falar com o GitHub ($LATEST_URL)." >&2
          echo "       a resposta não chegou: DNS, proxy ou TLS — não é 'sem versão'." >&2
        fi
      fi
      ;;
    *)
      return 1
      ;;
  esac
  # A release tag looks like `v0.1.35`. Demanding that shape is what separates
  # "no release published" from "a release was found": with every release in
  # draft, /releases/latest redirects to /releases, whose last path segment is
  # the word `releases` — a slug that passes a charset check and then builds
  # `.../releases/download/releases/mustard_releases_amd64.deb`, so the failure
  # only surfaces later, as a download error that names the wrong cause.
  _tag="${_url##*/}"
  case "$_tag" in
    v*) ;;
    *) return 1 ;;
  esac
  # Charset and "starts with a digit" are decided in valid_version(), the single
  # place that describes an accepted version — so the shape is edited once.
  valid_version "${_tag#v}" || return 1
  printf '%s\n' "$_tag"
}

download_file() {
  case "$DOWNLOADER" in
    # --connect-timeout bounds only the handshake: a proxy that accepts the
    # connection and then trickles nothing would wedge the one-liner forever,
    # three times over. --speed-limit/--speed-time aborts a stalled transfer
    # (under 1 KB/s for 30s) without capping a slow-but-honest download, which a
    # flat --max-time would do to a package this size. wget: --timeout=30.
    curl) curl -fL --progress-bar --connect-timeout 15 --retry 2 \
               --speed-limit 1024 --speed-time 30 -o "$2" "$1" ;;
    wget) wget --tries=3 --timeout=30 -O "$2" "$1" ;;
    *)    return 1 ;;
  esac
}

# --- 1) .deb ao lado deste script (fonte preferida) -------------------------
# $0 is not a usable path when the script arrives through a pipe (`curl | sh`):
# it is the literal string `sh`, which `[ -f "$0" ]` happily resolves against the
# CURRENT directory. Run the one-liner from a directory that happens to hold a
# file named `sh` (or `dash`, or `bash`) and SCRIPT_DIR became that directory,
# so the picker below installed whatever stale or foreign-architecture .deb was
# lying there instead of the release the user asked for. A path — something with
# a slash in it — is the part `curl | sh` cannot fake, so it is what gets tested,
# on top of the file still having to exist.
#
# What `curl | sh` cannot fake is a $0 that names THIS script: piped, $0 is the
# interpreter's own name. So the interpreter names are what gets rejected, rather
# than demanding a slash — `sh install.sh` (no slash, but a real script name) is a
# legitimate invocation, and rejecting it would ignore a .deb sitting right there
# and fail offline.
SCRIPT_DIR=""
case "$0" in
  sh|bash|dash|ash|ksh|zsh|-sh|-bash) ;;
  *)
    if [ -f "$0" ]; then
      SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
    fi
    ;;
esac
if [ -n "$SCRIPT_DIR" ]; then
  if [ -n "$VERSION" ]; then
    # An explicit version only accepts the local .deb of that same version.
    LOCAL_DEB="$SCRIPT_DIR/mustard_${VERSION}_amd64.deb"
    if [ -f "$LOCAL_DEB" ]; then
      DEB="$LOCAL_DEB"
    fi
  else
    # sort -V | tail -1 = the NEWEST match. Plain `ls | head -1` sorts ascending,
    # so a ~/Downloads holding 0.1.35 and 0.2.0 silently installed 0.1.35.
    DEB=$(ls "$SCRIPT_DIR"/mustard_*_amd64.deb 2>/dev/null | sort -V | tail -1 || true)
  fi
fi

# --- 2) sem .deb local: o Release do GitHub ---------------------------------
if [ -z "$DEB" ]; then
  # --dry-run não baixa nada, então não exige curl/wget: com MUSTARD_VERSION
  # fixada ele ainda sabe qual pacote nomearia. Sem downloader E sem versão
  # fixada ele não sabe, e é o bloco 3 que recusa — aqui não.
  if [ -z "$DOWNLOADER" ] && [ "$DRY_RUN" -eq 0 ]; then
    echo "erro: não achei um mustard_*_amd64.deb ao lado do install.sh e não há" >&2
    echo "      curl nem wget para baixar o pacote do Release." >&2
    echo "      instale um dos dois (sudo apt install curl) ou baixe o .deb e" >&2
    echo "      rode o install.sh de dentro da pasta do pacote." >&2
    exit 1
  fi

  TAG=""
  if [ -n "$VERSION" ]; then
    TAG="v$VERSION"
  else
    TAG=$(resolve_latest_tag) || TAG=""
    if [ -n "$TAG" ]; then
      VERSION="${TAG#v}"
    fi
  fi

  if [ -n "$TAG" ]; then
    DEB_URL="https://github.com/$GITHUB_REPO/releases/download/$TAG/mustard_${VERSION}_amd64.deb"
  elif [ "$DRY_RUN" -eq 0 ]; then
    echo "erro: não consegui resolver a última versão em $LATEST_URL." >&2
    echo "      sem rede? tente de novo, ou fixe a versão que quer instalar" >&2
    echo "      (a lista está em https://github.com/$GITHUB_REPO/releases):" >&2
    echo "      MUSTARD_VERSION=0.1.35 ./install.sh   (troque pelo número de lá)" >&2
    exit 1
  fi
fi

# --- 3) --dry-run: mostra o que seria feito e sai ---------------------------
# Resolution already happened above; nothing here touches apt or the network.
# The exit status is the whole point of this mode: 0 means "I know what I would
# install" — a local .deb (knowable offline, the package is right there) or a
# resolved/pinned tag — and NON-ZERO means the resolution failed. It used to
# exit 0 either way, "naming the URL it would have used", which turned a check
# on a runner with no egress into a green that proved nothing at all.
if [ "$DRY_RUN" -eq 1 ]; then
  if [ -z "$DEB" ] && [ -z "$DEB_URL" ]; then
    echo "erro: --dry-run não conseguiu determinar o que seria instalado." >&2
    echo "      não há mustard_*_amd64.deb ao lado do install.sh e a versão não" >&2
    echo "      pôde ser resolvida em $LATEST_URL (sem rede? sem curl nem wget?)." >&2
    echo "      fixe a versão que quer instalar (a lista está em" >&2
    echo "      https://github.com/$GITHUB_REPO/releases):" >&2
    echo "      MUSTARD_VERSION=0.1.35 ./install.sh --dry-run   (troque o número)" >&2
    exit 1
  fi
  echo "==> --dry-run: nada será instalado."
  if [ -n "$DEB" ]; then
    echo "    Pacote:  $DEB"
    echo "    Origem:  arquivo local (ao lado do install.sh)"
  else
    echo "    Pacote:  mustard_${VERSION}_amd64.deb"
    echo "    Origem:  $DEB_URL"
  fi
  echo "    Comando: apt-get install -y <pacote>"
  if [ -n "$TARGET" ]; then
    echo "    Depois:  mustard init --yes em $TARGET"
  fi
  exit 0
fi

# --- 4) baixa o pacote quando ele não veio do disco -------------------------
if [ -z "$DEB" ]; then
  TMP_DIR=$(mktemp -d 2>/dev/null || mktemp -d -t mustard.XXXXXX) || {
    echo "erro: não consegui criar um diretório temporário para o download." >&2
    exit 1
  }
  # apt drops privileges to the _apt user to read the file; 0700/0600 would only
  # produce a sandbox warning on every run.
  chmod 0755 "$TMP_DIR"
  DEB="$TMP_DIR/mustard_${VERSION}_amd64.deb"
  echo "==> Baixando $DEB_URL"
  if ! download_file "$DEB_URL" "$DEB" || [ ! -s "$DEB" ]; then
    echo "erro: falha ao baixar o pacote em $DEB_URL" >&2
    echo "      confira a rede, ou baixe o .deb à mão e rode o install.sh ao lado dele." >&2
    exit 1
  fi
  # "Não está vazio" não prova que é um pacote: um portal cativo responde 200 com
  # uma página HTML, que passa pelo `curl -f` e pelo teste acima — e só falha lá
  # na frente, como um erro de parse do dpkg que não nomeia a causa real. O
  # dpkg-deb sempre existe num Debian/Ubuntu; se faltar, seguimos sem a conferência.
  if command -v dpkg-deb >/dev/null 2>&1 && ! dpkg-deb --info "$DEB" >/dev/null 2>&1; then
    echo "erro: o arquivo baixado não é um pacote .deb válido." >&2
    echo "      origem: $DEB_URL" >&2
    echo "      isso costuma ser um proxy/portal cativo devolvendo uma página HTML" >&2
    echo "      no lugar do pacote. Confira a rede, ou baixe o .deb à mão da página" >&2
    echo "      do Release e rode o install.sh de dentro da pasta dele." >&2
    exit 1
  fi
  chmod 0644 "$DEB"
fi
echo "==> Pacote: $DEB"

# --- instala (apt resolve as dependências do dashboard) ---------------------
# Duas blindagens, e as duas existem por causa do `curl … | sh`:
#
# `< /dev/null` — nesse modo a ENTRADA deste script é o próprio cano que ainda
# carrega o resto do install.sh, ainda não lido pelo shell. O apt herda essa
# entrada: qualquer pergunta que ele faça (um conffile do dpkg, uma tela do
# debconf) lê dali e COME os bytes que faltavam — o bloco do `mustard init` e o
# bloco do plugin somem, ou meia linha é executada. Com /dev/null na entrada, o
# apt não tem de onde comer.
#
# DEBIAN_FRONTEND=noninteractive — o `-y` responde a pergunta do apt, não as do
# debconf/dpkg. Vai via `env` porque o sudo zera o ambiente (env_reset): um
# `DEBIAN_FRONTEND=… sudo apt-get …` seria descartado antes de chegar ao apt.
APT_ENV="env DEBIAN_FRONTEND=noninteractive"

echo "==> Atualizando índices do apt (para resolver as dependências do pacote)…"
$SUDO $APT_ENV apt-get update </dev/null \
  || echo "  aviso: 'apt-get update' falhou — seguindo (deps podem já estar em cache)."

echo "==> Instalando o Mustard (CLI + dashboard)…"
# O caminho absoluto faz o apt tratar como arquivo local e puxar as dependências.
$SUDO $APT_ENV apt-get install -y "$DEB" </dev/null

# --- opcional: prepara um projeto -------------------------------------------
INIT_RAN=0
if [ -n "$TARGET" ]; then
  # Já validado e resolvido para caminho absoluto na leitura dos argumentos.
  #
  # Como o apt exige root e o tutorial pede sudo, a forma natural do comando é
  # `curl … | sudo sh -s -- ~/meu-projeto` — e aí o `mustard init` rodaria como
  # root, deixando `.claude/` e `mustard.json` com dono root:root. Toda escrita
  # posterior do dono do projeto (o próprio Claude Code, o `mustard init` de
  # novo) morreria com EACCES. Quando o sudo preencheu SUDO_USER sabemos QUEM
  # chamou, e o init volta a rodar como essa pessoa. Root de verdade (login root,
  # sem SUDO_USER) roda como está: aí o dono do projeto é mesmo o root.
  INIT_AS=""
  if [ "$(id -u)" -eq 0 ] && [ -n "${SUDO_USER:-}" ] && [ "$SUDO_USER" != "root" ] \
     && command -v sudo >/dev/null 2>&1; then
    # -H para o HOME ser o de $SUDO_USER, e não o do root.
    INIT_AS="sudo -H -u $SUDO_USER"
    echo "==> Rodando 'mustard init' em $TARGET (como $SUDO_USER, não como root)"
  else
    echo "==> Rodando 'mustard init' em $TARGET"
  fi
  # `set -e` mataria o script inteiro se o init falhasse (templates fora do
  # lugar, alvo sem permissão de escrita, RTK ausente) — e o usuário sairia com
  # status != 0 logo depois de um apt que DEU CERTO, sem ler uma palavra sobre o
  # plugin, que é justamente o passo que este trabalho existe para não perder.
  # Dentro de um `if`, a falha vira desvio em vez de morte súbita.
  if ( cd "$TARGET" && $INIT_AS mustard init --yes ); then
    INIT_RAN=1
  else
    echo "aviso: o 'mustard init' falhou em $TARGET — o Mustard em si ESTÁ" >&2
    echo "       instalado (o apt terminou). Rode o init à mão para preparar o" >&2
    echo "       projeto:" >&2
    echo "           cd $TARGET && mustard init" >&2
  fi
fi

echo
echo "==> Pronto."
echo "    CLI:        mustard --version   (e mustard-rt, scan, rtk)"
echo "    Dashboard:  rode  mustard-dashboard  na pasta dos seus projetos —"
echo "                ele serve em http://127.0.0.1:7777/ e abre o navegador."
echo "                De outra máquina:  mustard-dashboard --host 0.0.0.0"
echo
# --- o passo do plugin -------------------------------------------------------
# ESTE bloco é a razão de a instalação não terminar no `apt-get install`. O .deb
# atualiza a cópia do sistema (/usr/bin/mustard-rt); o Claude Code executa a
# cópia do PLUGIN (~/.claude/plugins/cache/…), porque o plugin prepende o bin/
# dele ao PATH. Atualizar só a primeira deixa a máquina rodando a versão velha —
# medido em campo (2026-08-28): 0.1.55 instalada, plugin parado em 0.1.54.
#
# Até aqui este bloco só IMPRIMIA duas linhas pedindo que a pessoa fizesse o
# passo à mão, e enquanto ninguém as digitasse, nada acontecia. Agora o passo é
# executado; imprimir vira o caminho de EXCEÇÃO, para quando ele não puder rodar.
#
# O script mora dentro do pacote e não ao lado deste arquivo, porque no
# `curl … | sh` nada além deste install.sh chega ao disco — depois do apt, ele
# está em /usr/lib/mustard/. Um pacote anterior a esta mudança não o traz, e aí
# o bloco degrada para o texto de sempre.
PLUGIN_STEP="/usr/lib/mustard/plugin-step.sh"
if [ -x "$PLUGIN_STEP" ]; then
  # Dentro de um `if` porque `set -e` mataria o script se o passo falhasse — e o
  # passo é fail-open justamente para nunca derrubar uma instalação que deu certo.
  if ! sh "$PLUGIN_STEP"; then
    echo "aviso: o passo do plugin não concluiu; o Mustard em si ESTÁ instalado." >&2
  fi
elif [ "$INIT_RAN" -eq 1 ]; then
  echo "    Falta o plugin do Claude Code — é ele que traz os comandos /mustard:*,"
  echo "    os hooks e o MCP de memória. As duas linhas /plugin … estão logo acima,"
  echo "    na saída do 'mustard init' (\"Next: 1.\"): elas são digitadas DENTRO do"
  echo "    Claude Code, não no terminal. Depois feche e abra o Claude Code para os"
  echo "    hooks entrarem."
else
  echo "    Falta o plugin do Claude Code — é ele que traz os comandos /mustard:*,"
  echo "    os hooks e o MCP de memória. Abra o Claude Code no projeto (claude) e"
  echo "    digite estas duas linhas DENTRO dele (não são comandos de terminal):"
  echo "        /plugin marketplace add rubensrpj/mustard"
  echo "        /plugin install mustard@mustard-local"
  echo "    O \"@mustard-local\" é o NOME do marketplace, não um caminho. Depois"
  echo "    feche e abra o Claude Code para os hooks entrarem."
fi
echo
echo "    Preparar um projeto:  cd /caminho/do/projeto && mustard init"
echo "    Desinstalar tudo:     $SUDO apt remove mustard"
