#!/usr/bin/env bash
# ============================================================================
# build-pkg.sh — instalador ÚNICO e completo do Mustard para macOS (.pkg).
#
# Roda num Mac (runner macos-* do GitHub Actions ou máquina local — NÃO há
# cross-compile confiável de macOS a partir de outro SO). Gera UM instalador
# com o CLI:
#
#   dist/Mustard-<versao>-universal.pkg
#
# There is no `.app` any more. It existed because the desktop-app bundler
# produced one, and what is installed now is a folder of executables — the
# same shape the .deb installs. Layout, mirroring the Linux package:
#
#   /usr/local/mustard/bin/        os binários do CLI
#   /usr/local/mustard/templates/  a carga do `mustard init`
#
# (/usr/local, not /usr/lib: /usr is protected by SIP on macOS.)
#
# CUIDADO com o invariante que sustenta esses symlinks: current_exe() NÃO
# resolve symlink sozinho. No macOS o _NSGetExecutablePath devolve "a path",
# não "a real path" (dyld(3)), e a doc do Rust não garante nenhum dos dois
# comportamentos. Quem resolve é o resolve_templates_dir, que CANONICALIZA o
# caminho do executável antes de procurar o templates/ ao lado dele. Sem essa
# canonicalização o `mustard init` chamado pelo nome procura em
# /usr/local/bin/templates e morre — foi exatamente o defeito de 2026-07-29.
# Este comentário afirmava o contrário e foi o que legitimou o layout: não
# reintroduzir a premissa de que o symlink se resolve sozinho.
#
# Binários UNIVERSAIS (Intel x86_64 + Apple Silicon arm64 via `lipo`): um único
# .pkg roda nos dois tipos de Mac.
# ============================================================================
set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "$0")" && pwd)
REPO=$(cd -- "$SCRIPT_DIR/../.." && pwd)
DIST="$REPO/dist"
CLI_BINS="scan mustard-rt mustard"
PREFIX=/usr/local/mustard

# The version used to come from the desktop shell's config file, which no
# longer exists. The
# release job exports MUSTARD_RELEASE_VERSION (it is also what gets compiled
# into the binaries); a local run falls back to the workspace version, which
# `bump-on-main` keeps equal to plugin.json.
VERSION="${MUSTARD_RELEASE_VERSION:-}"
if [ -z "$VERSION" ]; then
  VERSION=$(sed -n '0,/^version = "/s/^version = "\([^"]*\)".*/\1/p' "$REPO/Cargo.toml" | head -1)
fi
[ -n "$VERSION" ] || { echo "erro: não consegui resolver a versão (MUSTARD_RELEASE_VERSION ou Cargo.toml)" >&2; exit 1; }
echo "==> versão: $VERSION"

mkdir -p "$DIST"

# --- 1. binários (universal) ------------------------------------------------
echo "==> [1/4] cargo build --release (universal)"
for t in x86_64-apple-darwin aarch64-apple-darwin; do rustup target add "$t" >/dev/null; done
( cd "$REPO" && cargo build --release --locked \
    --target x86_64-apple-darwin --target aarch64-apple-darwin \
    --bin scan --bin mustard-rt --bin mustard )

# --- 2. monta a raiz do pacote ----------------------------------------------
echo "==> [2/4] montando a raiz do pacote"
PKGROOT="$DIST/_pkgroot"
rm -rf "$PKGROOT"
BIN="$PKGROOT$PREFIX/bin"
mkdir -p "$BIN" "$PKGROOT$PREFIX/templates"

for b in $CLI_BINS; do
  lipo -create -output "$BIN/$b" \
    "$REPO/target/x86_64-apple-darwin/release/$b" \
    "$REPO/target/aarch64-apple-darwin/release/$b"
done

# rtk (best-effort; o job já tenta instalá-lo antes)
RTK=""
for p in "$HOME/.local/bin/rtk" "$HOME/.cargo/bin/rtk" \
         /usr/local/bin/rtk /opt/homebrew/bin/rtk; do
  if [ -x "$p" ]; then RTK="$p"; break; fi
done
if [ -n "$RTK" ]; then cp "$RTK" "$BIN/rtk"; echo "    rtk: $RTK"; else echo "    aviso: rtk ausente"; fi
chmod 0755 "$BIN"/*

# templates um nível acima -> <exe>/../templates resolve (igual ao .deb)
cp -R "$REPO/apps/cli/templates/." "$PKGROOT$PREFIX/templates/"

# O passo do plugin, o MESMO script que o .deb embarca. Fora de bin/ de
# propósito: bin/ vira symlinks no PATH (postinstall), e este não é um comando
# que alguém digita — é uma etapa que o postinstall chama pelo caminho absoluto.
cp "$REPO/packaging/installer/plugin-step.sh" "$PKGROOT$PREFIX/plugin-step.sh"
chmod 0755 "$PKGROOT$PREFIX/plugin-step.sh"

# --- 3. script de pós-instalação (symlinks no PATH + tira a quarentena) ------
echo "==> [3/4] montando o postinstall"
SCRIPTS="$DIST/_pkgscripts"
rm -rf "$SCRIPTS"
mkdir -p "$SCRIPTS"
cat > "$SCRIPTS/postinstall" <<'EOF'
#!/bin/bash
# Roda como root após copiar a árvore para /usr/local/mustard.
set -e
PREFIX=/usr/local/mustard
mkdir -p /usr/local/bin
for b in mustard mustard-rt scan rtk; do
  if [ -e "$PREFIX/bin/$b" ]; then ln -sf "$PREFIX/bin/$b" "/usr/local/bin/$b"; fi
done
# binários não assinados/notarizados: libera o Gatekeeper para esta instalação.
xattr -dr com.apple.quarantine "$PREFIX" 2>/dev/null || true

# O passo do plugin: atualiza a CÓPIA DO PLUGIN, que é a que o Claude Code
# executa — o .pkg acabou de atualizar apenas a cópia do sistema. Este
# postinstall roda como root e o plugin mora no ~/.claude de UMA pessoa; o
# próprio script descobre quem está no console e se rebaixa para essa pessoa.
# Fail-open: o `|| true` protege o `set -e` do topo, porque o pacote já está
# instalado e um exit != 0 aqui reprovaria a instalação inteira.
if [ -x "$PREFIX/plugin-step.sh" ]; then
  sh "$PREFIX/plugin-step.sh" || true
fi
exit 0
EOF
chmod +x "$SCRIPTS/postinstall"

# --- 5. monta o .pkg --------------------------------------------------------
echo "==> [4/4] pkgbuild"
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
