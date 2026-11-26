# Memory and performance

## Where the memory goes

| Item | Cost | Source |
| --- | --- | --- |
| One terminal cell | 24 bytes | `alacritty_terminal` 0.26 `term/cell.rs:303` |
| One scrollback row | the cells it uses + 1 (vendored `alacritty_terminal`, #89); upstream allocates columns × 24 B even when empty | `vendor/alacritty_terminal/TERN-PATCH.md` |
| Scrollback growth | lazy, in chunks of ≥1,000 rows; freed when shrunk >1,000 below capacity | `grid/storage.rs:111,133` |
| Full scrollback, 200 cols | 10,000 lines of ~25 characters: 46.3 MiB upstream, 7.8 MiB trimmed. Full-width lines still cost columns × 24 B | counting allocator, release build, 2026-10-14, #89 |
| Scrolling cost of trimming | 300k lines fed: 135 ms upstream, 273 ms trimmed (+0.5 µs a line); 20 resizes of a full history: 55 ms, 3 ms | same run |

## SSH session buffers (tern-ssh)

| Buffer | Bound | Source |
| --- | --- | --- |
| russh per-channel queue | 16 messages (russh default 100) | `session.rs` `CHANNEL_BUFFER` |
| Chunk being coalesced | < 64 KiB + one packet | `session.rs` `MAX_CHUNK` |
| Event queue to the UI | 8 chunks | `session.rs` `EVENT_QUEUE` |
| Input queue from the UI | 64 commands, full → `InputError::Busy` | `lib.rs` `COMMAND_QUEUE` |

When the UI falls behind, tern stops reading the channel, russh's queue fills, and russh stops
reading the socket, so the server slows down instead of tern's memory growing.

So the default scrollback is **5,000 lines**, configurable. A fresh tab pays only for what it has printed.

## Paint

| Item | Cost | Source |
| --- | --- | --- |
| Viewport snapshot (`Terminal::lines`), 200×50 | 22 µs/frame, 240 KB transient (freed each frame) | release-build timing, 2026-10-05, #3 |

That is 0.13% of a 60 fps frame, with no steady-state memory, so it stays (#3 closed wontfix).

## Budget

Idle memory with one connected tab, release build: RSS from `scripts/measure-rss.sh` (30 s
window, after connecting) and `footprint -p` (phys_footprint, what Activity Monitor shows; RSS
also counts shared framework pages).
The first measurement sets the baseline; every release records a new row and may not regress >10%.

| Date | Commit | Scenario | RSS |
| --- | --- | --- | --- |
| 2026-10-05 | tern-ssh (uncommitted → first commit) | `examples/connect` release, `seq 1 2000000` (16.9 MB) from strong-b, fast reader | 6 MB peak |
| 2026-10-05 | same | same, reader stalled 6 s (backpressure) | 6 MB peak |
| 2026-10-05 | tern-term | `local_demo` release, one idle local shell, 900×560 window (GPUI + fonts + Metal, no SSH) | 82 MB |
| 2026-10-05 | tern (window shell, #10) | `tern` release, empty main window 1320×880, no session | 66 MB |
| — | — | app with one idle tab | not yet measured |
| 2026-10-05 | 2fbea6e | no tab | 84.4 MB RSS · 44 MB footprint |
| 2026-10-05 | 2fbea6e | one tab, connected to a LAN host, idle shell | 86.6 MB RSS (peak 91.7) · 49 MB footprint |

Baseline, set by the second row: **49 MB footprint / 87 MB RSS**. Measured on an M4 Pro,
macOS 26.6.2, with the screen locked, so no frames were being composited. A connected tab
costs about 5 MB over an empty window. phys_footprint_peak reached 153 MB during start-up
and connection; that transient is not explained yet (likely shader and font atlas set-up).

## 2026-10-06 — 0.1.0 release build, wallpaper on

| Build | Scene | Result |
| --- | --- | --- |
| 0.1.0 release (`/Applications/tern.app`) | window open, Great Wave wallpaper (full window + blurred backdrop), no session, idle 12 s | 83 MB phys_footprint · 127 MB RSS · 186 MB footprint peak |

The start-up peak (F7) is the wallpaper: the picture is decoded at full size, then scaled,
blurred and cached (`wallpaper.rs` `prepare`), and the decode buffers are freed after.

## 2026-10-06 — against other terminal and SSH apps

Method: macOS `footprint -p <pid>`, `phys_footprint` summed over every process whose
executable lives in the app's bundle (Electron apps run several). Apps not already running
were launched hidden (`open -g -j`), left 20 s, measured and quit. iTerm2 and Ghostty were
already running with the owner's sessions and were measured as they were, so they are upper
bounds rather than idle figures.

| App | phys_footprint | Processes | State |
| --- | --- | --- | --- |
| tern 0.1.0 | 83 MB | 1 | fresh, idle, wallpaper on (107 MB with the owner's tabs open) |
| Terminal.app | 30 MB | 1 | fresh |
| iTerm2 | 200 MB | 1 | running, with sessions |
| Ghostty | 339 MB | 1 | running, with sessions |
| Termius | 627 MB | 7 | fresh, idle |
| Tabby | 752 MB | 5 | fresh, idle |

### SSH clients, same protocol (supersedes the rows above for tern, Tabby, Termius)

Each app launched fresh, `phys_footprint` summed over its processes every 15 s for 120 s.
Connected = one tab logged in to `password_server --open --demo` on 127.0.0.1:2399 (the
`--open` flag lets any client in with the `none` method, so no password is typed into any app).
Release build of 0.1.0, notarised bundle.

| App, state | 15s | 30s | 45s | 60s | 75s | 90s | 105s | 120s |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| tern idle, no wallpaper | 67 | 67 | 67 | 67 | 67 | 91 | 86 | 86 |
| tern idle, wallpaper | 124 | 129 | 124 | 129 | 124 | 124 | 107 | 108 |
| tern connected, no wallpaper | 180 | 181 | 180 | 82 | 76 | 76 | 76 | 76 |
| tern connected, wallpaper | 131 | 125 | 125 | 125 | 125 | 125 | 125 | 125 |
| Tabby idle (empty profile) | 555 | 477 | 485 | 474 | 469 | 469 | 470 | 471 |
| Tabby connected | 835 | 811 | 806 | 794 | 587 | 577 | 579 | 580 |
| Termius idle | 632 | 599 | 594 | 589 | 587 | 583 | 581 | 583 |

Notes:
- The first minute is noisy for every app (start-up allocations, the wallpaper decode); the
  README quotes the 120 s value.
- Tabby connected ran with the owner's profile, so two of their old tabs were restored
  (not connected). The empty profile did not connect from the command line, so it gives
  only the idle figure.
- Termius has no connected figure: its quick-connect could not be driven by script.
- The lead is over Electron apps, not over native ones: OxideTerm, another GPUI SSH client,
  reports 81 MB idle for itself (not measured here).

## 2026-10-14 — AutoFill helper opt-out (unmeasured)

`Info.plist` sets `NSAutoFillRequiresTextContentTypeForOneTimeCodeOnMac`, the documented opt-out
from one-time-code AutoFill that AppKit attaches to every text input from macOS 26. tern never
needs it (2FA goes through the SSH keyboard-interactive prompt). #82 reports that this starts a
separate "AutoFill (tern)" helper process; that is unconfirmed here: on 2026-10-14 no helper that
could be tied to tern appeared with or without the key (the system's SafariPlatformSupport
helpers outlive the app). No memory change is claimed; record before/after numbers from
`scripts/measure-rss.sh` here if it is ever measured.

## 2026-10-14 — wallpaper cache hit decodes nothing (#33)

Before: every launch with a wallpaper read and fully decoded the source, thumbnailed it, and
opened the cached 1600 px PNG only to test that it opened. The lead measured about 35 MB of extra
peak RSS at launch for a 1920x1038 source (131-139 MB with the wallpaper, 100-103 MB without).

Now: the cache name comes from the source's file length, and the accent and backdrop colours are
stored in `<hash>.meta.json` next to the picture. A hit checks the PNG signature and IEND trailer
of both PNGs and parses the sidecar; no pixel is decoded. A miss, a truncated file or an old
sidecar takes the previous path, which also writes the sidecar. The after-figure is measured by the
lead, not recorded here.

## 2026-11-26 — Tier 1 (app) & Tier 2 (vendored GPUI) memory pass

Changes:
- **App (Tier 1):**
  - `crates/kite/src/wallpaper_decode.rs`: JPEG decode uses macOS ImageIO (`CGImageSourceCreateThumbnailAtIndex` + `kCGImageSourceThumbnailMaxPixelSize`) so wallpapers and thumbnails never allocate a full-resolution RGBA intermediate; `wallpaper_fx::render` returns `rgba` directly for `Effect::None`.
  - `crates/kite/src/shell_wallpaper.rs` & `shell_wallpaper_gallery.rs`: dedicated `RetainAllImageCache` per surface (hero, 320 px blurred backdrop, gallery thumbnails). Hero GPU/CPU image is evicted when a tab or Settings opens or crossfade ends; gallery thumbnails decode sequentially and evict on leaving Settings → Appearance.
  - `crates/kite/src/shell_look.rs`, `shell_status_bar.rs`, `shell.rs`, `settings.rs`, `shell_settings.rs`: `window.observe_window_visibility` pauses the 1 s `StatusBar` clock when hidden/occluded; added macOS `windowBlur` ("Translucent window") opt-out toggle and open directly `Opaque` when disabled or when a wallpaper is active.
  - Default `gpui_platform` features drop `runtime_shaders` (moved behind optional `kite/runtime-shaders`).
- **Vendored GPUI & release profile (Tier 2):**
  - `vendor/gpui-pre-apple`: `build.rs` compiles `shaders.metallib` ahead of time whenever Xcode's `xcrun metal`/`metallib` is present (falling back cleanly on Command Line Tools-only hosts); `CAMetalLayer.set_maximum_drawable_count(2)`; lazy + `MTLPurgeableState::Volatile` `PathIntermediateTextures`; `InstanceBufferPool` starts at 512 KiB (cap 2 pooled, marked `Volatile`); `DEFAULT_ATLAS_SIZE` 1024 → 512; `autoreleasepool` around shader/pipeline setup.
  - `vendor/gpui-pre`: CJK / Hangul / Kana / fullwidth glyphs snap to integer horizontal device pixels (`subpixel_variant.x = 0`).
  - Workspace `[profile.release]`: `opt-level = 2`, `lto = "fat"`, `codegen-units = 1`, `strip = true`, `panic = "abort"`, dependencies at `opt-level = "s"` (keeping `kite-term`, `alacritty_terminal`, `vte` at `opt-level = 2`).

Measurement (Apple Silicon macOS, Command Line Tools-only host so runtime Metal shader fallback remained active):
- `/Applications/Kite.app/Contents/MacOS/Kite` stripped binary: **26 MB → 16 MB** (-38%).
- Freshly launched `/Applications/Kite.app` (user profile, t = 6 s): **86 MB `phys_footprint`** (**88.8 MB RSS**), breakdown: `IOSurface` 36 MB (2 × 18 MB), `IOAccelerator` 17 MB, `Malloc Small` 13 MB, `IOAccelerator (graphics)` 2.6 MB.
- Release/CI hosts with full Xcode automatically emit precompiled `shaders.metallib`, eliminating the runtime `MTLCompiler` heap as well.
