#!/usr/bin/env bash
# Builds ZapFast from this checkout on Arch Linux (or an Arch derivative such
# as Omarchy, EndeavourOS, Manjaro) and optionally installs it for the user.
#
# Usage: scripts/build-arch.sh [--no-deps] [--install] [--test]
#   --no-deps   skip installing system packages with pacman
#   --install   install the binary, icon and launcher entry into ~/.local
#               (through packaging/install-user.sh)
#   --test      also run the test suite (with the headless demo layout tests)
#
# The result is target/release/zapfast.
set -euo pipefail

repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo"

install_deps=1
do_install=0
do_test=0
for arg in "$@"; do
  case "$arg" in
    --no-deps) install_deps=0 ;;
    --install) do_install=1 ;;
    --test) do_test=1 ;;
    -h | --help)
      sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "Unknown option: $arg (see --help)" >&2
      exit 2
      ;;
  esac
done

if [[ ! -f /etc/arch-release ]] && ! command -v pacman >/dev/null 2>&1; then
  echo "This script is for Arch Linux (pacman not found)." >&2
  exit 1
fi

# The Rust version the project pins (rust-toolchain.toml / Cargo.toml).
toolchain=$(sed -n 's/^channel = "\(.*\)"$/\1/p' rust-toolchain.toml)
toolchain=${toolchain:-stable}

# 1. System packages: the compilers and headers the build needs (cmake for
#    libopus, clang for bindgen, ALSA and PipeWire for sound), plus the
#    libraries the window needs at run time.
if [[ $install_deps -eq 1 ]]; then
  echo "==> Installing build dependencies with pacman"
  pkgs=(
    base-devel git cmake perl clang pkgconf
    alsa-lib pipewire libpipewire
    libglvnd libxkbcommon libxkbcommon-x11 wayland
    libx11 libxcb libxcursor libxi libxrandr
    gnome-keyring libsecret xdg-desktop-portal noto-fonts-emoji
  )
  # rustup provides the pinned toolchain; skip it if Arch's rust package is
  # installed instead, since the two conflict.
  if ! pacman -Qq rust >/dev/null 2>&1; then
    pkgs+=(rustup)
  fi
  sudo pacman -S --needed --noconfirm "${pkgs[@]}"
fi

# 2. Rust toolchain. RUSTUP_TOOLCHAIN overrides rust-toolchain.toml, whose
#    cargo wrapper (mbx) is optional and usually absent on a fresh machine.
if command -v rustup >/dev/null 2>&1; then
  echo "==> Installing Rust $toolchain with rustup"
  rustup toolchain install "$toolchain" --profile minimal --component rustfmt,clippy
  export RUSTUP_TOOLCHAIN="$toolchain"
else
  need=$(sed -n 's/^rust-version = "\(.*\)"$/\1/p' Cargo.toml)
  have=$(rustc --version 2>/dev/null | cut -d' ' -f2 || true)
  if [[ -z "$have" ]]; then
    echo "No Rust compiler found. Install rustup: sudo pacman -S rustup" >&2
    exit 1
  fi
  if [[ "$(vercmp "$have" "$need")" -lt 0 ]]; then
    echo "ZapFast needs Rust $need or newer, but rustc is $have." >&2
    echo "Update with: sudo pacman -Syu, or switch to rustup." >&2
    exit 1
  fi
fi

# 3. A target/ copied from another machine (for example macOS) holds
#    artifacts for the wrong platform. Cargo would rebuild them anyway, but
#    they waste disk, so warn once.
if [[ -d target/release && ! -f target/.built-on-linux ]] && \
   find target -maxdepth 3 -name '*.dylib' -print -quit 2>/dev/null | grep -q .; then
  echo "==> Note: target/ contains macOS build output; consider 'rm -rf target' to free space."
fi

# 4. Build.
echo "==> Building ZapFast (release); the first build takes a while"
export CARGO_TARGET_DIR="$repo/target"
cargo build --locked --release
touch target/.built-on-linux

if [[ $do_test -eq 1 ]]; then
  echo "==> Running tests"
  cargo test --locked --features demo
fi

echo "==> Built: $repo/target/release/zapfast"

# 5. Optional user install: binary in ~/.local/bin, icon and launcher entry.
if [[ $do_install -eq 1 ]]; then
  echo "==> Installing for $USER"
  bash packaging/install-user.sh "$repo/target/release/zapfast"
fi
