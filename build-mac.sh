#!/usr/bin/env bash
# Compila PoxiTerminal para Mac y genera dist/PoxiTerminal-vX.Y.Z-macos-arm64.dmg.
# Solo necesita Rust y las Command Line Tools (no hace falta Xcode completo).
set -euo pipefail
cd "$(dirname "$0")"

version="$(python3 scripts/preview_release.py --print-package-version nebula_app/Cargo.toml)"
case "$(uname -m)" in
  arm64) architecture="aarch64" ;;
  x86_64) architecture="x86_64" ;;
  *) echo "Arquitectura no soportada: $(uname -m)" >&2; exit 1 ;;
esac

echo "Compilando PoxiTerminal $version para $architecture…"
cargo build --release -p nebula --bin poxiterminal -p nebula_hook --bin poxiterminal-hook

echo "Empaquetando…"
scripts/package-macos.sh \
  --binary target/release/poxiterminal \
  --version "$version" \
  --architecture "$architecture" \
  --build-number "$(git rev-list --count HEAD)" \
  --output-directory dist \
  --force

echo "Listo: dist/"
ls -lh dist/*.dmg
