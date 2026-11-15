# Shortcuts for the commands from README.md ("Quick start").
#
#   make test      host unit and integration tests (no board, no esp toolchain)
#   make build     host build of the whole rust/ workspace
#   make release   firmware release build (esp toolchain)
#   make install   build in release mode, flash the board and open the serial monitor
#
# Override on the command line when needed:
#   make install PORT=/dev/cu.usbmodem1101
#   make release ESP_ENV=/path/to/export-esp.sh

ESP_ENV ?= $(HOME)/export-esp.sh
FW_DIR  := rust/boards/jc3636k718c/firmware
PORT    ?=

# The firmware needs the espup environment (LIBCLANG_PATH, PATH) in the same shell.
ESP = . "$(ESP_ENV)" &&
ifneq ($(PORT),)
FLASH_ENV = ESPFLASH_PORT=$(PORT)
endif

.PHONY: help build test release install clippy render clean

help:
	@echo "make test      host tests (rust/ workspace)"
	@echo "make build     host build (rust/ workspace)"
	@echo "make release   firmware release build"
	@echo "make install   firmware release build + flash + serial monitor (PORT=/dev/cu.usbmodem...)"
	@echo "make clippy    clippy for the host workspace and for the firmware"
	@echo "make render    render a drone to ./<drone>.wav (DRONE=eno|starlids SECS=30 HZ=110)"
	@echo "make clean     cargo clean in both workspaces"

build:
	cd rust && cargo build --workspace

test:
	cd rust && cargo test --workspace

release:
	cd $(FW_DIR) && $(ESP) cargo build --release

install:
	cd $(FW_DIR) && $(ESP) $(FLASH_ENV) cargo run --release

clippy:
	cd rust && cargo clippy --workspace --all-targets -- -D warnings
	cd $(FW_DIR) && $(ESP) cargo clippy -- -D warnings

DRONE   ?= eno
SECS    ?= 30
HZ      ?= 110
render:
	cd rust && cargo run --release -p render-wav -- --drone $(DRONE) --hz $(HZ) --seconds $(SECS) --out ../$(DRONE).wav

clean:
	cd rust && cargo clean
	cd $(FW_DIR) && cargo clean
