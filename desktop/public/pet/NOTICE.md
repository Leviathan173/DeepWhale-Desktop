# dsh-pet asset notice

This project includes the desktop-pet animation clips from:

- Repository: https://github.com/PC2005-cloud/dsh-pet
- Commit: `29604935b1323ef110b799d04407973c7a976d14` (2026-10)
- License: MIT, Copyright (c) 2026 PC2005-cloud (see LICENSE in this directory)
- Character design credit: ZipZipPipe; character upstream: https://github.com/vlln/whale-girl

All 106 clips are the original VP9-alpha webm files (640×360), renamed from their
Chinese file names to the pinyin names used by the upstream preview script
(`scripts/encode_preview_gifs.py`, `PINYIN` table). No re-encoding was done.

`manifest.json` in this directory is ours: it maps our widget states
(`states`) and the random ambient action pool (`modes`) onto those clips. The
`label` field keeps the original Chinese animation name.

These clips are used as the default DSH appearance. The original project's license
and attribution are retained here; this notice does not change the license of the
surrounding project.

## Rebuilding at a higher resolution

The shipped clips are 640×360. Higher-resolution sources exist upstream:

- `assets-videos` release: `video/*.mp4`, 1280×720 H.264 with a green background
  and a `豆包AI生成` watermark in the bottom-right corner.
- `assets-mov` release: 640×360 HEVC-alpha (same resolution, for Safari) — no gain.

Upstream's own pipeline (in the dsh-pet repo under `scripts/`) is
`watermark_step01.py → chroma_step02.py → normalize_step03.py → encode_*.py`:
watermark fill, HSV-based chroma key at 1280×720, then normalization onto a
2160×1215 canvas (standing height 900px, feet at y=1115, centered at x=1080),
then the 640×360 playback encode.

Pitfalls found while rebuilding (do not repeat):

- Use `colorkey` (RGB distance) for the chrome key on these sources.
  `chromakey` (YUV distance) keys the character itself and leaves only edge
  fringes — verified on `待机呼吸休闲`.
- The watermark box is `x=1055:y=625:w=190:h=80`; `delogo` with that box is
  enough (the character never enters that corner).
- A straight ffmpeg re-encode comes out darker/greyer than the shipped clips
  (upstream converts to BT.601 explicitly in numpy). To match the current look
  exactly, run upstream's `chroma_step02.py` + `normalize_step03.py` and only
  change the final encode size/CRF.
- Re-encoding all 106 clips at 960×540 CRF42 yields ~1.07MB per clip
  (~110MB total, vs 51.8MB now), so it is not shipped by default.