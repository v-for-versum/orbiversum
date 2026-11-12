#!/usr/bin/env python
"""Watch the board's serial console for N seconds, to stdout and to a file.

`idf.py monitor` is interactive — it wants Ctrl+] to quit — so it cannot be
driven from a script. This records a log unattended and writes it somewhere it
can be read back afterwards.

Usage:
    python tools/serial_log.py COM4 60 build/passive.log
    python tools/serial_log.py COM4 20 build/boot.log --reset

WHY THIS DOES NOT RESET THE BOARD BY DEFAULT
    COM4 on this board is the ESP32-S3's internal USB-Serial-JTAG, not a
    USB-UART bridge, and that changes what DTR and RTS mean. On a bridge they
    idle a transistor stage wired to EN and IO0; on the USB-Serial-JTAG the
    peripheral interprets the line states itself, so driving DTR asserts a hold
    that stops the chip from running and produces a completely silent console.

    Silence is the trap: it looks identical to a healthy boot that simply has
    nothing to say, and an earlier version of this script treated "no downloader
    banner" as proof the application was running. It was holding the chip in
    reset.

    So the default is to reset nothing. Run `idf.py flash` first — it ends in a
    hard reset and starts the application — and open this immediately after. For
    a full boot log that includes the ROM banner, use `--reset`, which replays
    esptool's own hard reset for this peripheral (an EN pulse with no DTR
    write).

WHY THE PORT IS REOPENED ON DROP
    Every reset of the chip makes the USB-Serial-JTAG re-enumerate, which kills
    the open handle mid-reset. A script that opens once and keeps reading goes
    silent forever after the first reset, which again looks like a hang. Reopen
    and continue, and the log shows a spontaneous reset instead of being blinded
    by it.

    The reopen deasserts DTR and RTS before `open()`; pyserial applies the stored
    state during open, so the chip sees the idle line state and is observed
    rather than reset.
"""

import sys
import time

import serial

try:
    from esp_pylib.serial_reset import PIN_HIGH, PIN_LOW, set_rts
except ImportError:  # pragma: no cover - only on a machine without ESP-IDF
    PIN_HIGH, PIN_LOW = False, True

    def set_rts(port, state):
        port.rts = state


BAUDRATE = 115200
REOPEN_DELAY_SECONDS = 0.3


def open_idle(port):
    """Open the port without disturbing the chip."""
    ser = serial.Serial()
    ser.port = port
    ser.baudrate = BAUDRATE
    ser.timeout = 0.2
    ser.dtr = False
    ser.rts = False
    ser.open()
    return ser


def usb_hard_reset(ser):
    """esptool's hard reset for the internal USB-Serial-JTAG.

    Deliberately no DTR write: on this peripheral DTR is not a transistor-driven
    IO0, and asserting it leaves the chip held. This mirrors esptool's
    `hard_reset(..., hold_delay=0.2, post_release_delay=0.2)` as used for
    USB-based targets.
    """
    set_rts(ser, PIN_LOW)       # EN low: hold in reset
    time.sleep(0.2)
    set_rts(ser, PIN_HIGH)      # EN high: run
    time.sleep(0.2)


def emit(line, sink):
    print(line, flush=True)
    if sink is not None:
        sink.write(line + "\n")
        sink.flush()


def read_for(ser, seconds, sink):
    """Read lines for `seconds`. Raises ConnectionError if the port goes away."""
    deadline = time.time() + seconds
    while time.time() < deadline:
        try:
            chunk = ser.readline()
        except Exception as exc:                # noqa: BLE001 - port went away
            raise ConnectionError(str(exc)) from exc
        if not chunk:
            continue
        emit(chunk.decode("utf-8", errors="replace").rstrip("\r\n"), sink)


def main() -> int:
    args = [a for a in sys.argv[1:] if a != "--reset"]
    want_reset = "--reset" in sys.argv

    if len(args) < 2:
        print(__doc__)
        return 2

    port = args[0]
    seconds = float(args[1])
    out_path = args[2] if len(args) > 2 else None

    sink = open(out_path, "w", encoding="utf-8", errors="replace") if out_path else None

    try:
        ser = open_idle(port)
        if want_reset:
            emit("--- replaying esptool's USB hard reset ---", sink)
            usb_hard_reset(ser)
        else:
            emit("--- listening passively (no reset from this script) ---", sink)

        deadline = time.time() + seconds
        while time.time() < deadline:
            try:
                read_for(ser, min(5.0, deadline - time.time()), sink)
            except ConnectionError as exc:
                emit(f"--- port dropped ({exc}); reopening: the chip reset ---", sink)
                try:
                    ser.close()
                except Exception:               # noqa: BLE001 - already gone
                    pass
                time.sleep(REOPEN_DELAY_SECONDS)
                try:
                    ser = open_idle(port)
                except Exception as exc:        # noqa: BLE001 - still re-enumerating
                    emit(f"--- {port} not back yet ({exc}) ---", sink)
                    time.sleep(REOPEN_DELAY_SECONDS)
    finally:
        if sink is not None:
            sink.close()

    return 0


if __name__ == "__main__":
    sys.exit(main())
