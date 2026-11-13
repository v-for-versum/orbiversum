#!/usr/bin/env bash
# Склеивает прошивку Orbiversum (Rust) в один образ для записи из браузера.
#
# Берёт готовый ELF и bootloader/partition-table из target/ (после `cargo build --release`
# в rust/boards/jc3636k718c/firmware), собирает один образ с адреса 0 и обновляет
# manifest.json (полная установка) и update.json (KirpisDonis: только приложение).
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

# KirpisDonis: отдельный образ только приложения (с 0x10000). «Update» пишет его и не трогает
# загрузчик и таблицу разделов, так что обрыв записи не ломает то, с чего плата стартует.
app_offset=65536
app="$root/tools/webflash/firmware/orbiversum-app.bin"
tail -c +$((app_offset + 1)) "$out" >"$app"

python3 - "$root/tools/webflash" "$version" "$app_offset" <<'PY'
import hashlib, json, os, sys
d, version, app_offset = sys.argv[1], sys.argv[2], int(sys.argv[3])

def write(manifest, image, offset, name):
    p = os.path.join(d, image)
    data = open(p, "rb").read()
    m = {
        "name": name,
        "version": version,
        "new_install_prompt_erase": False,
        "image": {"path": image, "size": len(data), "sha256": hashlib.sha256(data).hexdigest()},
        "builds": [{"chipFamily": "ESP32-S3", "parts": [{"path": image, "offset": offset}]}],
    }
    with open(os.path.join(d, manifest), "w") as f:
        json.dump(m, f, indent=2); f.write("\n")
    print(f"{manifest}: {image} {len(data)} байт, sha256 {m['image']['sha256']}")

write("manifest.json", "firmware/orbiversum.bin", 0, "Orbiversum")
write("update.json", "firmware/orbiversum-app.bin", app_offset, "Orbiversum (KirpisDonis update)")
PY
echo "версия $version"
