#!/usr/bin/env bash
# ============================================================================
# build-deb.sh — roda DENTRO do container (packaging/linux/Dockerfile).
#
# Compila os 5 binários do CLI (scan, mustard-rt, mustard-mcp, mustard, rtk) e o
# servidor do dashboard (mustard-dashboard), constrói os assets do React, e
# empacota TUDO num único pacote Debian:
#
#   dist/mustard_<versao>_amd64.deb
#
# Layout instalado pelo .deb:
#   /usr/lib/mustard/bin/        os 5 binários do CLI + o mustard-dashboard
#   /usr/lib/mustard/bin/dist/   os assets do React que o servidor serve
#   /usr/lib/mustard/templates/  a carga do `mustard init`
#   /usr/share/applications/…    atalho .desktop que INICIA O SERVIDOR
# E o postinst cria os symlinks em /usr/bin para tudo entrar no PATH.
#
# The .deb used to be built by EXTRACTING the one the desktop-app bundler
# produced and injecting the CLI into it — that is where the .desktop entry, the
# icons and the webkit2gtk/gtk `Depends` came from. That bundler is gone: the
# tree below is written from scratch, and the dependency list shrank to the C
# runtime every Rust binary already needs.
#
# Por que /usr/lib/mustard/bin + symlinks (e não /usr/bin direto): o mustard e o
# dashboard resolvem a pasta templates como `<dir-do-exe>/../templates`. Com os
# reais binários juntos em /usr/lib/mustard/bin, `../templates` aponta para
# /usr/lib/mustard/templates para TODOS — inclusive o dashboard, que instala
# projetos chamando mustard_cli::init nativamente. current_exe() resolve o
# symlink para o caminho real, então a resolução funciona via /usr/bin também.
#
# The same invariant is what puts dist/ INSIDE bin/: the server resolves its
# assets as `<dir of the exe>/dist`, and on Linux `current_exe()` reads
# /proc/self/exe, which is already symlink-free — so reaching the binary through
# /usr/bin still lands on /usr/lib/mustard/bin/dist.
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
PNPM_STORE=/tmp/pnpm-store

CLI_BINS="scan mustard-rt mustard-mcp mustard"

echo "==> [1/6] copiando o repo para área de build isolada ($BUILD)"
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
echo "==> [2/6] cargo build --release (CLI + servidor do dashboard)"
( cd "$BUILD" && CARGO_TARGET_DIR="$CARGO_TARGET" MUSTARD_RELEASE_VERSION="$VERSION" \
    cargo build --release --locked \
      --bin scan --bin mustard-rt --bin mustard-mcp --bin mustard --bin mustard-dashboard )

# --- 3. rtk (binário pré-compilado oficial) ---------------------------------
echo "==> [3/6] obtendo o rtk"
RTK=""
curl -fsSL https://raw.githubusercontent.com/rtk-ai/rtk/master/install.sh | sh || true
for p in "$HOME/.local/bin/rtk" "$HOME/.cargo/bin/rtk" /opt/cargo/bin/rtk \
         /usr/local/bin/rtk /usr/bin/rtk; do
  if [ -x "$p" ]; then RTK="$p"; echo "    rtk: $p"; break; fi
done
[ -n "$RTK" ] || { echo "erro: rtk não pôde ser obtido — pacote incompleto." >&2; exit 1; }

# --- 4. assets do React -----------------------------------------------------
echo "==> [4/6] pnpm install + build do React (assets do dashboard)"
# /build é descartável — install normal (sem --frozen-lockfile) para não quebrar
# o empacotamento por um drift de lock; o store fica cacheado num volume.
( cd "$BUILD" && pnpm install --store-dir "$PNPM_STORE" )
( cd "$BUILD" && pnpm --filter mustard-dashboard build )
DIST_SRC="$BUILD/apps/dashboard/dist"
[ -f "$DIST_SRC/index.html" ] || { echo "erro: o build do React não gerou $DIST_SRC." >&2; exit 1; }

# --- 5. monta a árvore do .deb ----------------------------------------------
echo "==> [5/6] montando o .deb"
MERGE=/tmp/merge
rm -rf "$MERGE"
mkdir -p "$MERGE/DEBIAN" \
         "$MERGE/usr/lib/mustard/bin" \
         "$MERGE/usr/lib/mustard/templates" \
         "$MERGE/usr/share/applications"

# 5a. binários + rtk + assets + templates.
for b in $CLI_BINS mustard-dashboard; do
  cp "$CARGO_TARGET/release/$b" "$MERGE/usr/lib/mustard/bin/$b"
done
cp "$RTK" "$MERGE/usr/lib/mustard/bin/rtk"
chmod 0755 "$MERGE"/usr/lib/mustard/bin/*
cp -R "$DIST_SRC" "$MERGE/usr/lib/mustard/bin/dist"
cp -R "$BUILD/apps/cli/templates/." "$MERGE/usr/lib/mustard/templates/"

# 5b. atalho .desktop. It starts a SERVER: the entry runs the binary in a
#     terminal so the URL it prints is visible and Ctrl+C stops it — an app
#     window is exactly what there no longer is. The icon is a stock
#     freedesktop name because the icon set came from the old desktop-app bundle
#     and left with it; dropping a real icon in later only changes this line.
cat > "$MERGE/usr/share/applications/mustard-dashboard.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=Mustard Dashboard
Comment=Serve o Mustard Dashboard em http://127.0.0.1:7777 e abre o navegador
Exec=mustard-dashboard
Icon=utilities-system-monitor
Terminal=true
Categories=Development;
Keywords=mustard;claude;dashboard;
EOF

# 5c. control. Depends shrank with the desktop shell: what is left is the C runtime any Rust
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
Description: Mustard — harness de pipeline para Claude Code (CLI + dashboard)
 Instalação completa do Mustard: os binários de linha de comando
 (mustard, mustard-rt, mustard-mcp, scan, rtk) e o servidor do Mustard
 Dashboard, num único pacote.
EOF

# 5d. maintainer scripts: symlinks em /usr/bin (entram no PATH) + cache do menu
#     de aplicativos.
cat > "$MERGE/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
for b in mustard mustard-rt mustard-mcp scan rtk mustard-dashboard; do
  ln -sf "/usr/lib/mustard/bin/$b" "/usr/bin/$b"
done
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q /usr/share/applications || true
fi
exit 0
EOF
cat > "$MERGE/DEBIAN/prerm" <<'EOF'
#!/bin/sh
set -e
for b in mustard mustard-rt mustard-mcp scan rtk mustard-dashboard; do
  rm -f "/usr/bin/$b"
done
exit 0
EOF
cat > "$MERGE/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e
if [ "$1" = "remove" ] || [ "$1" = "purge" ]; then
  if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database -q /usr/share/applications || true
  fi
fi
exit 0
EOF
chmod 0755 "$MERGE/DEBIAN/postinst" "$MERGE/DEBIAN/prerm" "$MERGE/DEBIAN/postrm"

# 5e. md5sums.
( cd "$MERGE" && find usr -type f -exec md5sum {} + > DEBIAN/md5sums )

# --- 6. empacota + entrega no /dist -----------------------------------------
echo "==> [6/6] gerando o .deb e o instalador"
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
