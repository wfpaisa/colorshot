# Tareas del workspace.
default:
    @just --list

dev *args:
    cargo run --offline -p rapidshot -- {{args}}

run *args:
    cargo run --offline --release -p rapidshot -- {{args}}

test:
    cargo test --workspace --offline

lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --offline -- -D warnings

fix:
    cargo fmt --all

build:
    cargo build --workspace --release --offline

install: build icons
    install -Dm755 target/release/rapidshot "$HOME/.local/bin/rapidshot"
    # Logo y lanzador; el app_id de las ventanas es rapidshot, como el .desktop.
    install -Dm644 assets/logo.svg "${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/scalable/apps/rapidshot.svg"
    sed "s|^Exec=.*|Exec=$HOME/.local/bin/rapidshot|" assets/rapidshot.desktop | install -Dm644 /dev/stdin "${XDG_DATA_HOME:-$HOME/.local/share}/applications/rapidshot.desktop"
    -gtk-update-icon-cache -q -t "${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor"
    # Extensión de GNOME para «Capture Window»; la primera vez hay que volver a iniciar sesión.
    install -Dm644 -t "${XDG_DATA_HOME:-$HOME/.local/share}/gnome-shell/extensions/rapidshot@wfelipe.com" extension/rapidshot@wfelipe.com/*
    -gnome-extensions enable rapidshot@wfelipe.com

# Copia los SVG de la interfaz a la carpeta de iconos; se editan sin recompilar.
icons:
    install -Dm644 -t "${XDG_DATA_HOME:-$HOME/.local/share}/rapidshot/icons" assets/icons/*/*.svg

# Release portable (glibc de Debian 12) en dist/: binario, lanzador, iconos, extensión e instalador.
release:
    #!/usr/bin/env bash
    set -euo pipefail
    docker run --rm --network none --user "$(id -u):$(id -g)" -v "$PWD":/src -w /src \
        -v "${CARGO_HOME:-$HOME/.cargo}/registry":/cargo/registry \
        -e HOME=/tmp -e CARGO_HOME=/cargo -e CARGO_TARGET_DIR=/src/target/docker \
        rust:1-bookworm sh -c 'RUSTUP_TOOLCHAIN=$RUST_VERSION cargo build --release --locked --offline -p rapidshot'
    out=dist/rapidshot
    rm -rf dist && mkdir -p "$out/icons" "$out/extension"
    install -m755 target/docker/release/rapidshot install.sh "$out/"
    install -m644 assets/rapidshot.desktop "$out/"
    install -m644 assets/logo.svg "$out/rapidshot.svg"
    install -m644 assets/icons/*/*.svg "$out/icons/"
    cp -r extension/rapidshot@wfelipe.com "$out/extension/"
    install -m644 LICENSE README.md "$out/"
    tar -C dist -czf dist/rapidshot-x86_64-linux.tar.gz rapidshot
    (cd dist && sha256sum rapidshot-x86_64-linux.tar.gz > rapidshot-x86_64-linux.tar.gz.sha256)
    objdump -T "$out/rapidshot" | grep -oE 'GLIBC_[0-9.]+' | sort -Vu | tail -1
