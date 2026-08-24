#!/usr/bin/env bash
set -euo pipefail

# rustfmt deliberately preserves zero blank lines between top-level items.
# Keep declarations visually separated and make that convention CI-enforced.
if rg --multiline --line-number '^\}\n(?:#\[(?:derive|cfg)|pub (?:struct|enum|fn|mod)|impl |fn )' src; then
  echo "error: add a blank line between top-level Rust items" >&2
  exit 1
fi
