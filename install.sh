#!/usr/bin/env bash
# Installs (or updates) ColorShot for the current user from the latest GitHub release.
#
#   curl -fsSL https://raw.githubusercontent.com/wfpaisa/colorshot/main/install.sh | bash
#
# Options:
#   --version vX.Y.Z   install that release instead of the latest
#   --uninstall        remove ColorShot (settings in ~/.config/colorshot are kept)
#
# Run from an extracted release archive, it installs the files next to it.
set -euo pipefail

REPO="wfpaisa/colorshot"
ASSET="colorshot-x86_64-linux.tar.gz"
UUID="colorshot@wfelipe.com"
BIN_DIR="${HOME}/.local/bin"
DATA_DIR="${XDG_DATA_HOME:-${HOME}/.local/share}"

info() { printf '\033[1;34m::\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m!!\033[0m %s\n' "$*" >&2; }
die() { printf '\033[1;31mxx\033[0m %s\n' "$*" >&2; exit 1; }

# Stops the resident instance so the binary can be replaced.
stop_running() {
    if pgrep -x colorshot >/dev/null 2>&1; then
        "${BIN_DIR}/colorshot" quit >/dev/null 2>&1 || pkill -x colorshot || true
        sleep 1
    fi
}

uninstall() {
    stop_running
    if command -v gnome-extensions >/dev/null 2>&1; then
        gnome-extensions disable "${UUID}" >/dev/null 2>&1 || true
    fi
    rm -f "${BIN_DIR}/colorshot" \
        "${DATA_DIR}/applications/colorshot.desktop" \
        "${DATA_DIR}/icons/hicolor/scalable/apps/colorshot.svg"
    rm -rf "${DATA_DIR}/colorshot/icons" "${DATA_DIR}/gnome-shell/extensions/${UUID}"
    info "ColorShot removed. Settings are still in ~/.config/colorshot."
}

# Downloads the release archive (and its checksum) into $1 and extracts it there.
download() {
    local dir="$1" version="$2" base
    command -v curl >/dev/null 2>&1 || die "curl is required."
    if [[ -n "${version}" ]]; then
        base="https://github.com/${REPO}/releases/download/${version}"
    else
        base="https://github.com/${REPO}/releases/latest/download"
    fi
    info "Downloading ${version:-the latest release}…"
    curl -fsSL -o "${dir}/${ASSET}" "${base}/${ASSET}" || die "Could not download ${base}/${ASSET}"
    if curl -fsSL -o "${dir}/${ASSET}.sha256" "${base}/${ASSET}.sha256"; then
        (cd "${dir}" && sha256sum -c --quiet "${ASSET}.sha256") || die "Checksum mismatch."
    else
        warn "No checksum published; skipping verification."
    fi
    tar -xzf "${dir}/${ASSET}" -C "${dir}"
}

install_from() {
    local src="$1" fresh_extension=0
    [[ -x "${src}/colorshot" ]] || die "No colorshot binary in ${src}."

    stop_running
    install -Dm755 "${src}/colorshot" "${BIN_DIR}/colorshot"

    # Launcher and its icon; the windows' app_id is colorshot, like the .desktop file.
    install -Dm644 "${src}/colorshot.svg" "${DATA_DIR}/icons/hicolor/scalable/apps/colorshot.svg"
    sed "s|^Exec=.*|Exec=${BIN_DIR}/colorshot|" "${src}/colorshot.desktop" |
        install -Dm644 /dev/stdin "${DATA_DIR}/applications/colorshot.desktop"
    gtk-update-icon-cache -q -t "${DATA_DIR}/icons/hicolor" >/dev/null 2>&1 || true

    # Interface icons, read at runtime.
    install -Dm644 -t "${DATA_DIR}/colorshot/icons" "${src}"/icons/*.svg

    # GNOME Shell extension for Capture Window and area-only captures.
    [[ -d "${DATA_DIR}/gnome-shell/extensions/${UUID}" ]] || fresh_extension=1
    install -Dm644 -t "${DATA_DIR}/gnome-shell/extensions/${UUID}" "${src}/extension/${UUID}"/*
    if command -v gnome-extensions >/dev/null 2>&1; then
        gnome-extensions enable "${UUID}" >/dev/null 2>&1 || true
    fi

    info "Installed $("${BIN_DIR}/colorshot" --version 2>/dev/null || echo colorshot) to ${BIN_DIR}/colorshot"

    case ":${PATH}:" in
        *":${BIN_DIR}:"*) ;;
        *) warn "${BIN_DIR} is not in your PATH." ;;
    esac

    local missing=()
    command -v wl-copy >/dev/null 2>&1 || missing+=("wl-clipboard (copy on Wayland)")
    command -v tesseract >/dev/null 2>&1 || missing+=("tesseract (read text)")
    command -v zbarimg >/dev/null 2>&1 || missing+=("zbar (read QR codes)")
    command -v zenity >/dev/null 2>&1 || missing+=("zenity (Open File dialog)")
    command -v notify-send >/dev/null 2>&1 || missing+=("libnotify (notifications)")
    if ((${#missing[@]})); then
        warn "Optional tools not found:"
        printf '     - %s\n' "${missing[@]}" >&2
    fi

    if ((fresh_extension)); then
        warn "Log out and back in once so GNOME loads the ColorShot extension."
    fi
    info "Start it with: colorshot"
}

main() {
    local version="" action="install"
    while (($#)); do
        case "$1" in
            --version) version="${2:?--version needs a tag, e.g. v0.1.0}"; shift 2 ;;
            --uninstall) action="uninstall"; shift ;;
            -h | --help)
                echo "Usage: install.sh [--version vX.Y.Z] [--uninstall]"
                exit 0
                ;;
            *) die "Unknown option: $1" ;;
        esac
    done

    if [[ "${action}" == "uninstall" ]]; then
        uninstall
        return
    fi

    [[ "$(uname -s)" == "Linux" && "$(uname -m)" == "x86_64" ]] ||
        die "Release builds are only available for x86_64 Linux. Build from source instead."

    # From an extracted archive: install what is next to this script.
    local here=""
    if [[ -n "${BASH_SOURCE[0]:-}" && -f "${BASH_SOURCE[0]}" ]]; then
        here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    fi
    if [[ -z "${version}" && -n "${here}" && -x "${here}/colorshot" ]]; then
        install_from "${here}"
        return
    fi

    local tmp
    tmp="$(mktemp -d)"
    # Expanded now: the local variable is gone when the trap runs.
    # shellcheck disable=SC2064
    trap "rm -rf '${tmp}'" EXIT
    download "${tmp}" "${version}"
    install_from "${tmp}/colorshot"
}

main "$@"
