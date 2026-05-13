# Branding And Icons

App icon, sidebar wordmark, and README banner share one source: the 1024x1024
master at `docs/assets/lantor-icon-master-1024.png`.

Regenerate the Tauri icon set under `src-tauri/icons/` from that master with:

```bash
npx tauri icon docs/assets/lantor-icon-master-1024.png
```

The generated set includes `icns`, `ico`, multi-size PNGs, iOS assets, Android
assets, and Windows Store assets.

The web app picks up `public/lantor-icon.png` at `/lantor-icon.png` for the
sidebar pill. The README references `docs/assets/lantor-banner.png` at the top.
