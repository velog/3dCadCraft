#!/usr/bin/env python3
"""Scripted GUI driver for visual tests (X11 + XTest, needs python3-xlib).

Usage: tools/gui_sim.py <window-id> CMD ARGS [CMD ARGS ...]
Coordinates are window-relative pixels (read them off a screenshot).

  move X Y                 move the pointer
  click X Y [BUTTON]       press+release (1 left, 2 middle, 3 right)
  dclick X Y               double-click
  drag X1 Y1 X2 Y2 [BTN]   press, move in 15 steps, release
  down X Y [BTN] / up [BTN] press / release (compose curved drags with `move`)
  key NAME [+MOD ...]      X keysym, e.g. `key z +Control_L`, `key F6`, `key s`
  type TEXT                types a string (letters, digits, / . - _ and space)
  sleep SECONDS
  shot FILE.png            screenshot of the window (ImageMagick `import`)

Example:
  WID=$(tools/gui_launch.sh CADCRAFT3D_DEMO=slot)
  tools/gui_sim.py $WID drag 606 691 600 520 shot /tmp/after-drag.png
  pkill -x 3dcadcraft
"""
import sys, time, subprocess
from Xlib import X, XK, display
from Xlib.ext import xtest

d = display.Display()
wid = int(sys.argv[1], 16)
win = d.create_resource_object('window', wid)
def origin():
    g = win.get_geometry(); t = win.translate_coords(d.screen().root, 0, 0)
    return -t.x, -t.y
ox, oy = origin()
def move(x, y):
    xtest.fake_input(d, X.MotionNotify, x=ox + int(x), y=oy + int(y)); d.sync(); time.sleep(0.08)
def press(b): xtest.fake_input(d, X.ButtonPress, b); d.sync(); time.sleep(0.06)
def release(b): xtest.fake_input(d, X.ButtonRelease, b); d.sync(); time.sleep(0.06)
def key(name, mods=()):
    codes = [d.keysym_to_keycode(XK.string_to_keysym(m)) for m in mods]
    k = d.keysym_to_keycode(XK.string_to_keysym(name))
    for c in codes: xtest.fake_input(d, X.KeyPress, c)
    xtest.fake_input(d, X.KeyPress, k); d.sync(); time.sleep(0.05)
    xtest.fake_input(d, X.KeyRelease, k)
    for c in reversed(codes): xtest.fake_input(d, X.KeyRelease, c)
    d.sync(); time.sleep(0.15)
args = sys.argv[2:]
while args:
    c = args.pop(0)
    if c == 'move': move(args.pop(0), args.pop(0))
    elif c == 'click':
        x, y = args.pop(0), args.pop(0); b = 1
        if args and args[0].isdigit() and len(args[0]) == 1: b = int(args.pop(0))
        # Hover a frame first: egui needs it for menus and buttons.
        move(x, y); time.sleep(0.15); press(b); release(b); time.sleep(0.25)
    elif c == 'dclick':
        x, y = args.pop(0), args.pop(0); move(x, y); press(1); release(1); press(1); release(1); time.sleep(0.25)
    elif c == 'drag':
        x1, y1, x2, y2 = [float(args.pop(0)) for _ in range(4)]; b = 1
        if args and args[0].isdigit() and len(args[0]) == 1: b = int(args.pop(0))
        move(x1, y1); press(b)
        for i in range(1, 16): move(x1 + (x2 - x1) * i / 15, y1 + (y2 - y1) * i / 15)
        release(b); time.sleep(0.25)
    elif c == 'down':
        x, y = args.pop(0), args.pop(0); b = 1
        if args and args[0].isdigit() and len(args[0]) == 1: b = int(args.pop(0))
        move(x, y); press(b)
    elif c == 'up':
        b = 1
        if args and args[0].isdigit() and len(args[0]) == 1: b = int(args.pop(0))
        release(b); time.sleep(0.25)
    elif c == 'key':
        name = args.pop(0); mods = []
        while args and args[0].startswith('+'): mods.append(args.pop(0)[1:])
        key(name, mods)
    elif c == 'type':
        names = {'/': 'slash', '.': 'period', '-': 'minus', ' ': 'space', ':': 'colon'}
        for ch in args.pop(0):
            if ch == '_':
                key('minus', ['Shift_L'])
            elif ch.isupper():
                key(ch.lower(), ['Shift_L'])
            else:
                key(names.get(ch, ch))
    elif c == 'sleep': time.sleep(float(args.pop(0)))
    elif c == 'shot':
        time.sleep(0.6); subprocess.run(['import', '-window', hex(wid), args.pop(0)], check=True)
    else: raise SystemExit('unknown ' + c)
