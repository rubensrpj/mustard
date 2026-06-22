# ============================================================================
# Ambiente de build do pacote Linux do Mustard (CLI + dashboard num único .deb).
#
# Base Ubuntu 22.04 -> glibc 2.35: o artefato roda em Ubuntu 22.04+ (o dashboard
# Tauri 2 depende de webkit2gtk-4.1, que NÃO existe no 20.04). Os 5 binários do
# CLI e o dashboard são compilados aqui dentro e empacotados num só .deb pelo
# packaging/linux/build-deb.sh.
#
# A imagem é cacheada por camadas do Docker; o build-packages.ps1 faz `docker
# build` (rápido após a 1ª vez) e depois `docker run` montando o repo + o dist.
# ============================================================================
FROM ubuntu:22.04

ENV DEBIAN_FRONTEND=noninteractive

# Dependências de build do Tauri 2 (Linux) + pré-requisitos de Rust/Node + as
# ferramentas de empacotamento .deb (dpkg-dev, fakeroot) e cópia (rsync).
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential pkg-config file curl wget ca-certificates xz-utils git \
      rsync dpkg-dev fakeroot \
      libssl-dev \
      libwebkit2gtk-4.1-dev \
      libgtk-3-dev \
      librsvg2-dev \
      libayatana-appindicator3-dev \
    && rm -rf /var/lib/apt/lists/*

# Node.js 20 (NodeSource) + pnpm fixado na versão do dashboard (packageManager).
RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && npm install -g pnpm@10.18.1 \
    && rm -rf /var/lib/apt/lists/*

# Rust via rustup (stable — precisa ser >= 1.85 para as crates edition-2024 do
# workspace). Toolchain mora em /opt/cargo|/opt/rustup (camada da imagem); o
# cache de registry/target entra por volumes montados no `docker run`, fora daqui.
ENV RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/opt/cargo \
    PATH=/opt/cargo/bin:/usr/local/bin:/usr/bin:/bin
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --profile minimal --default-toolchain stable \
    && rustc --version && cargo --version

WORKDIR /work
