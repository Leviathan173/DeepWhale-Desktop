# dsh-pet asset notice

This project includes the desktop-pet animation clips from:

- Repository: https://github.com/PC2005-cloud/dsh-pet
- Commit: `29604935b1323ef110b799d04407973c7a976d14` (2026-10)
- License: MIT, Copyright (c) 2026 PC2005-cloud (see LICENSE in this directory)
- Character design credit: ZipZipPipe; character upstream: https://github.com/vlln/whale-girl

The 106 clips here are **960×540** VP9-alpha webm files (upstream ships 640×360).
They were rebuilt from the upstream 1280×720 green-screen sources and renamed to
the pinyin names used by the upstream preview script
(`scripts/encode_preview_gifs.py`, `PINYIN` table).

`manifest.json` in this directory is ours: it maps our widget states
(`states`), the random ambient action pool (`modes`), the click responses
(`clicks`) and the balance tiers (`balance`) onto those clips. The `label` field
keeps the original Chinese animation name.

These clips are used as the default DSH appearance. The original project's license
and attribution are retained here; this notice does not change the license of the
surrounding project.

## How the 960×540 set was built

Sources: the `assets-videos` release (182MB) — `video/*.mp4`, 1280×720 H.264,
green screen, with a `豆包AI生成` watermark. The `assets-mov` release is 640×360
HEVC-alpha and offers no extra detail.

The watermark is **not** fixed in one corner: in all 106 clips it sits bottom-right
for ~0–3.8s, jumps to the top-left for ~4.0–7.8s, then returns to the bottom-right
for ~8.0–10s (with ~0.2s fades). Both corners are empty of character, so both get a
`delogo` on the decode side, each limited to its own time window:

```
delogo=x=1040:y=628:w=220:h=78:enable='between(t,0,4.0)+between(t,7.8,99)',
delogo=x=20:y=28:w=226:h=78:enable='between(t,3.9,7.9)'
```

Per clip:

1. `chroma_step02.py` from the upstream repo (HSV hue-based keying at 1280×720,
   outputs VP9-alpha at CRF30), with those two `delogo` filters added on the decode
   side to erase the watermark. The character (centred, standing) never enters
   either corner, so `delogo` is enough; no per-pixel fill step was needed.
2. Normalization and the playback encode, using the same geometry as upstream's
   `normalize_step03.py` but at 540p: standing height 400px, feet at y=495.6,
   centered at x=480 on a 960×540 transparent canvas
   (= upstream's 900 / 1115 / 1080 on 2160×1215, ×540/1215), then
   `libvpx-vp9 -pix_fmt yuva420p -crf 50 -b:v 0 -auto-alt-ref 0`.
   `despill=type=green` is applied after the scale because upstream's chroma step
   has no despill and leaves a visible green rim at 720p.

Notes for anyone rebuilding this:

- Decode the step02/step03 intermediates with `-c:v libvpx-vp9`; ffmpeg's default
  decoder drops the alpha plane and you get the raw green background instead.
- `chromakey` (YUV distance) is wrong for these sources — it keys the character
  itself and leaves only edge fringes. Use upstream's HSV keying (or `colorkey`).
- Upstream's `video/碎碎念-*.mp4` files are actually named `岁岁念-*.mp4` in the
  archive (typo). Rename them before mapping pinyin names.
- When checking the result, sample at least one frame from each watermark window
  (e.g. t=2 and t=6) — a single frame at t=4 shows a clean corner and hides the
  top-left pass entirely.
- Resulting size: ~104MB for 106 clips (vs 51.8MB for the 640×360 set), which is
  why the app is ~117MB instead of ~70MB.
