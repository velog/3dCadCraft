#!/usr/bin/env python3
"""Scripted GUI driver for visual tests on Windows (WinAPI through ctypes, no packages).

The Windows counterpart of tools/gui_launch.sh + tools/gui_sim.py, with the same commands.

  python tools/gui_win.py launch [--size WxH|none] [--profile DIR|user] [ENV=value ...] [-- PROJECT_PATH]
      starts target/debug/3dcadcraft.exe, sizes its client area (default 1400x900),
      prints the window handle; output goes to target/gui-test.log. APPDATA points to a
      fresh target/gui-profile (default layout, no Restore offer, the user's settings,
      presets and autosaves untouched); --profile DIR keeps one, `user` uses the real one
  python tools/gui_win.py [HWND] CMD ARGS [CMD ARGS ...]
      drives the window (HWND from `launch`; omitted = the one 3DCadCraft window)
  python tools/gui_win.py kill [--all]
      stops the app started by `launch` (--all: every 3dcadcraft.exe, also your own)

Coordinates are client-area pixels, the same as in the screenshots (the script is
DPI aware, so at 150 % scaling they are physical pixels, not egui points).

  move X Y                 move the pointer
  click X Y [BUTTON]       press+release (1 left, 2 middle, 3 right)
  dclick X Y               double-click
  drag X1 Y1 X2 Y2 [BTN]   press, move in 15 steps, release
  down X Y [BTN] / up [BTN] press / release (compose curved drags with `move`)
  wheel X Y CLICKS         mouse wheel (positive = away from the user)
  key NAME [+MOD ...]      e.g. `key z +Control_L`, `key F6`, `key s`, `key Return`
  type TEXT                types a string (keys of the current layout, else Unicode)
  sleep SECONDS
  shot FILE.png [X Y W H [SCALE]]
                           screenshot of the client area, optionally a crop scaled
                           SCALE times (nearest pixel) to locate small dots
  focus                    bring the window to the front
  place X Y                move the window's outer top-left corner to screen X Y
  info                     print the window's outer rectangle and state

Input goes to the front window: commands other than `shot` bring it to the front, and
the desktop must not be locked. `shot` uses PrintWindow, so a covered window still works.

Example:
  HWND=$(python tools/gui_win.py launch CADCRAFT3D_DEMO=slot)
  python tools/gui_win.py $HWND drag 606 691 600 520 shot target/after-drag.png
  python tools/gui_win.py kill
"""
import ctypes
import ctypes.wintypes as W
import os
import shutil
import struct
import subprocess
import sys
import time
import zlib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
# CARGO_TARGET_DIR: a worktree building into its own target directory.
EXE = os.path.join(os.environ.get('CARGO_TARGET_DIR') or os.path.join(ROOT, 'target'), 'debug', '3dcadcraft.exe')
LOG = os.path.join(ROOT, 'target', 'gui-test.log')
PID = os.path.join(ROOT, 'target', 'gui-test.pid')
PROFILE = os.path.join(ROOT, 'target', 'gui-profile')

user32 = ctypes.WinDLL('user32', use_last_error=True)
gdi32 = ctypes.WinDLL('gdi32')
H = ctypes.c_void_p
try:
    user32.SetProcessDpiAwarenessContext(H(-4))  # per-monitor v2: physical pixels
except AttributeError:
    pass

for name, args, res in [
    ('GetDC', [H], H), ('ReleaseDC', [H, H], ctypes.c_int),
    ('GetClientRect', [H, ctypes.POINTER(W.RECT)], W.BOOL),
    ('GetWindowRect', [H, ctypes.POINTER(W.RECT)], W.BOOL),
    ('ClientToScreen', [H, ctypes.POINTER(W.POINT)], W.BOOL),
    ('PrintWindow', [H, H, W.UINT], W.BOOL),
    ('SetForegroundWindow', [H], W.BOOL), ('GetForegroundWindow', [], H),
    ('IsIconic', [H], W.BOOL), ('IsZoomed', [H], W.BOOL), ('IsWindow', [H], W.BOOL),
    ('IsWindowVisible', [H], W.BOOL), ('ShowWindow', [H, ctypes.c_int], W.BOOL),
    ('SetWindowPos', [H, H, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, W.UINT], W.BOOL),
    ('GetWindowTextW', [H, W.LPWSTR, ctypes.c_int], ctypes.c_int),
    ('GetWindowThreadProcessId', [H, ctypes.POINTER(W.DWORD)], W.DWORD),
    ('SetCursorPos', [ctypes.c_int, ctypes.c_int], W.BOOL),
    ('VkKeyScanW', [W.WCHAR], ctypes.c_short), ('MapVirtualKeyW', [W.UINT, W.UINT], W.UINT),
]:
    f = getattr(user32, name)
    f.argtypes, f.restype = args, res
for name, args, res in [
    ('CreateCompatibleDC', [H], H), ('CreateCompatibleBitmap', [H, ctypes.c_int, ctypes.c_int], H),
    ('SelectObject', [H, H], H), ('DeleteObject', [H], W.BOOL), ('DeleteDC', [H], W.BOOL),
    ('BitBlt', [H] + [ctypes.c_int] * 4 + [H, ctypes.c_int, ctypes.c_int, W.DWORD], W.BOOL),
    ('GetDIBits', [H, H, W.UINT, W.UINT, ctypes.c_void_p, ctypes.c_void_p, W.UINT], ctypes.c_int),
]:
    f = getattr(gdi32, name)
    f.argtypes, f.restype = args, res


class MOUSEINPUT(ctypes.Structure):
    _fields_ = [('dx', W.LONG), ('dy', W.LONG), ('mouseData', W.DWORD), ('dwFlags', W.DWORD),
                ('time', W.DWORD), ('dwExtraInfo', ctypes.c_size_t)]


class KEYBDINPUT(ctypes.Structure):
    _fields_ = [('wVk', W.WORD), ('wScan', W.WORD), ('dwFlags', W.DWORD), ('time', W.DWORD),
                ('dwExtraInfo', ctypes.c_size_t)]


class HARDWAREINPUT(ctypes.Structure):
    _fields_ = [('uMsg', W.DWORD), ('wParamL', W.WORD), ('wParamH', W.WORD)]


class _INPUTU(ctypes.Union):
    _fields_ = [('mi', MOUSEINPUT), ('ki', KEYBDINPUT), ('hi', HARDWAREINPUT)]


class INPUT(ctypes.Structure):
    _fields_ = [('type', W.DWORD), ('u', _INPUTU)]


def send(*inputs):
    arr = (INPUT * len(inputs))(*inputs)
    if user32.SendInput(len(inputs), arr, ctypes.sizeof(INPUT)) != len(inputs):
        raise SystemExit('SendInput was blocked (locked desktop or an elevated window in front?)')


def mouse(flags, data=0):
    i = INPUT(type=0)
    i.u.mi = MOUSEINPUT(0, 0, data & 0xFFFFFFFF, flags, 0, 0)
    return i


KEYUP, EXTENDED, UNICODE = 0x2, 0x1, 0x4
EXTENDED_VKS = {0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x2D, 0x2E, 0xA3, 0xA5, 0x5B, 0x5C}


def key_input(vk, up=False, scan=None):
    i = INPUT(type=1)
    flags = (KEYUP if up else 0) | (EXTENDED if vk in EXTENDED_VKS else 0)
    if scan is None:
        scan = user32.MapVirtualKeyW(vk, 0)
    i.u.ki = KEYBDINPUT(vk, scan, flags, 0, 0)
    return i


# X keysym names (as in gui_sim.py) and a few short forms.
VK = {'Return': 0x0D, 'Enter': 0x0D, 'KP_Enter': 0x0D, 'Escape': 0x1B, 'Esc': 0x1B, 'Tab': 0x09,
      'space': 0x20, 'BackSpace': 0x08, 'Delete': 0x2E, 'Insert': 0x2D, 'Home': 0x24, 'End': 0x23,
      'Prior': 0x21, 'Page_Up': 0x21, 'Next': 0x22, 'Page_Down': 0x22, 'Left': 0x25, 'Up': 0x26,
      'Right': 0x27, 'Down': 0x28, 'Control_L': 0xA2, 'Control_R': 0xA3, 'Ctrl': 0xA2,
      'Shift_L': 0xA0, 'Shift_R': 0xA1, 'Shift': 0xA0, 'Alt_L': 0xA4, 'Alt_R': 0xA5, 'Alt': 0xA4,
      'Super_L': 0x5B, 'slash': 0xBF, 'period': 0xBE, 'comma': 0xBC, 'minus': 0xBD, 'equal': 0xBB,
      'plus': 0xBB, 'semicolon': 0xBA, 'apostrophe': 0xDE, 'bracketleft': 0xDB,
      'bracketright': 0xDD, 'backslash': 0xDC, 'grave': 0xC0}
VK.update({f'F{n}': 0x6F + n for n in range(1, 25)})


def vk_of(name):
    if name in VK:
        return VK[name]
    if len(name) == 1 and name.isascii() and name.isalnum():
        return ord(name.upper())
    raise SystemExit(f'unknown key {name!r}')


def key(name, mods=()):
    codes = [vk_of(m) for m in mods]
    k = vk_of(name)
    send(*[key_input(c) for c in codes], key_input(k))
    time.sleep(0.05)
    send(key_input(k, True), *[key_input(c, True) for c in reversed(codes)])
    time.sleep(0.15)


def type_char(ch):
    r = user32.VkKeyScanW(ch) if ch.isascii() else -1
    if r != -1 and (r >> 8) & 0xFF in (0, 1):
        vk, shift = r & 0xFF, (r >> 8) & 1
        key_vk = [key_input(0xA0)] if shift else []
        send(*key_vk, key_input(vk))
        send(key_input(vk, True), *([key_input(0xA0, True)] if shift else []))
    else:
        down, up = INPUT(type=1), INPUT(type=1)
        down.u.ki = KEYBDINPUT(0, ord(ch), UNICODE, 0, 0)
        up.u.ki = KEYBDINPUT(0, ord(ch), UNICODE | KEYUP, 0, 0)
        send(down, up)
    time.sleep(0.06)


def title(h):
    buf = ctypes.create_unicode_buffer(512)
    user32.GetWindowTextW(h, buf, 512)
    return buf.value


def windows(pid=None):
    found = []

    @ctypes.WINFUNCTYPE(W.BOOL, H, W.LPARAM)
    def each(h, _):
        # The title is "<file>[ *] - 3DCadCraft": match its end.
        if user32.IsWindowVisible(h) and title(h).endswith('- 3DCadCraft'):
            p = W.DWORD()
            user32.GetWindowThreadProcessId(h, ctypes.byref(p))
            if pid is None or p.value == pid:
                found.append(h)
        return True

    user32.EnumWindows(each, 0)
    return found


def client_rect(h):
    r, p = W.RECT(), W.POINT(0, 0)
    user32.GetClientRect(h, ctypes.byref(r))
    user32.ClientToScreen(h, ctypes.byref(p))
    return p.x, p.y, r.right, r.bottom


def focus(h):
    if user32.IsIconic(h):
        user32.ShowWindow(h, 9)  # SW_RESTORE
    if user32.GetForegroundWindow() != h:
        # Windows lets a process take the foreground right after it sent input: tap Alt.
        send(key_input(0xA4), key_input(0xA4, True))
        user32.SetForegroundWindow(h)
        time.sleep(0.25)


def resize(h, w, hgt):
    if user32.IsZoomed(h) or user32.IsIconic(h):
        user32.ShowWindow(h, 9)
    wr = W.RECT()
    user32.GetWindowRect(h, ctypes.byref(wr))
    _, _, cw, ch = client_rect(h)
    dw, dh = (wr.right - wr.left) - cw, (wr.bottom - wr.top) - ch
    user32.SetWindowPos(h, None, 0, 0, w + dw, hgt + dh, 0x2 | 0x4)  # NOMOVE | NOZORDER


def capture(h):
    """Client area as (width, height, BGRA rows top-down)."""
    _, _, w, hgt = client_rect(h)
    screen = user32.GetDC(None)
    mem = gdi32.CreateCompatibleDC(screen)
    bmp = gdi32.CreateCompatibleBitmap(screen, w, hgt)
    old = gdi32.SelectObject(mem, bmp)
    buf = ctypes.create_string_buffer(w * hgt * 4)
    info = struct.pack('<IiiHHIIiiII', 40, w, -hgt, 1, 32, 0, 0, 0, 0, 0, 0)
    try:
        # PW_CLIENTONLY | PW_RENDERFULLCONTENT: works for the GPU-drawn viewport, also when covered.
        ok = user32.PrintWindow(h, mem, 3)
        gdi32.GetDIBits(mem, bmp, 0, hgt, buf, info, 0)
        if not ok or not any(buf.raw[i] for i in range(0, len(buf.raw), 4097)):
            focus(h)
            x, y, _, _ = client_rect(h)
            gdi32.BitBlt(mem, 0, 0, w, hgt, screen, x, y, 0x00CC0020 | 0x40000000)
            gdi32.GetDIBits(mem, bmp, 0, hgt, buf, info, 0)
    finally:
        gdi32.SelectObject(mem, old)
        gdi32.DeleteObject(bmp)
        gdi32.DeleteDC(mem)
        user32.ReleaseDC(None, screen)
    return w, hgt, buf.raw


def write_png(path, w, hgt, bgra, crop=None, scale=1):
    x0, y0, cw, ch = crop or (0, 0, w, hgt)
    x0, y0 = max(0, min(x0, w - 1)), max(0, min(y0, hgt - 1))
    cw, ch = min(cw, w - x0), min(ch, hgt - y0)
    rows = []
    for y in range(y0, y0 + ch):
        src = bgra[(y * w + x0) * 4:(y * w + x0 + cw) * 4]
        rgb = bytearray(cw * 3)
        rgb[0::3], rgb[1::3], rgb[2::3] = src[2::4], src[1::4], src[0::4]
        if scale > 1:
            rgb = b''.join(rgb[i:i + 3] * scale for i in range(0, len(rgb), 3))
        rows.extend([b'\0' + bytes(rgb)] * scale)

    def chunk(tag, data):
        return struct.pack('>I', len(data)) + tag + data + struct.pack('>I', zlib.crc32(tag + data))

    png = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', cw * scale, ch * scale, 8, 2, 0, 0, 0))
    png += chunk(b'IDAT', zlib.compress(b''.join(rows), 6)) + chunk(b'IEND', b'')
    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    with open(path, 'wb') as f:
        f.write(png)


def launch(args):
    size, envs, rest, profile, beside = (1400, 900), {}, [], PROFILE, False
    while args:
        a = args.pop(0)
        if a == '--':
            rest = args
            break
        if a == '--size':
            s = args.pop(0)
            size = None if s == 'none' else tuple(int(v) for v in s.lower().split('x'))
        elif a == '--profile':
            profile = args.pop(0)
        elif a == '--beside':
            # Another 3DCadCraft window (the user's) may stay open: input is sent only while
            # the launched window is in front (`drive` checks before every step).
            beside = True
        else:
            k, _, v = a.partition('=')
            envs[k] = v
    if not os.path.exists(EXE):
        raise SystemExit(f'{EXE} not found: run `cargo build` first')
    if windows() and not beside:
        raise SystemExit('a 3DCadCraft window is already open: close it or `gui_win.py kill`')
    env = dict(os.environ, RUST_LOG='warn')
    if profile == PROFILE:
        shutil.rmtree(PROFILE, ignore_errors=True)
    if profile != 'user':
        # Settings, layout, presets, logs and autosaves all live under %APPDATA%\3dcadcraft.
        os.makedirs(profile, exist_ok=True)
        env['APPDATA'] = os.path.abspath(profile)
    env.update(envs)
    flags = subprocess.DETACHED_PROCESS | subprocess.CREATE_NEW_PROCESS_GROUP
    with open(LOG, 'w') as log:
        try:
            # Out of the caller's job: a tool shell that ends its job must not close the app.
            p = subprocess.Popen([EXE, *rest], cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT,
                                 creationflags=flags | subprocess.CREATE_BREAKAWAY_FROM_JOB)
        except OSError:
            p = subprocess.Popen([EXE, *rest], cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT,
                                 creationflags=flags)
    with open(PID, 'w') as f:
        f.write(str(p.pid))
    for _ in range(150):
        time.sleep(0.2)
        found = windows(p.pid)
        if found:
            break
        if p.poll() is not None:
            raise SystemExit(f'3dcadcraft exited with {p.returncode}, see {LOG}')
    else:
        raise SystemExit(f'no 3DCadCraft window after 30 s, see {LOG}')
    h = found[0]
    if size:
        resize(h, *size)
    time.sleep(2)
    print(h)


def kill(args):
    if args and args[0] == '--all':
        subprocess.run(['taskkill', '/IM', '3dcadcraft.exe', '/F'], check=False)
        return
    if not os.path.exists(PID):
        raise SystemExit('no app started by `launch`')
    with open(PID) as f:
        pid = f.read().strip()
    subprocess.run(['taskkill', '/PID', pid, '/F'], check=False)
    os.remove(PID)


def button(args):
    b = 1
    if args and args[0] in ('1', '2', '3'):
        b = int(args.pop(0))
    return {1: (0x2, 0x4), 2: (0x20, 0x40), 3: (0x8, 0x10)}[b]


def drive(h, args):
    ox, oy, _, _ = client_rect(h)

    def move(x, y):
        user32.SetCursorPos(ox + int(float(x)), oy + int(float(y)))
        send(mouse(0x1))  # a relative move of 0: the window sees WM_MOUSEMOVE
        time.sleep(0.08)

    def press(b):
        send(mouse(b[0]))
        time.sleep(0.06)

    def release(b):
        send(mouse(b[1]))
        time.sleep(0.06)

    if args and args[0] not in ('shot', 'info'):
        focus(h)
    while args:
        c = args.pop(0)
        if c not in ('shot', 'info', 'sleep') and user32.GetForegroundWindow() != h:
            focus(h)
            if user32.GetForegroundWindow() != h:
                raise SystemExit(f'window {h} is not in front: input stopped before `{c}`')
        if c == 'move':
            move(args.pop(0), args.pop(0))
        elif c == 'click':
            x, y = args.pop(0), args.pop(0)
            b = button(args)
            # Hover a frame first: egui needs it for menus and buttons.
            move(x, y)
            time.sleep(0.15)
            press(b)
            release(b)
            time.sleep(0.25)
        elif c == 'dclick':
            move(args.pop(0), args.pop(0))
            b = button([])
            press(b), release(b), press(b), release(b)
            time.sleep(0.25)
        elif c == 'drag':
            x1, y1, x2, y2 = [float(args.pop(0)) for _ in range(4)]
            b = button(args)
            move(x1, y1)
            press(b)
            for i in range(1, 16):
                move(x1 + (x2 - x1) * i / 15, y1 + (y2 - y1) * i / 15)
            release(b)
            time.sleep(0.25)
        elif c == 'down':
            x, y = args.pop(0), args.pop(0)
            b = button(args)
            move(x, y)
            press(b)
        elif c == 'up':
            release(button(args))
            time.sleep(0.25)
        elif c == 'wheel':
            move(args.pop(0), args.pop(0))
            send(mouse(0x800, int(float(args.pop(0)) * 120)))
            time.sleep(0.25)
        elif c == 'key':
            name, mods = args.pop(0), []
            while args and args[0].startswith('+'):
                mods.append(args.pop(0)[1:])
            key(name, mods)
        elif c == 'type':
            for ch in args.pop(0):
                type_char(ch)
        elif c == 'sleep':
            time.sleep(float(args.pop(0)))
        elif c == 'focus':
            focus(h)
        elif c == 'place':
            if user32.IsZoomed(h):
                user32.ShowWindow(h, 9)
            user32.SetWindowPos(h, None, int(args.pop(0)), int(args.pop(0)), 0, 0, 0x1 | 0x4)
            time.sleep(0.3)
        elif c == 'info':
            wr = W.RECT()
            user32.GetWindowRect(h, ctypes.byref(wr))
            _, _, cw, ch = client_rect(h)
            state = 'minimized' if user32.IsIconic(h) else 'maximized' if user32.IsZoomed(h) else 'normal'
            print(f'outer {wr.left},{wr.top} {wr.right - wr.left}x{wr.bottom - wr.top} '
                  f'client {cw}x{ch} {state} title={title(h)!r}')
        elif c == 'shot':
            path = args.pop(0)
            crop, scale = None, 1
            if len(args) >= 4 and all(a.lstrip('-').isdigit() for a in args[:4]):
                crop = tuple(int(args.pop(0)) for _ in range(4))
                if args and args[0].isdigit():
                    scale = int(args.pop(0))
            time.sleep(0.6)
            write_png(path, *capture(h), crop=crop, scale=scale)
        else:
            raise SystemExit('unknown ' + c)


def main():
    args = sys.argv[1:]
    if not args or args[0] in ('-h', '--help'):
        print(__doc__)
        return
    if args[0] == 'launch':
        return launch(args[1:])
    if args[0] == 'kill':
        return kill(args[1:])
    if args[0].isdigit():
        h = int(args.pop(0))
        if not user32.IsWindow(h):
            raise SystemExit(f'window {h} is gone')
    else:
        found = windows()
        if len(found) != 1:
            raise SystemExit(f'{len(found)} 3DCadCraft windows open: pass the HWND')
        h = found[0]
    drive(h, args)


if __name__ == '__main__':
    main()
