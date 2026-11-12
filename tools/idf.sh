#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# POSIX counterpart of tools/idf.ps1 — run `idf.py` with a fully exported
# ESP-IDF environment on Linux and macOS.
#
# ESP-IDF is located in this order:
#   1. $IDF_PATH, if already exported.
#   2. .idf-env.json in the repository root (git-ignored):
#          { "idfPath": "/opt/esp/v5.4/esp-idf", "toolsPath": "/opt/esp/tools" }
#   3. The usual install locations: ~/esp/esp-idf, ~/esp/v5.*/esp-idf,
#      /opt/esp-idf, /opt/esp/v5.*/esp-idf.
#
# Usage:
#   ./tools/idf.sh build
#   ./tools/idf.sh -p /dev/ttyACM0 flash monitor
# ---------------------------------------------------------------------------
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

log()  { printf '\033[36m==> %s\033[0m\n' "$1"; }
fail() { printf '\033[31mERROR: %s\033[0m\n' "$1" >&2; }

json_value() {
    # Minimal extractor so the script does not depend on jq.
    # $1 = file, $2 = key
    [ -f "$1" ] || return 1
    sed -n "s/.*\"$2\"[[:space:]]*:[[:space:]]*\"\([^\"]*\)\".*/\1/p" "$1" | head -n1
}

idf_path="${IDF_PATH:-}"
tools_path="${IDF_TOOLS_PATH:-}"

config_file="$repo_root/.idf-env.json"
if [ -f "$config_file" ]; then
    [ -n "$idf_path" ]   || idf_path="$(json_value "$config_file" idfPath || true)"
    [ -n "$tools_path" ] || tools_path="$(json_value "$config_file" toolsPath || true)"
fi

if [ -z "$idf_path" ] || [ ! -f "$idf_path/export.sh" ]; then
    for candidate in \
        "$HOME/esp/esp-idf" \
        "$HOME"/esp/v5.*/esp-idf \
        /opt/esp-idf \
        /opt/esp/v5.*/esp-idf \
        /opt/esp-idf/v5.*
    do
        if [ -f "$candidate/export.sh" ]; then
            idf_path="$candidate"
            break
        fi
    done
fi

if [ -z "$idf_path" ] || [ ! -f "$idf_path/export.sh" ]; then
    fail "ESP-IDF was not found."
    cat >&2 <<'EOF'

Install it, or point this project at an existing copy:

  # 1. install (once)
  mkdir -p ~/esp && cd ~/esp
  git clone -b v5.4 --recursive https://github.com/espressif/esp-idf.git
  cd esp-idf && ./install.sh esp32s3

  # 2. tell this project where it is
  echo '{ "idfPath": "'"$HOME"'/esp/esp-idf" }' > .idf-env.json
EOF
    exit 3
fi

if [ -n "$tools_path" ]; then
    export IDF_TOOLS_PATH="$tools_path"
fi
export IDF_PATH="$idf_path"

log "ESP-IDF: $idf_path"

# shellcheck disable=SC1090
. "$idf_path/export.sh" >/dev/null

if ! command -v idf.py >/dev/null 2>&1; then
    fail "export.sh completed but idf.py is still not on PATH."
    echo "Run the installer for this target first: $idf_path/install.sh esp32s3" >&2
    exit 4
fi

if [ "$#" -eq 0 ]; then
    cat <<'EOF'
No arguments given. Common commands:
  build            build the application
  -p /dev/ttyACM0 flash    flash over the given port
  monitor          open the serial monitor
  flash monitor    flash, then monitor
EOF
    exit 0
fi

cd "$repo_root"
exec idf.py "$@"
