# webflash — flash Orbiversum from the browser

A page with an install button (esp-web-tools), a manifest and one merged firmware image.

| File | What it is |
| --- | --- |
| `merge.sh` | merges the Rust build into `firmware/orbiversum.bin` (+ `orbiversum-app.bin`) and rewrites both manifests |
| `firmware/orbiversum.bin` | merged image: bootloader, partition table, app; written from address 0 |
| `firmware/orbiversum-app.bin` | app only (partition at 0x10000), for the safe Update |
| `manifest.json` | manifest of the full Install (address 0): version, image size and SHA-256 |
| `update.json` | manifest of the Update (address 0x10000, app only) |
| `index.html` | the page with the install button |

## Workflow

```sh
cd rust/boards/jc3636k718c/firmware && cargo build --release
cd ../../../.. && tools/webflash/merge.sh          # or: merge.sh 0.2.0
python3 -m http.server 8787 --directory tools/webflash   # http://localhost:8787/
```

`merge.sh` needs `espflash` and does not build anything itself. Flash mode, frequency and size
(dio, 80 MHz, 16 MB) match `sdkconfig.jc3636k718c`; other values leave the board not booting.
Web Serial works only over https or on localhost (Chrome/Edge), so the page can be published
as-is on GitHub Pages together with `manifest.json` and `firmware/`.

## KirpisDonis (anti-brick)

- **Update** writes only the app at 0x10000; the bootloader and the partition table stay intact, so an
  interrupted write cannot remove what the board boots from. **Install** writes from 0 and is for a fresh or
  non-booting board, or after a partition layout change.
- The page downloads both images and compares size and SHA-256 with the manifests before it shows the
  buttons; a mismatch blocks flashing.
- The ESP32-S3 download mode is in ROM, so a failed write is recoverable: hold BOOT while plugging USB in,
  then Install. The page has this recovery section.
