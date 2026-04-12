# Vortex Icons

Application icons are located in `src-tauri/icons/`:

- `32x32.png` — small icon for taskbars and file managers
- `128x128.png` — standard icon
- `128x128@2x.png` — HiDPI icon (256x256 pixels)
- `icon.icns` — macOS icon bundle
- `icon.ico` — Windows icon

## Generating icons from SVG source

Once an SVG source is available, use `tauri icon` to regenerate all sizes:

```bash
npx tauri icon path/to/vortex.svg
```

This command writes all required sizes directly to `src-tauri/icons/`.
