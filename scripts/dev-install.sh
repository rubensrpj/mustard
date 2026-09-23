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
# O instalador oficial (packaging/installer/plugin-step.sh) recusa escrever
# direto no cache do plugin de propósito — aquele layout é interno e pode
# mudar sem aviso. Este script FAZ ISSO, e só porque é uma ferramenta de
# desenvolvimento: quem o roda já sabe da versão em uso e aceita o risco de um
# layout que pode mudar.
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
# datada (--restore desfaz, copiando de volta). A pasta datada é criada sem
# sobrescrever uma que já exista: duas rodadas no mesmo segundo produziriam o
# mesmo nome, e a segunda recusa antes de tocar em qualquer arquivo, para não
# gravar por cima do backup da primeira o programa que a primeira já trocou.
#
# A cópia do sistema pede administrador: só é trocada quando o script já
# roda como root. Como o `sudo` do Ubuntu troca o HOME para `/root` e não
# tem `~/.cargo/bin` no PATH, o comando impresso para rodar com sudo não é
# este mesmo script sem argumento nenhum — é este script com
# `--system-copy-only <pasta-dos-binários-já-compilados> <pasta-de-backup>`,
# um modo que não compila nada, não procura a cópia do plugin e só troca a
# cópia do sistema a partir dos binários que este mesmo processo (sem root)
# acabou de compilar.
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
# AO TERMINAR de trocar as cópias, o script resolve mustard, mustard-rt e scan
# pelo CAMINHO DE BUSCA (`command -v`) e avisa quando quem responde não é
# nenhuma das duas cópias trocadas. Trocar os arquivos das cópias não troca
# quem o PATH escolhe: em 21/09/2026 o script disse trocado com sucesso e quem
# rodou passou a testar uma cópia velha em ~/.cargo/bin, que vem antes no
# caminho, três commits atrás.
#
# Uso:
#   scripts/dev-install.sh                        # compila e troca no lugar
#   scripts/dev-install.sh --update-project <dir> # e roda o `mustard init`
#                                                  # novo nesse projeto
#   scripts/dev-install.sh --restore <pasta-datada>  # desfaz uma troca: o
#                                                  # plugin sempre; o sistema
#                                                  # só quando já roda como
#                                                  # root, senão imprime o
#                                                  # comando pronto com sudo
#   scripts/dev-install.sh --system-copy-only <pasta-dos-binários> <pasta-de-backup>
#                                                  # só a cópia do sistema,
#                                                  # sem compilar (é o comando
#                                                  # que o script acima
#                                                  # imprime pronto com sudo)
#   scripts/dev-install.sh --restore-system-only <pasta-de-backup-do-sistema>
#                                                  # só devolve a cópia do
#                                                  # sistema, sem procurar o
#                                                  # plugin (é o comando que
#                                                  # o --restore acima
#                                                  # imprime pronto com sudo)
# ============================================================================
set -eu

usage() {
  echo "uso: $(basename -- "$0") [--update-project <pasta-do-projeto>]"
  echo "     $(basename -- "$0") --restore <pasta-datada-do-backup>"
  echo "     $(basename -- "$0") --system-copy-only <pasta-dos-binarios> <pasta-de-backup>"
  echo "     $(basename -- "$0") --restore-system-only <pasta-de-backup-do-sistema>"
}

RESTORE_DIR=""
UPDATE_PROJECT=""
SYSTEM_ONLY_RELEASE_DIR=""
SYSTEM_ONLY_BACKUP_DIR=""
RESTORE_SYSTEM_ONLY_DIR=""
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
    --system-copy-only)
      [ $# -ge 3 ] || { echo "erro: --system-copy-only precisa da pasta dos binários e da pasta de backup." >&2; exit 1; }
      SYSTEM_ONLY_RELEASE_DIR="$2"
      SYSTEM_ONLY_BACKUP_DIR="$3"
      shift 3
      ;;
    --restore-system-only)
      [ $# -ge 2 ] || { echo "erro: --restore-system-only precisa da pasta de backup do sistema." >&2; exit 1; }
      RESTORE_SYSTEM_ONLY_DIR="$2"
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
MODOS=0
if [ -n "$RESTORE_DIR" ]; then MODOS=$((MODOS + 1)); fi
if [ -n "$UPDATE_PROJECT" ]; then MODOS=$((MODOS + 1)); fi
if [ -n "$SYSTEM_ONLY_RELEASE_DIR" ]; then MODOS=$((MODOS + 1)); fi
if [ -n "$RESTORE_SYSTEM_ONLY_DIR" ]; then MODOS=$((MODOS + 1)); fi
if [ "$MODOS" -gt 1 ]; then
  echo "erro: --restore, --update-project, --system-copy-only e --restore-system-only não se combinam." >&2
  exit 1
fi

# --- onde este script e o repo moram ----------------------------------------
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
SCRIPT_PATH="$SCRIPT_DIR/$(basename -- "$0")"
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)

# --- as duas cópias ----------------------------------------------------------
CLAUDE_DIR="${CLAUDE_CONFIG_DIR:-${HOME:-}/.claude}"
SYSTEM_DIR="${MUSTARD_DEV_INSTALL_SYSTEM_DIR:-/usr/lib/mustard}"

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

# --- troca um binário dentro da cópia do sistema, deixando-o de root:root,
#     modo 755 — cp -p herdaria o dono e o modo de quem compilou (o usuário,
#     às vezes com escrita para o grupo), que não é o que a cópia do sistema,
#     feita como root, deve deixar para trás.
swap_file_root_owned() {
  fonte=$1
  destino=$2
  backup=$3
  if [ -f "$destino" ]; then
    mkdir -p "$(dirname -- "$backup")"
    cp -p "$destino" "$backup"
  fi
  mkdir -p "$(dirname -- "$destino")"
  cp "$fonte" "$destino"
  chown root:root "$destino"
  chmod 755 "$destino"
}

# --- o mesmo, para uma pasta inteira (os moldes da cópia do sistema) --------
swap_tree_root_owned() {
  fonte_dir=$1
  destino_dir=$2
  backup_dir=$3
  if [ -d "$destino_dir" ]; then
    mkdir -p "$(dirname -- "$backup_dir")"
    cp -pR "$destino_dir" "$backup_dir"
  fi
  rm -rf "$destino_dir"
  mkdir -p "$(dirname -- "$destino_dir")"
  cp -R "$fonte_dir" "$destino_dir"
  chown -R root:root "$destino_dir"
}

# --- o oposto de cada uma, para o --restore ----------------------------------
# Sem backup, o destino não existia antes da troca — o desfazer tem de
# APAGAR o que a troca criou, não deixá-lo para trás.
restore_file() {
  backup=$1
  destino=$2
  if [ -f "$backup" ]; then
    mkdir -p "$(dirname -- "$destino")"
    cp -p "$backup" "$destino"
  else
    rm -f "$destino"
  fi
}
restore_tree() {
  backup_dir=$1
  destino_dir=$2
  if [ -d "$backup_dir" ]; then
    rm -rf "$destino_dir"
    mkdir -p "$(dirname -- "$destino_dir")"
    cp -pR "$backup_dir" "$destino_dir"
  else
    rm -rf "$destino_dir"
  fi
}

# --- --system-copy-only: só a cópia do sistema, sem compilar e sem procurar
#     a cópia do plugin. É o comando que a rodada sem root imprime pronto com
#     sudo, para não repetir a compilação nem a busca da cópia do plugin como
#     root (onde o HOME e o PATH do sudo não servem para nada disso).
if [ -n "$SYSTEM_ONLY_RELEASE_DIR" ]; then
  [ "$(id -u)" -eq 0 ] || { echo "erro: --system-copy-only precisa rodar como root." >&2; exit 1; }
  [ -d "$SYSTEM_ONLY_RELEASE_DIR" ] || { echo "erro: pasta de binários inexistente: $SYSTEM_ONLY_RELEASE_DIR" >&2; exit 1; }
  for b in mustard mustard-rt scan; do
    [ -x "$SYSTEM_ONLY_RELEASE_DIR/$b" ] || { echo "erro: $SYSTEM_ONLY_RELEASE_DIR/$b não existe ou não é executável." >&2; exit 1; }
  done
  if [ -e "$SYSTEM_ONLY_BACKUP_DIR" ]; then
    echo "erro: a pasta de backup já existe: $SYSTEM_ONLY_BACKUP_DIR" >&2
    exit 1
  fi
  mkdir -p "$(dirname -- "$SYSTEM_ONLY_BACKUP_DIR")"
  mkdir "$SYSTEM_ONLY_BACKUP_DIR"
  echo "==> Trocando a cópia do sistema ($SYSTEM_DIR)…"
  for b in mustard mustard-rt scan; do
    swap_file_root_owned "$SYSTEM_ONLY_RELEASE_DIR/$b" "$SYSTEM_DIR/bin/$b" "$SYSTEM_ONLY_BACKUP_DIR/bin/$b"
  done
  swap_tree_root_owned "$REPO_ROOT/apps/cli/templates" "$SYSTEM_DIR/templates" "$SYSTEM_ONLY_BACKUP_DIR/templates"
  # A pasta datada nasce de root (este modo só roda assim, via sudo) dentro da
  # pasta pessoal de quem chamou — sem devolver o DONO DAS PASTAS a essa
  # pessoa, ela não apaga o próprio backup depois sem sudo de novo. Só as
  # pastas trocam de dono, nunca os arquivos: apagar um arquivo pede escrita
  # na pasta que o contém, não posse do arquivo — e os binários dentro
  # continuam de root:root, do jeito que um --restore-system-only posterior
  # (cp -p, como root) precisa achá-los para devolver a cópia do sistema como
  # estava.
  if [ -n "${SUDO_UID:-}" ] && [ -n "${SUDO_GID:-}" ]; then
    find "$SYSTEM_ONLY_BACKUP_DIR" -type d -exec chown "$SUDO_UID:$SUDO_GID" {} +
  fi
  echo "==> Originais preservados em: $SYSTEM_ONLY_BACKUP_DIR"
  exit 0
fi

# --- --restore-system-only: só devolve a cópia do sistema, sem procurar a
#     cópia do plugin. É o comando que a rodada sem root imprime pronto com
#     sudo quando um --restore acha a parte do sistema na pasta datada — pelo
#     mesmo motivo do --system-copy-only: o HOME e o PATH do sudo não servem
#     para achar o plugin nem para compilar nada.
if [ -n "$RESTORE_SYSTEM_ONLY_DIR" ]; then
  [ "$(id -u)" -eq 0 ] || { echo "erro: --restore-system-only precisa rodar como root." >&2; exit 1; }
  [ -d "$RESTORE_SYSTEM_ONLY_DIR" ] || { echo "erro: pasta de backup do sistema inexistente: $RESTORE_SYSTEM_ONLY_DIR" >&2; exit 1; }
  echo "==> Restaurando a cópia do sistema ($SYSTEM_DIR)…"
  for b in mustard mustard-rt scan; do
    restore_file "$RESTORE_SYSTEM_ONLY_DIR/bin/$b" "$SYSTEM_DIR/bin/$b"
  done
  restore_tree "$RESTORE_SYSTEM_ONLY_DIR/templates" "$SYSTEM_DIR/templates"
  echo "==> Sistema restaurado a partir de $RESTORE_SYSTEM_ONLY_DIR."
  exit 0
fi

# --- a versão que decide a pasta do plugin ----------------------------------
MANIFESTO="$REPO_ROOT/plugin/.claude-plugin/plugin.json"
VERSAO=$(sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$MANIFESTO" 2>/dev/null | head -n 1)
if [ -z "$VERSAO" ]; then
  echo "erro: não consegui ler \"version\" em $MANIFESTO." >&2
  exit 1
fi

# Lista, uma por linha, cada cópia do plugin na versão do manifesto. Mais de
# uma linha é ambíguo — quem chama decide se isso é erro fatal (instalação)
# ou só motivo para pular essa parte (desfazer).
listar_copias_do_plugin() {
  raiz="$CLAUDE_DIR/plugins/cache"
  [ -d "$raiz" ] || return 0
  find "$raiz" -maxdepth 3 -type d -path "*/mustard/$VERSAO" 2>/dev/null
}
contar_copias_do_plugin() {
  # `grep -c` sai com status 1 quando não acha nada — sob `set -e`, isso
  # derrubaria o script antes de a chamada decidir o que fazer com zero.
  printf '%s\n' "$1" | grep -c '.' || true
}

# --- --restore: desfaz uma troca anterior -----------------------------------
# A cópia do plugin volta sempre que achada, aqui mesmo — a busca só roda
# DEPOIS de saber que estamos desfazendo, e a ausência dela não é fatal: uma
# pessoa pode desfazer depois de já ter desinstalado o plugin, e aí só a
# cópia do sistema importa. A cópia do sistema pede administrador, como na
# instalação: só volta direto quando este processo já é root; senão, o script
# não toca nela — imprime o comando pronto com sudo (--restore-system-only,
# que não procura o plugin nem compila nada).
if [ -n "$RESTORE_DIR" ]; then
  [ -d "$RESTORE_DIR" ] || { echo "erro: pasta de backup inexistente: $RESTORE_DIR" >&2; exit 1; }
  PLUGIN_COPIES=$(listar_copias_do_plugin)
  PLUGIN_COPY_COUNT=$(contar_copias_do_plugin "$PLUGIN_COPIES")
  if [ "$PLUGIN_COPY_COUNT" -gt 1 ]; then
    echo "erro: achei mais de uma cópia do plugin na versão $VERSAO:" >&2
    printf '%s\n' "$PLUGIN_COPIES" >&2
    echo "      apague a que não deve ser usada antes de desfazer." >&2
    exit 1
  fi
  if [ "$PLUGIN_COPY_COUNT" -eq 1 ]; then
    PLUGIN_COPY="$PLUGIN_COPIES"
    echo "==> Restaurando a cópia do plugin ($PLUGIN_COPY)…"
    for b in mustard mustard-rt scan; do
      restore_file "$RESTORE_DIR/plugin/bin/$b" "$PLUGIN_COPY/bin/$b"
    done
    restore_tree "$RESTORE_DIR/plugin/bin/templates" "$PLUGIN_COPY/bin/templates"
    restore_tree "$RESTORE_DIR/plugin/commands" "$PLUGIN_COPY/commands"
    restore_tree "$RESTORE_DIR/plugin/hooks" "$PLUGIN_COPY/hooks"
    restore_tree "$RESTORE_DIR/plugin/output-styles" "$PLUGIN_COPY/output-styles"
    echo "==> Restaurado a partir de $RESTORE_DIR."
  else
    echo "==> Sem cópia do plugin na versão $VERSAO — pulando essa parte do desfazer." >&2
  fi
  if [ -d "$RESTORE_DIR/system" ]; then
    if [ "$(id -u)" -eq 0 ]; then
      echo "==> Restaurando a cópia do sistema ($SYSTEM_DIR)…"
      for b in mustard mustard-rt scan; do
        restore_file "$RESTORE_DIR/system/bin/$b" "$SYSTEM_DIR/bin/$b"
      done
      restore_tree "$RESTORE_DIR/system/templates" "$SYSTEM_DIR/templates"
    else
      echo "==> A cópia do sistema ($SYSTEM_DIR) pede administrador. Para restaurá-la:"
      echo "        sudo env MUSTARD_DEV_INSTALL_SYSTEM_DIR=\"$SYSTEM_DIR\" sh \"$SCRIPT_PATH\" --restore-system-only \"$RESTORE_DIR/system\""
    fi
  fi
  exit 0
fi

# --- a cópia do plugin: obrigatória para instalar, e só procurada aqui, DEPOIS
#     do ramo do desfazer acima — instalar precisa de um lugar para trocar os
#     arquivos, então zero ou mais de uma cópia é erro fatal aqui (o desfazer,
#     acima, trata os dois casos de outro jeito).
PLUGIN_COPIES=$(listar_copias_do_plugin)
PLUGIN_COPY_COUNT=$(contar_copias_do_plugin "$PLUGIN_COPIES")
if [ "$PLUGIN_COPY_COUNT" -gt 1 ]; then
  echo "erro: achei mais de uma cópia do plugin na versão $VERSAO:" >&2
  printf '%s\n' "$PLUGIN_COPIES" >&2
  echo "      apague a que não deve ser usada antes de rodar de novo." >&2
  exit 1
fi
if [ "$PLUGIN_COPY_COUNT" -eq 0 ]; then
  echo "erro: não achei a cópia do plugin na versão $VERSAO, dentro de" >&2
  echo "      $CLAUDE_DIR/plugins/cache — instale o plugin nesta versão antes:" >&2
  echo "      /plugin marketplace add rubensrpj/mustard && /plugin install mustard@mustard-local" >&2
  exit 1
fi
PLUGIN_COPY="$PLUGIN_COPIES"

# --- compila os três programas, no modo de entrega --------------------------
echo "==> Compilando mustard, mustard-rt e scan (cargo build --release)…"
( cd "$REPO_ROOT" && cargo build --release --locked --bin scan --bin mustard-rt --bin mustard )
# $CARGO_TARGET_DIR relativo é resolvido contra a RAIZ DO REPOSITÓRIO — o
# mesmo lugar de onde o `cargo build` acima rodou (o `cd "$REPO_ROOT"` do
# subshell) — nunca contra a pasta de onde esta pessoa chamou o script.
if [ -n "${CARGO_TARGET_DIR:-}" ]; then
  case "$CARGO_TARGET_DIR" in
    /*) TARGET_DIR="$CARGO_TARGET_DIR" ;;
    *) TARGET_DIR="$REPO_ROOT/$CARGO_TARGET_DIR" ;;
  esac
else
  TARGET_DIR="$REPO_ROOT/target"
fi
RELEASE_DIR="$TARGET_DIR/release"
for b in mustard mustard-rt scan; do
  [ -x "$RELEASE_DIR/$b" ] || { echo "erro: cargo build não deixou $RELEASE_DIR/$b" >&2; exit 1; }
done

# --- a pasta datada deste backup ---------------------------------------------
# Sem `-p` no `mkdir` final: duas rodadas no mesmo segundo caem no mesmo
# nome, e a segunda recusa aqui, ANTES de trocar qualquer arquivo — senão a
# pasta datada da segunda receberia o programa que a primeira já trocou, e o
# original de verdade (guardado pela primeira) seria sobrescrito.
BACKUP_ROOT="${MUSTARD_DEV_INSTALL_BACKUP_DIR:-$CLAUDE_DIR/mustard-dev-backups}"
mkdir -p "$BACKUP_ROOT"
BACKUP_DIR="$BACKUP_ROOT/$(date +%Y%m%d-%H%M%S)"
if [ -e "$BACKUP_DIR" ]; then
  echo "erro: já existe uma pasta de backup para este segundo: $BACKUP_DIR" >&2
  echo "      rode de novo daqui a pouco, para a pasta datada ter um nome novo." >&2
  exit 1
fi
mkdir "$BACKUP_DIR"

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
  echo "        sudo env MUSTARD_DEV_INSTALL_SYSTEM_DIR=\"$SYSTEM_DIR\" sh \"$SCRIPT_PATH\" --system-copy-only \"$RELEASE_DIR\" \"$BACKUP_DIR/system\""
fi

echo "==> Originais preservados em: $BACKUP_DIR"
echo "    Para desfazer: sh \"$SCRIPT_PATH\" --restore \"$BACKUP_DIR\""

# --- quem responde pelo nome no caminho de busca -----------------------------
# Trocar as duas cópias não troca quem responde no terminal: o PATH pode ter uma
# terceira cópia antes delas, e o script não tem como saber disso sem resolver o
# nome. Por isso, ao terminar, cada um dos três nomes é resolvido por
# `command -v` e comparado com as duas cópias que este script troca; quando quem
# responde é outra coisa, sai um aviso, porque o que a pessoa vai testar no
# terminal não é o que acabou de ser compilado.
caminho_real() {
  readlink -f "$1" 2>/dev/null || printf '%s\n' "$1"
}
echo "==> Quem responde no caminho de busca (PATH):"
for b in mustard mustard-rt scan; do
  # `command -v` sai com status 1 quando nada responde — sob `set -e` isso
  # derrubaria o script antes de o aviso sair.
  QUEM=$(command -v "$b" 2>/dev/null || true)
  if [ -z "$QUEM" ]; then
    echo "    aviso: nada responde por $b no caminho de busca." >&2
    continue
  fi
  QUEM_REAL=$(caminho_real "$QUEM")
  if [ "$QUEM_REAL" = "$(caminho_real "$PLUGIN_COPY/bin/$b")" ] \
    || [ "$QUEM_REAL" = "$(caminho_real "$SYSTEM_DIR/bin/$b")" ]; then
    echo "    $b responde de $QUEM — uma das cópias trocadas."
  else
    echo "    aviso: $b responde de $QUEM, que NÃO é nenhuma das cópias trocadas" >&2
    echo "           ($PLUGIN_COPY/bin/$b e $SYSTEM_DIR/bin/$b)." >&2
    echo "           Quem digitar $b roda essa outra cópia, não a recém-compilada." >&2
  fi
done

# --- opcional: roda a atualização do Mustard num projeto ---------------------
if [ -n "$UPDATE_PROJECT" ]; then
  [ -d "$UPDATE_PROJECT" ] || { echo "erro: projeto inexistente: $UPDATE_PROJECT" >&2; exit 1; }
  echo "==> Rodando a atualização do Mustard em $UPDATE_PROJECT (mustard init --yes)…"
  ( cd "$UPDATE_PROJECT" && "$RELEASE_DIR/mustard" init --yes )
fi
