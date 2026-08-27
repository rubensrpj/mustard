# ============================================================================
# Ambiente de build do pacote Linux do Mustard (CLI + dashboard num único .deb).
#
# Base Ubuntu 22.04 -> glibc 2.35: o artefato roda em Ubuntu 22.04+. The floor
# used to be webkit2gtk-4.1 (absent on 20.04) and is now just the glibc this
# image links against — the dashboard is a `tiny_http` server with no system
# dependency at all, so the WebKitGTK/GTK/librsvg/appindicator build packages
# left with the desktop shell. Os binários do CLI e o servidor do dashboard são compilados
# aqui dentro e empacotados num só .deb pelo packaging/linux/build-deb.sh.
#
# A imagem é cacheada por camadas do Docker; o build-packages.ps1 faz `docker
# build` (rápido após a 1ª vez) e depois `docker run` montando o repo + o dist.
# ============================================================================
FROM ubuntu:22.04

ENV DEBIAN_FRONTEND=noninteractive

# Pré-requisitos de Rust/Node + as ferramentas de empacotamento .deb (dpkg-dev,
# fakeroot) e cópia (rsync).
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential pkg-config file curl wget ca-certificates xz-utils git \
      rsync dpkg-dev fakeroot \
      libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Node.js 20 (NodeSource) + pnpm fixado na versão do dashboard (packageManager).
RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && npm install -g pnpm@10.18.1 \
    && rm -rf /var/lib/apt/lists/*

# rustup only — NO toolchain baked in. The repo is mounted at /work and pins its
# compiler in `rust-toolchain.toml`; rustup honours that file over any default,
# so the first `cargo` inside /work installs exactly the pinned version. Baking
# `stable` here used to look like a floor ("precisa ser >= 1.85") but it was a
# second version request that the mounted file overrode anyway — an unused
# toolchain in the image layer, and a number free to disagree with the one every
# other build uses.
#
# rustup home is /opt/rustup, cargo home /opt/cargo; the registry/target caches
# come in as volumes on `docker run`, outside this file.
ENV RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/opt/cargo \
    PATH=/opt/cargo/bin:/usr/local/bin:/usr/bin:/bin
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --profile minimal --default-toolchain none \
    && rustup --version

# THE PINNED TOOLCHAIN IS BAKED INTO THE IMAGE, and this line is why.
#
# Installing rustup with `--default-toolchain none` removes the version that
# used to disagree with the repository's — but it also leaves the image with no
# compiler at all. `packaging/build-packages.ps1` runs `docker run --rm` and
# mounts volumes only for /opt/cargo/registry, /opt/cargo/git and the target
# dirs: RUSTUP_HOME is neither baked nor mounted, so every single package build
# would download and install the whole toolchain before compiling a line. That is
# a speed regression in the exact dimension this unit exists to remove, and it
# was found in review rather than by anyone timing a build.
#
# Copying the pin and installing it here gets both halves: one version, named by
# the repository, resolved once at image-build time and cached in the layer.
# `--locked` is not a thing for toolchains; the guarantee is that the file below
# is the same file the repository carries, so a bump to it rebuilds this layer.
COPY rust-toolchain.toml /tmp/rust-toolchain.toml
RUN cd /tmp && rustup toolchain install && rustup show active-toolchain && rm -f /tmp/rust-toolchain.toml

WORKDIR /work
