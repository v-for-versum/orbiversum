#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# POSIX counterpart of tools/upload_fs.ps1.
#
# Packs ./assets into a SPIFFS image and flashes it into the `storage`
# partition. See the PowerShell version for the full rationale.
#
# Usage:
#   ./tools/upload_fs.sh -p /dev/ttyACM0
#   ./tools/upload_fs.sh -p /dev/ttyACM0 --assets ./assets --only-build
# ---------------------------------------------------------------------------
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

port=""
assets_dir="$repo_root/assets"
only_build=0

while [ "$#" -gt 0 ]; do
    case "$1" in
        -p|--port)     port="$2";       shift 2 ;;
        --assets)      assets_dir="$2"; shift 2 ;;
        --only-build)  only_build=1;    shift ;;
        -h|--help)
            sed -n '2,11p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
            exit 0 ;;
        *) echo "Unknown argument: $1" >&2; exit 1 ;;
    esac
done

log()  { printf '\033[36m==> %s\033[0m\n' "$1"; }
fail() { printf '\033[31mERROR: %s\033[0m\n' "$1" >&2; }

if [ ! -d "$assets_dir" ]; then
    fail "Assets folder not found: $assets_dir"
    exit 2
fi
assets_dir="$(cd "$assets_dir" && pwd)"

if [ "$only_build" -eq 0 ] && [ -z "$port" ]; then
    fail "No serial port given. Use -p /dev/ttyACM0, or --only-build to skip flashing."
    exit 2
fi

# Locate ESP-IDF the same way tools/idf.sh does.
idf_path="${IDF_PATH:-}"
if [ -f "$repo_root/.idf-env.json" ]; then
    [ -n "$idf_path" ] || idf_path="$(sed -n 's/.*"idfPath"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' \
        "$repo_root/.idf-env.json" | head -n1)"
fi
if [ -z "$idf_path" ] || [ ! -f "$idf_path/export.sh" ]; then
    for candidate in "$HOME/esp/esp-idf" "$HOME"/esp/v5.*/esp-idf /opt/esp-idf /opt/esp/v5.*/esp-idf; do
        if [ -f "$candidate/export.sh" ]; then idf_path="$candidate"; break; fi
    done
fi
if [ -z "$idf_path" ] || [ ! -f "$idf_path/export.sh" ]; then
    fail "ESP-IDF was not found. See tools/idf.sh for setup instructions."
    exit 3
fi

export IDF_PATH="$idf_path"
# shellcheck disable=SC1090
. "$idf_path/export.sh" >/dev/null

# --- Resolve the storage partition ------------------------------------------
partition_csv="$repo_root/partitions.csv"
gen_part="$idf_path/components/partition_table/gen_esp32part.py"
resolved_csv="$(mktemp -t guition_knob_partitions.XXXXXX.csv)"

if [ ! -f "$partition_csv" ]; then
    fail "partitions.csv not found at $partition_csv"
    exit 4
fi

log "Resolving partition offsets"
python "$gen_part" "$partition_csv" "$resolved_csv"

# gen_esp32part.py emits lines like: storage,data,spiffs,0x810000,7M,
storage_line="$(awk -F, '$1 == "storage" { print; exit }' "$resolved_csv")"
if [ -z "$storage_line" ]; then
    fail "No partition named 'storage' in $partition_csv"
    exit 6
fi

offset_hex="$(echo "$storage_line" | cut -d, -f4 | tr -d ' ')"
size_raw="$(echo "$storage_line" | cut -d, -f5 | tr -d ' ')"

# 7M / 4096K / plain bytes -> bytes
case "$size_raw" in
    *[Mm]) size_bytes=$(( ${size_raw%[Mm]} * 1024 * 1024 )) ;;
    *[Kk]) size_bytes=$(( ${size_raw%[Kk]} * 1024 )) ;;
    *)     size_bytes="$size_raw" ;;
esac

log "storage partition: offset $offset_hex, size $size_bytes bytes"

# --- Build the SPIFFS image -------------------------------------------------
spiffsgen="$idf_path/components/spiffs/spiffsgen.py"
mkdir -p "$repo_root/build"
image="$repo_root/build/storage.bin"

log "Packing $assets_dir"
python "$spiffsgen" "$size_bytes" "$assets_dir" "$image" \
    --page-size 256 --obj-name-len 32 --meta-len 4 --use-magic

image_size="$(stat -c %s "$image" 2>/dev/null || stat -f %z "$image")"
log "Image: $image ($image_size bytes)"

if [ "$only_build" -eq 1 ]; then
    echo
    printf '\033[33mBuilt only (--only-build). Not flashing.\033[0m\n'
    exit 0
fi

# --- Flash ------------------------------------------------------------------
log "Flashing to $port"
esptool.py --chip esp32s3 --port "$port" --baud 921600 \
    write-flash "$offset_hex" "$image"

echo
printf '\033[32mDone. The files are now on the device at /assets.\033[0m\n'
