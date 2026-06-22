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
#   ./install.sh                  # instala tudo
#   ./install.sh /caminho/projeto # também roda `mustard init` nesse projeto
# ============================================================================
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

# --- localiza o .deb (ao lado deste script) ---------------------------------
DEB=$(ls "$SCRIPT_DIR"/mustard_*_amd64.deb 2>/dev/null | head -1 || true)
if [ -z "$DEB" ]; then
  echo "erro: não achei mustard_*_amd64.deb ao lado do install.sh." >&2
  echo "      rode o install.sh de dentro da pasta do pacote." >&2
  exit 1
fi
echo "==> Pacote: $DEB"

# --- precisa de apt (Ubuntu/Debian) -----------------------------------------
if ! command -v apt-get >/dev/null 2>&1; then
  echo "erro: este instalador usa apt (Ubuntu/Debian). Não encontrei o apt-get." >&2
  exit 1
fi

# --- sudo só quando não-root ------------------------------------------------
SUDO=""
if [ "$(id -u)" -ne 0 ]; then
  if command -v sudo >/dev/null 2>&1; then
    SUDO="sudo"
  else
    echo "erro: não sou root e não há sudo. Rode como root ou instale o sudo." >&2
    exit 1
  fi
fi

# --- instala (apt resolve as dependências do dashboard) ---------------------
echo "==> Atualizando índices do apt (para resolver as dependências do dashboard)…"
$SUDO apt-get update || echo "  aviso: 'apt-get update' falhou — seguindo (deps podem já estar em cache)."

echo "==> Instalando o Mustard (CLI + dashboard)…"
# O ./ inicial faz o apt tratar como arquivo local e puxar as dependências.
$SUDO apt-get install -y "$DEB"

# --- opcional: prepara um projeto -------------------------------------------
TARGET="${1:-}"
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
