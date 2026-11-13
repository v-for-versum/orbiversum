# webflash — flash Orbiversum from the browser

A page with an install button (esp-web-tools), a manifest and one merged firmware image.

| File | What it is |
| --- | --- |
| `merge.sh` | merges the Rust build into `firmware/orbiversum.bin` and updates the manifest |
| `firmware/orbiversum.bin` | merged image: bootloader, partition table, app; written from address 0 |
| `manifest.json` | esp-web-tools manifest: name, version, chip, image size and SHA-256 |
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
