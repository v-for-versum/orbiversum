#!/usr/bin/env bash
# Склеивает прошивку Orbiversum (Rust) в один образ для записи из браузера.
#
# Берёт готовый ELF и bootloader/partition-table из target/ (после `cargo build --release`
# в rust/boards/jc3636k718c/firmware), собирает один образ с адреса 0 и обновляет
# manifest.json: version, image.path, image.size, image.sha256.
#
#   tools/webflash/merge.sh [версия]      # версия по умолчанию — из Cargo.toml
#
# Скрипт ничего не собирает: если target/ устарел, сначала `cargo build --release`.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fw="$root/rust/boards/jc3636k718c/firmware"
rel="$fw/target/xtensa-esp32s3-espidf/release"
bin="orbiversum-firmware-jc3636k718c"
out="$root/tools/webflash/firmware/orbiversum.bin"
manifest="$root/tools/webflash/manifest.json"

version="${1:-$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$fw/Cargo.toml" | head -1)}"

for f in "$rel/$bin" "$rel/bootloader.bin" "$rel/partition-table.bin"; do
  [ -f "$f" ] || { echo "нет $f — сначала cargo build --release в $fw" >&2; exit 1; }
done
command -v espflash >/dev/null || { echo "нужен espflash (cargo install espflash)" >&2; exit 1; }

mkdir -p "$(dirname "$out")"
# Режим, частота и размер — те же, что в sdkconfig.jc3636k718c (dio, 80m, 16MB):
# с другими значениями прибор после записи из браузера не стартует.
espflash save-image --chip esp32s3 --merge --skip-padding \
  --flash-mode dio --flash-freq 80mhz --flash-size 16mb \
  --bootloader "$rel/bootloader.bin" --partition-table "$rel/partition-table.bin" \
  "$rel/$bin" "$out"

size="$(wc -c <"$out" | tr -d ' ')"
sha="$(shasum -a 256 "$out" | cut -d' ' -f1)"

python3 - "$manifest" "$version" "$size" "$sha" <<'PY'
import json, sys
path, version, size, sha = sys.argv[1:]
m = json.load(open(path))
m["version"] = version
m["image"] = {"path": "firmware/orbiversum.bin", "size": int(size), "sha256": sha}
m["builds"][0]["parts"] = [{"path": "firmware/orbiversum.bin", "offset": 0}]
json.dump(m, open(path, "w"), indent=2, ensure_ascii=False)
open(path, "a").write("\n")
PY

echo "образ: $out ($size байт, sha256 $sha), версия $version"
