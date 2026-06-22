#!/usr/bin/env bash
# ============================================================================
# build-deb.sh — roda DENTRO do container (packaging/linux/Dockerfile).
#
# Compila os 5 binários do CLI (scan, mustard-rt, mustard-mcp, mustard, rtk) e o
# Mustard Dashboard (app Tauri 2), e funde TUDO num único pacote Debian:
#
#   dist/mustard_<versao>_amd64.deb
#
# Layout instalado pelo .deb:
#   /usr/lib/mustard/bin/        os 5 binários do CLI + o mustard-dashboard
#   /usr/lib/mustard/templates/  a carga do `mustard init`
#   /usr/share/applications/…    atalho .desktop (vem do bundle Tauri)
#   /usr/share/icons/…           ícones (vêm do bundle Tauri)
# E o postinst cria os symlinks em /usr/bin para tudo entrar no PATH.
#
# Por que /usr/lib/mustard/bin + symlinks (e não /usr/bin direto): o mustard e o
# dashboard resolvem a pasta templates como `<dir-do-exe>/../templates`. Com os
# reais binários juntos em /usr/lib/mustard/bin, `../templates` aponta para
# /usr/lib/mustard/templates para TODOS — inclusive o dashboard, que instala
# projetos chamando mustard_cli::init nativamente. current_exe() resolve o
# symlink para o caminho real, então a resolução funciona via /usr/bin também.
#
# Montagens esperadas (feitas pelo build-packages.ps1):
#   /work   -> repo (somente leitura efetiva; copiamos para /build)
#   /dist   -> saída (recebe o .deb + instalador + tutorial)
# ============================================================================
set -euo pipefail

REPO=/work
BUILD=/build
DIST=/dist
CLI_TARGET=/tmp/cli-target
DASH_TARGET=/tmp/dash-target
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

VERSION=$(grep -m1 '"version"' "$BUILD/apps/dashboard/src-tauri/tauri.conf.json" \
  | sed -E 's/.*"version"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/')
[ -n "$VERSION" ] || { echo "erro: não consegui ler a versão do tauri.conf.json" >&2; exit 1; }
echo "    versão: $VERSION"

# --- 2. binários do CLI (workspace) -----------------------------------------
echo "==> [2/6] cargo build --release (binários do CLI)"
( cd "$BUILD" && CARGO_TARGET_DIR="$CLI_TARGET" \
    cargo build --release --locked \
      --bin scan --bin mustard-rt --bin mustard-mcp --bin mustard )

# --- 3. rtk (binário pré-compilado oficial) ---------------------------------
echo "==> [3/6] obtendo o rtk"
RTK=""
curl -fsSL https://raw.githubusercontent.com/rtk-ai/rtk/master/install.sh | sh || true
for p in "$HOME/.local/bin/rtk" "$HOME/.cargo/bin/rtk" /opt/cargo/bin/rtk \
         /usr/local/bin/rtk /usr/bin/rtk; do
  if [ -x "$p" ]; then RTK="$p"; echo "    rtk: $p"; break; fi
done
[ -n "$RTK" ] || { echo "erro: rtk não pôde ser obtido — pacote incompleto." >&2; exit 1; }

# --- 4. dashboard (Tauri -> .deb) -------------------------------------------
echo "==> [4/6] pnpm install + tauri build (dashboard, só bundle .deb)"
# /build é descartável — install normal (sem --frozen-lockfile) para não quebrar
# o empacotamento por um drift de lock; o store fica cacheado num volume.
( cd "$BUILD" && pnpm install --store-dir "$PNPM_STORE" )
( cd "$BUILD" && CARGO_TARGET_DIR="$DASH_TARGET" \
    pnpm --filter mustard-dashboard exec tauri build --bundles deb )

TAURI_DEB=$(ls "$DASH_TARGET"/release/bundle/deb/*.deb 2>/dev/null | head -1 || true)
[ -n "$TAURI_DEB" ] || { echo "erro: o tauri build não gerou um .deb." >&2; exit 1; }
echo "    .deb do dashboard: $TAURI_DEB"

# --- 5. fusão: dashboard + CLI + templates num só .deb ----------------------
echo "==> [5/6] montando o .deb unificado"
MERGE=/tmp/merge
rm -rf "$MERGE"
dpkg-deb -R "$TAURI_DEB" "$MERGE"        # extrai data + DEBIAN/ do bundle Tauri

# 5a. move o binário do dashboard para o diretório privado e dá nome estável.
DASH_BIN_NAME=$(ls "$MERGE/usr/bin/" | head -1)
[ -n "$DASH_BIN_NAME" ] || { echo "erro: binário do dashboard não achado no bundle." >&2; exit 1; }
mkdir -p "$MERGE/usr/lib/mustard/bin" "$MERGE/usr/lib/mustard/templates"
mv "$MERGE/usr/bin/$DASH_BIN_NAME" "$MERGE/usr/lib/mustard/bin/mustard-dashboard"
rmdir "$MERGE/usr/bin" 2>/dev/null || true

# 5b. atalho .desktop: aponta o Exec para o comando que o postinst cria no PATH.
for d in "$MERGE"/usr/share/applications/*.desktop; do
  [ -e "$d" ] || continue
  sed -i -E 's|^Exec=.*|Exec=mustard-dashboard %U|' "$d"
done

# 5c. injeta os binários do CLI + rtk + templates.
for b in $CLI_BINS; do
  cp "$CLI_TARGET/release/$b" "$MERGE/usr/lib/mustard/bin/$b"
done
cp "$RTK" "$MERGE/usr/lib/mustard/bin/rtk"
cp -R "$BUILD/apps/cli/templates/." "$MERGE/usr/lib/mustard/templates/"
chmod 0755 "$MERGE"/usr/lib/mustard/bin/*

# 5d. control: renomeia o pacote para `mustard`, herda os Depends do Tauri
#     (webkit2gtk-4.1, gtk-3, …) e recalcula o Installed-Size.
DEPENDS=$(awk -F': ' 'tolower($1)=="depends"{sub(/^[^:]*:[[:space:]]*/,""); print; exit}' "$MERGE/DEBIAN/control")
INSTALLED_SIZE=$(du -k -s "$MERGE/usr" | cut -f1)
cat > "$MERGE/DEBIAN/control" <<EOF
Package: mustard
Version: $VERSION
Architecture: amd64
Maintainer: Atiz <rubens@atiz.com.br>
Section: utils
Priority: optional
Installed-Size: $INSTALLED_SIZE
Depends: $DEPENDS
Description: Mustard — harness de pipeline para Claude Code (CLI + dashboard)
 Instalação completa do Mustard: os binários de linha de comando
 (mustard, mustard-rt, mustard-mcp, scan, rtk) e o Mustard Dashboard
 (aplicativo desktop Tauri), num único pacote.
EOF

# 5e. maintainer scripts: symlinks em /usr/bin (entram no PATH) + caches de
#     desktop/ícones. Regeneramos os do bundle Tauri — substituímos por estes,
#     que fazem o mesmo (refresh de cache) e mais (os symlinks).
cat > "$MERGE/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
for b in mustard mustard-rt mustard-mcp scan rtk mustard-dashboard; do
  ln -sf "/usr/lib/mustard/bin/$b" "/usr/bin/$b"
done
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q /usr/share/applications || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true
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

# 5f. md5sums (regenera para refletir os arquivos injetados).
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
