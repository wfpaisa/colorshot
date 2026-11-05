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
