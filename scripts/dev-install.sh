#!/usr/bin/env sh
# ============================================================================
# dev-install.sh — instalação local, para desenvolvimento.
#
# Compila, no modo de entrega (`cargo build --release`), os três programas
# desta branch (mustard, mustard-rt, scan) e troca, NO LUGAR, os arquivos de
# duas cópias que já existem numa máquina com o Mustard instalado:
#
#   cópia do PLUGIN ... ~/.claude/plugins/cache/<marketplace>/mustard/<v>/
#                        — a que o Claude Code de fato executa (ver
#                        packaging/installer/plugin-step.sh)
#   cópia do SISTEMA .... /usr/lib/mustard/ — a que o pacote oficial (.deb)
#                        instala (ver packaging/linux/build-deb.sh)
#
# A versão que decide QUAL pasta do cache é a cópia do plugin vem do
# manifesto desta branch (plugin/.claude-plugin/plugin.json), não de uma
# variável — é assim que o script acerta a pasta certa mesmo sem a pessoa
# saber de cor a versão instalada.
#
# O SELO DE VERSÃO (bin/.version, dentro da cópia do plugin) nunca é tocado.
# Ele é o que o `mustard-boot` lê para decidir se baixa binários novos
# (packaging/installer/plugin-step.sh, plugin/bin/mustard-boot): mexer nele
# faria o plugin achar que já está na versão do manifesto, e a PRÓXIMA
# instalação oficial pararia de sobrescrever os arquivos que este script
# trocou à mão.
#
# ANTES de trocar qualquer arquivo, o original é copiado para uma pasta
# datada (--restore desfaz, copiando de volta). A cópia do sistema pede
# administrador: só é trocada quando o script já roda como root; senão, o
# comando pronto para rodar com sudo é impresso, e nada na cópia do sistema é
# tocado.
#
# Pastas trocáveis por variável de ambiente (para o teste, que não tem root
# nem um ~/.claude de verdade):
#   CLAUDE_CONFIG_DIR                a pasta do Claude Code (mesma variável
#                                     que o próprio Claude Code respeita);
#                                     sem ela, $HOME/.claude
#   MUSTARD_DEV_INSTALL_SYSTEM_DIR   a cópia do sistema; sem ela,
#                                     /usr/lib/mustard
#   MUSTARD_DEV_INSTALL_BACKUP_DIR   onde nascem as pastas datadas; sem ela,
#                                     <pasta do Claude Code>/mustard-dev-backups
#   CARGO_TARGET_DIR                 já respeitada pelo cargo; é dali que os
#                                     três binários compilados são copiados
#
# Nada aqui escreve em .git/config — o script não chama git nenhum.
#
# Uso:
#   scripts/dev-install.sh                        # compila e troca no lugar
#   scripts/dev-install.sh --update-project <dir> # e roda o `mustard init`
#                                                  # novo nesse projeto
#   scripts/dev-install.sh --restore <pasta-datada>  # desfaz uma troca
# ============================================================================
set -eu

usage() {
  echo "uso: $(basename -- "$0") [--update-project <pasta-do-projeto>]"
  echo "     $(basename -- "$0") --restore <pasta-datada-do-backup>"
}

RESTORE_DIR=""
UPDATE_PROJECT=""
while [ $# -gt 0 ]; do
  case "$1" in
    --restore)
      [ $# -ge 2 ] || { echo "erro: --restore precisa do caminho da pasta datada." >&2; exit 1; }
      RESTORE_DIR="$2"
      shift 2
      ;;
    --update-project)
      [ $# -ge 2 ] || { echo "erro: --update-project precisa do caminho do projeto." >&2; exit 1; }
      UPDATE_PROJECT="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "erro: opção desconhecida: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done
if [ -n "$RESTORE_DIR" ] && [ -n "$UPDATE_PROJECT" ]; then
  echo "erro: --restore e --update-project não se combinam." >&2
  exit 1
fi

# --- onde este script e o repo moram ----------------------------------------
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
SCRIPT_PATH="$SCRIPT_DIR/$(basename -- "$0")"
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)

# --- a versão que decide a pasta do plugin ----------------------------------
MANIFESTO="$REPO_ROOT/plugin/.claude-plugin/plugin.json"
VERSAO=$(sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$MANIFESTO" 2>/dev/null | head -n 1)
if [ -z "$VERSAO" ]; then
  echo "erro: não consegui ler \"version\" em $MANIFESTO." >&2
  exit 1
fi

# --- as duas cópias ----------------------------------------------------------
CLAUDE_DIR="${CLAUDE_CONFIG_DIR:-${HOME:-}/.claude}"
SYSTEM_DIR="${MUSTARD_DEV_INSTALL_SYSTEM_DIR:-/usr/lib/mustard}"

achar_copia_do_plugin() {
  raiz="$CLAUDE_DIR/plugins/cache"
  [ -d "$raiz" ] || return 0
  find "$raiz" -maxdepth 3 -type d -path "*/mustard/$VERSAO" 2>/dev/null | head -n 1
}
PLUGIN_COPY=$(achar_copia_do_plugin)
if [ -z "$PLUGIN_COPY" ]; then
  echo "erro: não achei a cópia do plugin na versão $VERSAO, dentro de" >&2
  echo "      $CLAUDE_DIR/plugins/cache — instale o plugin nesta versão antes:" >&2
  echo "      /plugin marketplace add rubensrpj/mustard && /plugin install mustard@mustard-local" >&2
  exit 1
fi

# --- troca um arquivo, com backup do que havia antes ------------------------
swap_file() {
  fonte=$1
  destino=$2
  backup=$3
  if [ -f "$destino" ]; then
    mkdir -p "$(dirname -- "$backup")"
    cp -p "$destino" "$backup"
  fi
  mkdir -p "$(dirname -- "$destino")"
  cp -p "$fonte" "$destino"
}

# --- troca uma pasta inteira, com backup da pasta que havia antes -----------
swap_tree() {
  fonte_dir=$1
  destino_dir=$2
  backup_dir=$3
  if [ -d "$destino_dir" ]; then
    mkdir -p "$(dirname -- "$backup_dir")"
    cp -pR "$destino_dir" "$backup_dir"
  fi
  rm -rf "$destino_dir"
  mkdir -p "$(dirname -- "$destino_dir")"
  cp -pR "$fonte_dir" "$destino_dir"
}

# --- o oposto de cada uma, para o --restore ----------------------------------
restore_file() {
  backup=$1
  destino=$2
  [ -f "$backup" ] || return 0
  mkdir -p "$(dirname -- "$destino")"
  cp -p "$backup" "$destino"
}
restore_tree() {
  backup_dir=$1
  destino_dir=$2
  [ -d "$backup_dir" ] || return 0
  rm -rf "$destino_dir"
  mkdir -p "$(dirname -- "$destino_dir")"
  cp -pR "$backup_dir" "$destino_dir"
}

# --- --restore: desfaz uma troca anterior -----------------------------------
if [ -n "$RESTORE_DIR" ]; then
  [ -d "$RESTORE_DIR" ] || { echo "erro: pasta de backup inexistente: $RESTORE_DIR" >&2; exit 1; }
  for b in mustard mustard-rt scan; do
    restore_file "$RESTORE_DIR/plugin/bin/$b" "$PLUGIN_COPY/bin/$b"
  done
  restore_tree "$RESTORE_DIR/plugin/bin/templates" "$PLUGIN_COPY/bin/templates"
  restore_tree "$RESTORE_DIR/plugin/commands" "$PLUGIN_COPY/commands"
  restore_tree "$RESTORE_DIR/plugin/hooks" "$PLUGIN_COPY/hooks"
  restore_tree "$RESTORE_DIR/plugin/output-styles" "$PLUGIN_COPY/output-styles"
  for b in mustard mustard-rt scan; do
    restore_file "$RESTORE_DIR/system/bin/$b" "$SYSTEM_DIR/bin/$b"
  done
  restore_tree "$RESTORE_DIR/system/templates" "$SYSTEM_DIR/templates"
  echo "==> Restaurado a partir de $RESTORE_DIR."
  exit 0
fi

# --- compila os três programas, no modo de entrega --------------------------
echo "==> Compilando mustard, mustard-rt e scan (cargo build --release)…"
( cd "$REPO_ROOT" && cargo build --release --locked --bin scan --bin mustard-rt --bin mustard )
RELEASE_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}/release"
for b in mustard mustard-rt scan; do
  [ -x "$RELEASE_DIR/$b" ] || { echo "erro: cargo build não deixou $RELEASE_DIR/$b" >&2; exit 1; }
done

# --- a pasta datada deste backup ---------------------------------------------
BACKUP_ROOT="${MUSTARD_DEV_INSTALL_BACKUP_DIR:-$CLAUDE_DIR/mustard-dev-backups}"
BACKUP_DIR="$BACKUP_ROOT/$(date +%Y%m%d-%H%M%S)"
mkdir -p "$BACKUP_DIR"

# --- cópia do plugin: os três programas, os moldes, o estilo de resposta,
#     os comandos e os ganchos. O selo de versão (bin/.version) nunca entra
#     aqui — nem para ler, nem para trocar, nem para o backup.
echo "==> Trocando a cópia do plugin ($PLUGIN_COPY)…"
for b in mustard mustard-rt scan; do
  swap_file "$RELEASE_DIR/$b" "$PLUGIN_COPY/bin/$b" "$BACKUP_DIR/plugin/bin/$b"
done
swap_tree "$REPO_ROOT/apps/cli/templates" "$PLUGIN_COPY/bin/templates" "$BACKUP_DIR/plugin/bin/templates"
swap_tree "$REPO_ROOT/plugin/commands" "$PLUGIN_COPY/commands" "$BACKUP_DIR/plugin/commands"
swap_tree "$REPO_ROOT/plugin/hooks" "$PLUGIN_COPY/hooks" "$BACKUP_DIR/plugin/hooks"
swap_tree "$REPO_ROOT/plugin/output-styles" "$PLUGIN_COPY/output-styles" "$BACKUP_DIR/plugin/output-styles"

# --- cópia do sistema: pede administrador -----------------------------------
if [ "$(id -u)" -eq 0 ]; then
  echo "==> Trocando a cópia do sistema ($SYSTEM_DIR)…"
  for b in mustard mustard-rt scan; do
    swap_file "$RELEASE_DIR/$b" "$SYSTEM_DIR/bin/$b" "$BACKUP_DIR/system/bin/$b"
  done
  swap_tree "$REPO_ROOT/apps/cli/templates" "$SYSTEM_DIR/templates" "$BACKUP_DIR/system/templates"
else
  echo "==> A cópia do sistema ($SYSTEM_DIR) pede administrador. Para trocá-la:"
  echo "        sudo env MUSTARD_DEV_INSTALL_SYSTEM_DIR=\"$SYSTEM_DIR\" \"$SCRIPT_PATH\""
fi

echo "==> Originais preservados em: $BACKUP_DIR"
echo "    Para desfazer: $SCRIPT_PATH --restore \"$BACKUP_DIR\""

# --- opcional: roda a atualização do Mustard num projeto ---------------------
if [ -n "$UPDATE_PROJECT" ]; then
  [ -d "$UPDATE_PROJECT" ] || { echo "erro: projeto inexistente: $UPDATE_PROJECT" >&2; exit 1; }
  echo "==> Rodando a atualização do Mustard em $UPDATE_PROJECT (mustard init --yes)…"
  ( cd "$UPDATE_PROJECT" && "$RELEASE_DIR/mustard" init --yes )
fi
