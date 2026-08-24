#!/bin/sh
set -eu

repo="${PVESTATE_REPO:-dougmaitelli/pvestate}"
install_dir="${PVESTATE_INSTALL_DIR:-${HOME}/.local/bin}"
case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) target="x86_64-unknown-linux-gnu" ;;
  Darwin-x86_64) target="x86_64-apple-darwin" ;;
  Darwin-arm64) target="aarch64-apple-darwin" ;;
  *) echo "unsupported platform: $(uname -s) $(uname -m)" >&2; exit 1 ;;
esac
base="https://github.com/${repo}/releases/latest/download"
asset="pves-${target}.tar.gz"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT INT TERM
curl --proto '=https' --tlsv1.2 -fsSL "${base}/${asset}" -o "${tmp_dir}/${asset}"
curl --proto '=https' --tlsv1.2 -fsSL "${base}/${asset}.sha256" -o "${tmp_dir}/${asset}.sha256"
(cd "$tmp_dir" && if command -v sha256sum >/dev/null 2>&1; then sha256sum -c "${asset}.sha256"; else shasum -a 256 -c "${asset}.sha256"; fi)
tar -C "$tmp_dir" -xzf "${tmp_dir}/${asset}"
mkdir -p "$install_dir"
install -m 0755 "${tmp_dir}/pves" "${install_dir}/pves"
echo "installed PVE State to ${install_dir}/pves"
