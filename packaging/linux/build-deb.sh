#!/usr/bin/env bash
# ============================================================================
# build-deb.sh — roda DENTRO do container (packaging/linux/Dockerfile).
#
# Compila os binários do CLI (scan, mustard-rt, mustard, rtk) e empacota tudo
# num único pacote Debian:
#
#   dist/mustard_<versao>_amd64.deb
#
# Layout instalado pelo .deb:
#   /usr/lib/mustard/bin/        os binários do CLI
#   /usr/lib/mustard/templates/  a carga do `mustard init`
# E o postinst cria os symlinks em /usr/bin para tudo entrar no PATH.
#
# The .deb used to be built by EXTRACTING the one the desktop-app bundler
# produced and injecting the CLI into it — that is where the icons and the
# webkit2gtk/gtk `Depends` came from. That bundler is gone: the tree below is
# written from scratch, and the dependency list shrank to the C runtime every
# Rust binary already needs.
#
# Por que /usr/lib/mustard/bin + symlinks (e não /usr/bin direto): o mustard
# resolve a pasta templates como `<dir-do-exe>/../templates`. Com os binários
# reais juntos em /usr/lib/mustard/bin, `../templates` aponta para
# /usr/lib/mustard/templates. current_exe() resolve o symlink para o caminho
# real, então a resolução funciona via /usr/bin também.
#
# Montagens esperadas (feitas pelo build-packages.ps1):
#   /work   -> repo (somente leitura efetiva; copiamos para /build)
#   /dist   -> saída (recebe o .deb + instalador + tutorial)
# ============================================================================
set -euo pipefail

REPO=/work
BUILD=/build
DIST=/dist
CARGO_TARGET=/tmp/cli-target

CLI_BINS="scan mustard-rt mustard"

echo "==> [1/5] copiando o repo para área de build isolada ($BUILD)"
mkdir -p "$BUILD"
rsync -a --delete \
  --exclude='.git/' \
  --exclude='target/' \
  --exclude='target-qa/' \
  --exclude='node_modules/' \
  --exclude='dist/' \
  "$REPO"/ "$BUILD"/

# The version used to come from the desktop shell's config file, which no
# longer exists. The
# release job exports MUSTARD_RELEASE_VERSION (it is also what gets compiled
# into the binaries); a local run falls back to the workspace version, which
# `bump-on-main` keeps equal to plugin.json.
VERSION="${MUSTARD_RELEASE_VERSION:-}"
if [ -z "$VERSION" ]; then
  VERSION=$(sed -n '0,/^version = "/s/^version = "\([^"]*\)".*/\1/p' "$BUILD/Cargo.toml" | head -1)
fi
[ -n "$VERSION" ] || { echo "erro: não consegui resolver a versão (MUSTARD_RELEASE_VERSION ou Cargo.toml)" >&2; exit 1; }
echo "    versão: $VERSION"

# --- 2. binários (workspace) ------------------------------------------------
echo "==> [2/5] cargo build --release (CLI)"
( cd "$BUILD" && CARGO_TARGET_DIR="$CARGO_TARGET" MUSTARD_RELEASE_VERSION="$VERSION" \
    cargo build --release --locked \
      --bin scan --bin mustard-rt --bin mustard )

# --- 3. rtk (binário pré-compilado oficial) ---------------------------------
echo "==> [3/5] obtendo o rtk"
RTK=""
curl -fsSL https://raw.githubusercontent.com/rtk-ai/rtk/master/install.sh | sh || true
for p in "$HOME/.local/bin/rtk" "$HOME/.cargo/bin/rtk" /opt/cargo/bin/rtk \
         /usr/local/bin/rtk /usr/bin/rtk; do
  if [ -x "$p" ]; then RTK="$p"; echo "    rtk: $p"; break; fi
done
[ -n "$RTK" ] || { echo "erro: rtk não pôde ser obtido — pacote incompleto." >&2; exit 1; }

# --- 4. monta a árvore do .deb ----------------------------------------------
echo "==> [4/5] montando o .deb"
MERGE=/tmp/merge
rm -rf "$MERGE"
mkdir -p "$MERGE/DEBIAN" \
         "$MERGE/usr/lib/mustard/bin" \
         "$MERGE/usr/lib/mustard/templates"

# 4a. binários + rtk + templates.
for b in $CLI_BINS; do
  cp "$CARGO_TARGET/release/$b" "$MERGE/usr/lib/mustard/bin/$b"
done
cp "$RTK" "$MERGE/usr/lib/mustard/bin/rtk"
chmod 0755 "$MERGE"/usr/lib/mustard/bin/*
cp -R "$BUILD/apps/cli/templates/." "$MERGE/usr/lib/mustard/templates/"

# 4a-bis. o passo do plugin. Ele NÃO fica em bin/ de propósito: bin/ inteiro
# entra no PATH via symlinks em /usr/bin (passo 5c), e este script não é um
# comando que alguém digita — é uma etapa que o install.sh chama pelo caminho
# absoluto. No `curl … | sh` nada além do install.sh chega ao disco, então
# embarcá-lo aqui é o que torna o passo alcançável depois do apt.
cp "$REPO/packaging/installer/plugin-step.sh" "$MERGE/usr/lib/mustard/plugin-step.sh"
chmod 0755 "$MERGE/usr/lib/mustard/plugin-step.sh"

# 4b. control. Depends shrank with the desktop shell: what is left is the C runtime any Rust
#     binary links. webkit2gtk-4.1/gtk-3/librsvg/appindicator are gone, and with
#     them the reason the package could not be installed on older systems for
#     anything but glibc.
INSTALLED_SIZE=$(du -k -s "$MERGE/usr" | cut -f1)
cat > "$MERGE/DEBIAN/control" <<EOF
Package: mustard
Version: $VERSION
Architecture: amd64
Maintainer: Atiz <rubens@atiz.com.br>
Section: utils
Priority: optional
Installed-Size: $INSTALLED_SIZE
Depends: libc6 (>= 2.35), libgcc-s1
Description: Mustard — harness de pipeline para Claude Code
 Instalação completa do Mustard: os binários de linha de comando
 (mustard, mustard-rt, scan, rtk) num único pacote.
EOF

# 4c. maintainer scripts: symlinks em /usr/bin (entram no PATH).
cat > "$MERGE/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
for b in mustard mustard-rt scan rtk; do
  ln -sf "/usr/lib/mustard/bin/$b" "/usr/bin/$b"
done
exit 0
EOF
cat > "$MERGE/DEBIAN/prerm" <<'EOF'
#!/bin/sh
set -e
for b in mustard mustard-rt scan rtk; do
  rm -f "/usr/bin/$b"
done
exit 0
EOF
chmod 0755 "$MERGE/DEBIAN/postinst" "$MERGE/DEBIAN/prerm"

# 4d. md5sums.
( cd "$MERGE" && find usr -type f -exec md5sum {} + > DEBIAN/md5sums )

# --- 5. empacota + entrega no /dist -----------------------------------------
echo "==> [5/5] gerando o .deb e o instalador"
mkdir -p "$DIST"
OUT="$DIST/mustard_${VERSION}_amd64.deb"
rm -f "$OUT"
dpkg-deb --root-owner-group --build "$MERGE" "$OUT"

# instalador + docs ao lado do .deb (o install.sh chama `apt install`).
cp "$REPO/packaging/installer/install.sh" \
   "$REPO/packaging/installer/README.txt" \
   "$REPO/packaging/installer/TUTORIAL-LINUX.md" "$DIST/"
sed -i 's/\r$//' "$DIST/install.sh"
chmod +x "$DIST/install.sh"

echo
echo "==> Pronto. Conteúdo do pacote (.deb):"
dpkg-deb -c "$OUT" | sed -n '1,40p'
echo
echo "==> control:"
dpkg-deb -f "$OUT"
echo
echo "==> Saída em $DIST:"
ls -la "$DIST"
