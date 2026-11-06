# Tareas del workspace.
default:
    @just --list

dev *args:
    cargo run --offline -p colorshot -- {{args}}

run *args:
    cargo run --offline --release -p colorshot -- {{args}}

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
    install -Dm755 target/release/colorshot "$HOME/.local/bin/colorshot"
    # Logo y lanzador; el app_id de las ventanas es colorshot, como el .desktop.
    install -Dm644 assets/logo.svg "${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/scalable/apps/colorshot.svg"
    sed "s|^Exec=colorshot|Exec=$HOME/.local/bin/colorshot|" assets/colorshot.desktop | install -Dm644 /dev/stdin "${XDG_DATA_HOME:-$HOME/.local/share}/applications/colorshot.desktop"
    -gtk-update-icon-cache -q -t "${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor"
    # Extensión de GNOME para «Capture Window»; la primera vez hay que volver a iniciar sesión.
    install -Dm644 -t "${XDG_DATA_HOME:-$HOME/.local/share}/gnome-shell/extensions/colorshot@wfelipe.com" extension/colorshot@wfelipe.com/*
    -gnome-extensions enable colorshot@wfelipe.com

# Instala los iconos propios de ColorShot (assets/icons) en ~/.local/share/colorshot/icons
icons:
    install -Dm644 -t "${XDG_DATA_HOME:-$HOME/.local/share}/colorshot/icons" assets/icons/*/*.svg

# Release portable (glibc de Debian 12) en dist/: binario, lanzador, iconos, extensión e instalador.
release:
    #!/usr/bin/env bash
    set -euo pipefail
    docker run --rm --network none --user "$(id -u):$(id -g)" -v "$PWD":/src -w /src \
        -v "${CARGO_HOME:-$HOME/.cargo}/registry":/cargo/registry \
        -e HOME=/tmp -e CARGO_HOME=/cargo -e CARGO_TARGET_DIR=/src/target/docker \
        rust:1-bookworm sh -c 'RUSTUP_TOOLCHAIN=$RUST_VERSION cargo build --release --locked --offline -p colorshot'
    out=dist/colorshot
    rm -rf dist && mkdir -p "$out/icons" "$out/extension"
    install -m755 target/docker/release/colorshot install.sh "$out/"
    install -m644 assets/colorshot.desktop "$out/"
    install -m644 assets/logo.svg "$out/colorshot.svg"
    install -m644 assets/icons/*/*.svg "$out/icons/"
    cp -r extension/colorshot@wfelipe.com "$out/extension/"
    install -m644 LICENSE README.md "$out/"
    tar -C dist -czf dist/colorshot-x86_64-linux.tar.gz colorshot
    (cd dist && sha256sum colorshot-x86_64-linux.tar.gz > colorshot-x86_64-linux.tar.gz.sha256)
    objdump -T "$out/colorshot" | grep -oE 'GLIBC_[0-9.]+' | sort -Vu | tail -1
