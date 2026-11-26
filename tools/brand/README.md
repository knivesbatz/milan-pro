# tools/brand

Regenerates everything in `assets/brand` (the look is described in `assets/brand/BRAND.md`).

- `render.py`: Blender script that models and renders the icon objects (transparent PNG, Cycles on the CPU).
  Blender runs headless only: `blender -b --factory-startup --python tools/brand/render.py -- reel out.png 1024`.
- `compose.py`: Python 3 + Pillow + numpy. Calls Blender for the renders it needs (headless, no window), then
  makes the XP-finished icon sizes, the drawn 16-24 px icons, the .ico, the SVG, the wordmark, the splash, the
  banners and the installer images.

```
python tools/brand/compose.py                      # writes assets/brand/*
python tools/brand/compose.py --directions out.png # the three explored directions side by side
python tools/brand/compose.py --sheet out.png      # contact sheet of the delivered files
```

Options: `--blender PATH` (or env `BLENDER`; otherwise the newest `Blender Foundation\*\blender.exe` under
`%LOCALAPPDATA%\Programs` or `%ProgramFiles%`), `--cache DIR` (renders are reused from there; default
`%TEMP%\milan-brand-renders`), `--rerender` (render again after changing `render.py`). A full run takes a few
minutes, most of it the 1024 px master render. Fonts come from `%WINDIR%\Fonts` (Trebuchet MS, Tahoma).

After changing the icon, rebuild the app: `build.rs` embeds `assets/brand/milan-pro.ico`, and the window icon,
splash and About banner are compiled in with `include_bytes!`.
