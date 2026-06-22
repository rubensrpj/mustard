#!/usr/bin/env bash
# ============================================================================
# build-pkg.sh — instalador ÚNICO e completo do Mustard para macOS (.pkg).
#
# Roda num Mac (runner macos-* do GitHub Actions ou máquina local — NÃO há
# cross-compile confiável de macOS a partir de outro SO). Gera UM instalador
# que traz o Dashboard E o CLI:
#
#   dist/Mustard-<versao>-universal.pkg
#
# Espelha o que o .deb faz no Linux: os binários do CLI ficam JUNTO do binário
# do Dashboard dentro do .app, com a pasta `templates/` ao lado, de modo que a
# resolução `<dir-do-exe>/templates` funcione para TODOS (CLI e Dashboard) — o
# mesmo invariante de `mustard_cli::resolve_templates_dir`. Nenhum código Rust
# muda. O script de pós-instalação do .pkg cria os symlinks do CLI no PATH
# (/usr/local/bin); current_exe() resolve o symlink para o caminho real dentro
# do .app, então a resolução de templates continua valendo via PATH também.
#
# Binários UNIVERSAIS (Intel x86_64 + Apple Silicon arm64 via `lipo`): um único
# .pkg roda nos dois tipos de Mac.
# ============================================================================
set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "$0")" && pwd)
REPO=$(cd -- "$SCRIPT_DIR/../.." && pwd)
DIST="$REPO/dist"
APP_NAME="Mustard Dashboard.app"
CLI_BINS="scan mustard-rt mustard-mcp mustard"
TARGET_UNIVERSAL="universal-apple-darwin"

VERSION=$(grep -m1 '"version"' "$REPO/apps/dashboard/src-tauri/tauri.conf.json" \
  | sed -E 's/.*"version"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/')
[ -n "$VERSION" ] || { echo "erro: não consegui ler a versão do tauri.conf.json" >&2; exit 1; }
echo "==> versão: $VERSION"

mkdir -p "$DIST"

# --- 1. binários do CLI (universal) -----------------------------------------
echo "==> [1/5] cargo build --release (universal)"
for t in x86_64-apple-darwin aarch64-apple-darwin; do rustup target add "$t" >/dev/null; done
( cd "$REPO" && cargo build --release --locked \
    --target x86_64-apple-darwin --target aarch64-apple-darwin \
    --bin scan --bin mustard-rt --bin mustard-mcp --bin mustard )

# --- 2. Dashboard .app (universal, só o bundle .app — sem .dmg) --------------
echo "==> [2/5] tauri build (Dashboard, bundle app)"
( cd "$REPO" && pnpm install --frozen-lockfile )
( cd "$REPO" && pnpm --filter mustard-dashboard exec \
    tauri build --target "$TARGET_UNIVERSAL" --bundles app )
APP_SRC="$REPO/apps/dashboard/src-tauri/target/$TARGET_UNIVERSAL/release/bundle/macos/$APP_NAME"
[ -d "$APP_SRC" ] || { echo "erro: .app não gerado em $APP_SRC" >&2; exit 1; }

# --- 3. funde o CLI + rtk + templates dentro do .app ------------------------
echo "==> [3/5] injetando CLI + templates no .app"
PKGROOT="$DIST/_pkgroot"
rm -rf "$PKGROOT"
mkdir -p "$PKGROOT/Applications"
cp -R "$APP_SRC" "$PKGROOT/Applications/"
APP="$PKGROOT/Applications/$APP_NAME"
MACOS="$APP/Contents/MacOS"

for b in $CLI_BINS; do
  lipo -create -output "$MACOS/$b" \
    "$REPO/target/x86_64-apple-darwin/release/$b" \
    "$REPO/target/aarch64-apple-darwin/release/$b"
done

# rtk (best-effort; o job já tenta instalá-lo antes)
RTK=""
for p in "$HOME/.local/bin/rtk" "$HOME/.cargo/bin/rtk" \
         /usr/local/bin/rtk /opt/homebrew/bin/rtk; do
  if [ -x "$p" ]; then RTK="$p"; break; fi
done
if [ -n "$RTK" ]; then cp "$RTK" "$MACOS/rtk"; echo "    rtk: $RTK"; else echo "    aviso: rtk ausente"; fi

# templates ao lado dos exes -> <exe>/templates resolve (igual ao .deb)
rm -rf "$MACOS/templates"
cp -R "$REPO/apps/cli/templates" "$MACOS/templates"
chmod 0755 "$MACOS"/* 2>/dev/null || true

# --- 4. script de pós-instalação (symlinks no PATH + tira a quarentena) ------
echo "==> [4/5] montando o postinstall"
SCRIPTS="$DIST/_pkgscripts"
rm -rf "$SCRIPTS"
mkdir -p "$SCRIPTS"
cat > "$SCRIPTS/postinstall" <<'EOF'
#!/bin/bash
# Roda como root após copiar o .app para /Applications.
set -e
APP="/Applications/Mustard Dashboard.app"
MACOS="$APP/Contents/MacOS"
mkdir -p /usr/local/bin
for b in mustard mustard-rt mustard-mcp scan rtk; do
  if [ -e "$MACOS/$b" ]; then ln -sf "$MACOS/$b" "/usr/local/bin/$b"; fi
done
# binários não assinados/notarizados: libera o Gatekeeper para esta instalação.
xattr -dr com.apple.quarantine "$APP" 2>/dev/null || true
exit 0
EOF
chmod +x "$SCRIPTS/postinstall"

# --- 5. monta o .pkg --------------------------------------------------------
echo "==> [5/5] pkgbuild"
OUT="$DIST/Mustard-${VERSION}-universal.pkg"
rm -f "$OUT"
pkgbuild \
  --root "$PKGROOT" \
  --identifier com.atiz.mustard \
  --version "$VERSION" \
  --scripts "$SCRIPTS" \
  --install-location / \
  "$OUT"

echo
echo "==> Pronto: $OUT"
ls -la "$OUT"
