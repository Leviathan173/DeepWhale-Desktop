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