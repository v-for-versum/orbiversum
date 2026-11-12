"""Посылает MIDI-ноты в инструмент OBOD по USB.

Инструмент работает как USB-MIDI-устройство: компьютер видит его как
MIDI-выход, и этот скрипт пишет в него ноты. Никаких библиотек ставить не
надо — используется системный интерфейс Windows (winmm) через ctypes, то
есть подойдёт любой Python 3.

    python tools/midi-send.py --list              список MIDI-выходов
    python tools/midi-send.py --demo              гамма, аккорд, лестница
                                                  велосити, проверка note-off
    python tools/midi-send.py --note 60 --vel 100 --ms 500
    python tools/midi-send.py --port 3 --demo     если порт OBOD не найден сам

Скрипт ничего не знает про внутренности инструмента: он только посылает
обычные трёхбайтные сообщения note-on и note-off.
"""

import argparse
import ctypes
import sys
import time
from ctypes import wintypes

if sys.platform != "win32":
    sys.exit("Этот скрипт использует системный MIDI Windows и на других системах не работает.")


class MIDIOUTCAPS(ctypes.Structure):
    _fields_ = [
        ("wMid", wintypes.WORD),
        ("wPid", wintypes.WORD),
        ("vDriverVersion", wintypes.UINT),
        ("szPname", wintypes.WCHAR * 32),
        ("wTechnology", wintypes.WORD),
        ("wVoices", wintypes.WORD),
        ("wNotes", wintypes.WORD),
        ("wChannelMask", wintypes.WORD),
        ("dwSupport", wintypes.DWORD),
    ]


winmm = ctypes.WinDLL("winmm")
winmm.midiOutGetNumDevs.restype = wintypes.UINT
winmm.midiOutGetDevCapsW.argtypes = [wintypes.UINT, ctypes.POINTER(MIDIOUTCAPS), wintypes.UINT]
winmm.midiOutGetDevCapsW.restype = wintypes.UINT
winmm.midiOutOpen.argtypes = [ctypes.POINTER(wintypes.HANDLE), wintypes.UINT, wintypes.DWORD,
                              wintypes.DWORD, wintypes.DWORD]
winmm.midiOutOpen.restype = wintypes.UINT
winmm.midiOutShortMsg.argtypes = [wintypes.HANDLE, wintypes.DWORD]
winmm.midiOutShortMsg.restype = wintypes.UINT
winmm.midiOutClose.argtypes = [wintypes.HANDLE]
winmm.midiOutClose.restype = wintypes.UINT

MMSYSERR_NOERROR = 0


def outputs():
    """Список (номер, имя) всех MIDI-выходов системы."""
    found = []
    for index in range(winmm.midiOutGetNumDevs()):
        caps = MIDIOUTCAPS()
        if winmm.midiOutGetDevCapsW(index, ctypes.byref(caps), ctypes.sizeof(caps)) == MMSYSERR_NOERROR:
            found.append((index, caps.szPname))
    return found


def pick_port(preferred=None):
    """Выбирает порт: указанный номер, иначе первый, в имени которого есть OBOD."""
    ports = outputs()
    if not ports:
        sys.exit("В системе нет ни одного MIDI-выхода. Инструмент подключён по USB и включён?")
    if preferred is not None:
        for index, name in ports:
            if index == preferred:
                return index, name
        sys.exit(f"Порта с номером {preferred} нет. Посмотри список: --list")
    for index, name in ports:
        if "obod" in name.lower():
            return index, name
    sys.exit("Порт OBOD не найден. Посмотри список (--list) и укажи номер вручную (--port).")


class Port:
    """Открытый MIDI-выход."""

    def __init__(self, index, channel=0):
        self.handle = wintypes.HANDLE()
        self.channel = channel & 0x0F
        err = winmm.midiOutOpen(ctypes.byref(self.handle), index, 0, 0, 0)
        if err != MMSYSERR_NOERROR:
            sys.exit(f"Не удалось открыть порт {index}: код {err}")

    def close(self):
        winmm.midiOutClose(self.handle)

    def raw(self, status, data1, data2):
        message = status | (data1 << 8) | (data2 << 16)
        err = winmm.midiOutShortMsg(self.handle, message)
        if err != MMSYSERR_NOERROR:
            sys.exit(f"Порт отверг сообщение: код {err}")

    def note_on(self, note, velocity, channel=None):
        ch = self.channel if channel is None else channel & 0x0F
        self.raw(0x90 | ch, note & 0x7F, velocity & 0x7F)

    def note_off(self, note, channel=None):
        ch = self.channel if channel is None else channel & 0x0F
        self.raw(0x80 | ch, note & 0x7F, 0)

    def all_off(self, channel=None):
        ch = self.channel if channel is None else channel & 0x0F
        self.raw(0xB0 | ch, 123, 0)

    def realtime(self, status):
        """Однобайтовое сообщение реального времени: часы, старт, продолжение, стоп."""
        self.raw(status, 0, 0)


def hold(port, notes, velocity, seconds):
    """Открывает ноты, держит их и снимает."""
    for note in notes:
        port.note_on(note, velocity)
        print(f"  нота {note:3d}  велосити {velocity:3d}")
    time.sleep(seconds)
    for note in notes:
        port.note_off(note)


def demo(port):
    print("гамма до-мажор, велосити 100")
    for note in (60, 62, 64, 65, 67, 69, 71, 72):
        port.note_on(note, 100)
        time.sleep(0.18)
        port.note_off(note)
    time.sleep(0.4)

    print("аккорд, велосити 110")
    hold(port, (60, 64, 67), 110, 1.2)
    time.sleep(0.4)

    print("лестница велосити — проверка акцентов и слабых долей")
    for velocity in (20, 40, 60, 80, 100, 127):
        port.note_on(48, velocity)
        time.sleep(0.25)
        port.note_off(48)
    time.sleep(0.3)

    print("две ноты подряд — проверка правила «последняя нота»")
    port.note_on(55, 100)
    time.sleep(0.3)
    port.note_on(62, 100)
    time.sleep(0.5)
    port.note_off(55)
    time.sleep(0.4)
    port.note_off(62)
    time.sleep(0.2)

    print("все ноты сняты поимённо: control change прибор не обрабатывает")


def send_clocks(port, bpm, seconds):
    """Часы с заданным темпом: 24 тика на четверть ноты.

    Точность Windows-таймера около 15 мс, поэтому до цели досиживаем
    коротким сном и добираем остаток вращением: иначе на 120 BPM тик
    в 20.8 мс гулял бы сильнее, чем длится сам.
    """
    period = 60.0 / (bpm * 24.0)
    print(f"часы: {bpm:g} BPM, тик каждые {period * 1000:.1f} мс")
    tick = time.perf_counter()
    end = tick + seconds
    while tick < end:
        port.realtime(0xF8)
        tick += period
        delay = tick - time.perf_counter()
        if delay > 0.002:
            time.sleep(delay - 0.001)
        while time.perf_counter() < tick:
            pass


def transport(port, bpm, seconds):
    """Старт, часы и стоп — весь транспорт одной командой."""
    port.realtime(0xFA)
    print("старт")
    send_clocks(port, bpm, seconds)
    port.realtime(0xFC)
    print("стоп")


def main():
    parser = argparse.ArgumentParser(description="Посылает MIDI-ноты в инструмент OBOD.")
    parser.add_argument("--list", action="store_true", help="показать MIDI-выходы и выйти")
    parser.add_argument("--port", type=int, default=None, help="номер порта (см. --list)")
    parser.add_argument("--channel", type=int, default=1,
                        help="канал 1..16: 1 — лиды, 2 — перкуссия (по умолчанию 1)")
    parser.add_argument("--chord", default=None,
                        help="несколько нот через запятую, держать одновременно")
    parser.add_argument("--demo", action="store_true", help="проиграть проверочную последовательность")
    parser.add_argument("--note", type=int, default=None, help="послать одну ноту")
    parser.add_argument("--vel", type=int, default=100, help="велосити одной ноты, 0..127")
    parser.add_argument("--ms", type=int, default=500, help="сколько держать одну ноту, мс")
    parser.add_argument("--clock", type=float, default=None,
                        help="старт, часы с этим темпом и стоп (длительность задаёт --ms)")
    parser.add_argument("--clocks", type=float, default=None,
                        help="только часы с этим темпом, без старта и стопа")
    parser.add_argument("--start", action="store_true", help="послать только старт, 0xFA")
    parser.add_argument("--continue", dest="cont", action="store_true",
                        help="послать только продолжение, 0xFB")
    parser.add_argument("--stop", action="store_true", help="послать только стоп, 0xFC")
    args = parser.parse_args()

    if args.list:
        rows = outputs()
        if not rows:
            print("MIDI-выходов нет.")
        for index, name in rows:
            print(f"{index}: {name}")
        return

    if not 1 <= args.channel <= 16:
        sys.exit("Канал должен быть от 1 до 16.")

    index, name = pick_port(args.port)
    print(f"порт {index}: {name}, канал {args.channel}")
    port = Port(index, args.channel - 1)
    try:
        if args.clock is not None:
            transport(port, args.clock, args.ms / 1000.0)
        elif args.clocks is not None:
            send_clocks(port, args.clocks, args.ms / 1000.0)
        elif args.start:
            port.realtime(0xFA)
            print("старт")
        elif args.cont:
            port.realtime(0xFB)
            print("продолжение")
        elif args.stop:
            port.realtime(0xFC)
            print("стоп")
        elif args.demo:
            demo(port)
        elif args.chord:
            notes = [int(n) for n in args.chord.replace(" ", "").split(",") if n]
            hold(port, notes, args.vel, args.ms / 1000.0)
        elif args.note is not None:
            hold(port, (args.note,), args.vel, args.ms / 1000.0)
        else:
            parser.print_help()
    finally:
        port.close()


if __name__ == "__main__":
    main()
