#!/usr/bin/env sh
# ============================================================================
# Mustard — instalador completo (Ubuntu / Debian)
#
# Instala o pacote .deb que traz TUDO: os binários do CLI (mustard, mustard-rt,
# mustard-mcp, scan, rtk) E o Mustard Dashboard (app desktop). Usa `apt`, que
# resolve sozinho as dependências de sistema do dashboard (webkit2gtk-4.1, gtk).
#
# Layout instalado (gerenciado pelo apt, removível com `sudo apt remove mustard`):
#   /usr/lib/mustard/bin/        binários reais (CLI + dashboard)
#   /usr/lib/mustard/templates/  a carga do `mustard init`
#   /usr/bin/mustard, …          symlinks no PATH (criados pelo pacote)
#   atalho "Mustard Dashboard" no menu de aplicativos
#
# Uso:
#   curl -fsSL https://github.com/rubensrpj/mustard/releases/latest/download/install.sh | sh
#                                 # instala baixando o .deb do último Release
#   ./install.sh                  # instala tudo
#   ./install.sh /caminho/projeto # também roda `mustard init` nesse projeto
#   ./install.sh --dry-run        # só mostra o que seria instalado; não instala
#   MUSTARD_VERSION=0.1.35 ./install.sh   # fixa a versão em vez do último Release
#
# O .deb ao lado deste script é a fonte preferida; quando não há nenhum (é o caso
# do `curl … | sh`, que não deixa arquivo nenhum em disco) o instalador baixa o
# pacote do Release do GitHub.
# ============================================================================
set -eu

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

# Drop the download directory on every exit path, failures and signals included.
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
      TARGET="$arg"
      ;;
  esac
done

# --- precisa de apt (Ubuntu/Debian) -----------------------------------------
# --dry-run only resolves and prints, so it must not demand apt.
if [ "$DRY_RUN" -eq 0 ] && ! command -v apt-get >/dev/null 2>&1; then
  echo "erro: este instalador usa apt (Ubuntu/Debian). Não encontrei o apt-get." >&2
  exit 1
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
      _url=$(curl -fsSL --connect-timeout 10 --max-time 60 \
               -o /dev/null -w '%{url_effective}' "$LATEST_URL" 2>/dev/null) || return 1
      ;;
    wget)
      # -q would also silence -S, so the headers are read from the captured stderr.
      _url=$(wget -S --max-redirect=0 --tries=1 --timeout=20 -O /dev/null "$LATEST_URL" 2>&1 \
               | sed -n 's/^[[:space:]]*Location:[[:space:]]*\([^[:space:]]*\).*/\1/p' \
               | head -1)
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
    v[0-9]*) ;;
    *) return 1 ;;
  esac
  case "$_tag" in
    *[!A-Za-z0-9._-]*) return 1 ;;
  esac
  printf '%s\n' "$_tag"
}

download_file() {
  case "$DOWNLOADER" in
    curl) curl -fL --progress-bar --connect-timeout 15 --retry 2 -o "$2" "$1" ;;
    wget) wget --tries=3 --timeout=30 -O "$2" "$1" ;;
    *)    return 1 ;;
  esac
}

# --- 1) .deb ao lado deste script (fonte preferida) -------------------------
# $0 is not a usable path when the script arrives through a pipe (`curl | sh`),
# so the directory is only derived when $0 really points at a file.
SCRIPT_DIR=""
if [ -f "$0" ]; then
  SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
fi
if [ -n "$SCRIPT_DIR" ]; then
  if [ -n "$VERSION" ]; then
    # An explicit version only accepts the local .deb of that same version.
    [ -f "$SCRIPT_DIR/mustard_${VERSION}_amd64.deb" ] \
      && DEB="$SCRIPT_DIR/mustard_${VERSION}_amd64.deb" || true
  else
    DEB=$(ls "$SCRIPT_DIR"/mustard_*_amd64.deb 2>/dev/null | head -1 || true)
  fi
fi

# --- 2) sem .deb local: o Release do GitHub ---------------------------------
if [ -z "$DEB" ]; then
  # --dry-run só imprime, então sobrevive sem curl/wget (nomeia a URL mesmo assim).
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
    echo "      sem rede? tente de novo, ou fixe a versão:" >&2
    echo "      MUSTARD_VERSION=0.1.35 ./install.sh" >&2
    exit 1
  fi
fi

# --- 3) --dry-run: mostra o que seria feito e sai ---------------------------
# Resolution already happened above; nothing here touches apt or the network, so
# this path exits 0 offline as well, naming the URL it would have used.
if [ "$DRY_RUN" -eq 1 ]; then
  echo "==> --dry-run: nada será instalado."
  if [ -n "$DEB" ]; then
    echo "    Pacote:  $DEB"
    echo "    Origem:  arquivo local (ao lado do install.sh)"
  elif [ -n "$DEB_URL" ]; then
    echo "    Pacote:  mustard_${VERSION}_amd64.deb"
    echo "    Origem:  $DEB_URL"
  else
    echo "    Pacote:  mustard_<versao>_amd64.deb"
    echo "    Origem:  $LATEST_URL  (a versão não pôde ser resolvida agora — sem rede?)"
  fi
  echo "    Comando: apt-get install -y <pacote>"
  if [ -n "$TARGET" ]; then
    echo "    Depois:  mustard init --yes em $TARGET"
  fi
  exit 0
fi

# --- sudo só quando não-root ------------------------------------------------
if [ "$(id -u)" -ne 0 ]; then
  if command -v sudo >/dev/null 2>&1; then
    SUDO="sudo"
  else
    echo "erro: não sou root e não há sudo. Rode como root ou instale o sudo." >&2
    exit 1
  fi
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
  chmod 0644 "$DEB"
fi
echo "==> Pacote: $DEB"

# --- instala (apt resolve as dependências do dashboard) ---------------------
echo "==> Atualizando índices do apt (para resolver as dependências do dashboard)…"
$SUDO apt-get update || echo "  aviso: 'apt-get update' falhou — seguindo (deps podem já estar em cache)."

echo "==> Instalando o Mustard (CLI + dashboard)…"
# O caminho absoluto faz o apt tratar como arquivo local e puxar as dependências.
$SUDO apt-get install -y "$DEB"

# --- opcional: prepara um projeto -------------------------------------------
if [ -n "$TARGET" ]; then
  [ -d "$TARGET" ] || { echo "erro: projeto-alvo não existe: $TARGET" >&2; exit 1; }
  TARGET=$(CDPATH= cd -- "$TARGET" && pwd)
  echo "==> Rodando 'mustard init' em $TARGET"
  ( cd "$TARGET" && mustard init --yes )
fi

echo
echo "==> Pronto."
echo "    CLI:        mustard --version   (e mustard-rt, scan, rtk)"
echo "    Dashboard:  procure \"Mustard Dashboard\" no menu de aplicativos,"
echo "                ou rode  mustard-dashboard  no terminal."
echo
echo "    Preparar um projeto:  cd /caminho/do/projeto && mustard init"
echo "    Desinstalar tudo:     $SUDO apt remove mustard"
