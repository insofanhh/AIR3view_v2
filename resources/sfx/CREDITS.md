# AIR3view reveal sounds

Replaced 2026-10-29 with the three audio files supplied by the user:

| App style / asset | Supplied file | Source interval | Duration |
| --- | --- | --- | --- |
| Whoosh / `whoosh.wav` | `paper_tear.mp3` | 0.451–1.095 s | 644 ms |
| Ting / `ting.wav` | `appear.mp3` | 0.055–0.375 s | 320 ms |
| Pop / `pop.wav` | `dragon-studio-pop-402324.mp3` | 0.173–0.375 s | 202 ms |

PCM16 mono 48 kHz for offline preview/export. Silence trimmed with a small
onset margin and natural decay; gain balanced with a -1 dBFS peak limiter;
3 ms fade-in / 20 ms fade-out prevent clicks. Project volume controls still apply.
Exact source/output SHA256 hashes and processing measurements: `sources.json`.
Reproduction: `scripts/import-reveal-sfx.py --source-dir <folder-with-the-MP3s>`.

These user-provided files replace the earlier Kenney CC0 sounds. No CC0 license
or third-party redistribution license is asserted for the replacement files.
