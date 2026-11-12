# Changelog

All notable changes to Keepsake Atelier. Newest first.

## Unreleased

### Added

- **Window-level desktop checks.** Linux CI boots the AppImage under Xvfb and
  openbox, waits for the Tauri window, checks that the frame is not blank,
  hovers and clicks a rail section (a 451k-pixel page change), and calls the MCP
  server's `initialize` and `tools/list` (33 tools). Windows CI installs the
  NSIS build, finds the real window through UI Automation, invokes the
  command-palette trigger, requires the palette dialog, and checks the same MCP
  methods. Both seed a config that enables MCP; the Linux check also disables
  the launch-time KEYDB download so startup is deterministic.
- **Windows burning is probed, not assumed.** `doctor` instantiates
  `IMAPI2.MsftDiscMaster2` and reports the recorder count or the COM error, and
  the Windows CLI smoke requires that line.
- **The Linux CLI tarball and Windows CLI zip are self-contained.** The Linux
  package copies the linked library closure, libaacs and libbdplus into `lib/`
  with an `$ORIGIN/lib` rpath and carries the bundled ffmpeg, ffprobe and menu
  JAR. The Windows zip now also carries ffmpeg, ffprobe and the menu JAR.
  `bundled_lib` resolves the platform-specific backend names (`.so`, `.dylib`,
  `.dll`, including a flat next-to-the-exe layout), so `doctor` and `libs`
  report the shipped backends on every platform, and the default backend is
  now `libaacs` whenever a bundled copy exists even without a system install.
  CI extracts the Linux tarball and runs it in a bare `ubuntu:24.04` container.
- **Headless UI interaction checks.** `npm run test:ui` runs Playwright against
  the `?mock=1` fixture: landmarks, the skip link and focus handoff, reading a
  disc, title selection, the command palette, the shortcuts sheet, the compact
  rail and the reduced-motion path. Linux CI uploads the screenshot.
- **The macOS CLI tarball is self-contained and macOS 12 compatible.** It is
  built against the same source-built disc libraries, bundled with
  `@executable_path/lib`, and carries ffmpeg, ffprobe and the menu JAR. The
  CLI's `lib/` and `bundled_lib` lookup also make `doctor` and `keys` report the
  bundled AACS backends instead of Homebrew.
- **macOS 12 support.** The DMG was blocked at 15 by Homebrew's deployment
  target. `scripts/build-disc-libs.sh` now builds libgpg-error, libgcrypt,
  libudfread, libaacs, libbdplus and libbluray from source with
  `MACOSX_DEPLOYMENT_TARGET=12.0`, without freetype, fontconfig or libxml2.
  The app, the bundled libraries and the ffmpeg all report `minos 12.0`, and CI
  refuses a DMG with any bundled item above 12.0.
- **A self-contained Linux ffmpeg.** The Linux app now builds the same
  ffmpeg n9.0.2 as macOS, with libdvdread, libdvdnav, libx264 and libx265, and
  bundles it. DVD-Video works without installing a newer system ffmpeg. CI
  checks the `dvdvideo` demuxer and the encoders before packaging.
- **Native AACS on Linux and Windows.** The macOS path's crypto half is now a
  portable C file backed by a vendored libaacs 0.11.1 subset. Linux reads the
  drive over `SG_IO`; Windows uses `IOCTL_SCSI_PASS_THROUGH_DIRECT`. Both try
  LibreDrive's vendor READ BUFFER first, then a KEYDB host certificate. The
  MSVC app loads a MinGW-built `keepsake-aacs.dll` because vcpkg's libgcrypt
  port is not available for MSVC. The GNU CLI links the code directly. Unit
  tests cover the MMC CDBs and the LibreDrive record scan.
- **BD-J render check.** `scripts/test-menu.sh <disc> <out>` runs the shipped
  JAR with the disc's own menu data, captures three frames, and asserts
  navigation, playlist selection and the return after end of media. Linux CI
  runs it under Xvfb and uploads the frames.
- **Portable UDF 2.50 authoring.** Linux and Windows can now merge multi-title
  discs into a UDF 2.50 ISO instead of stopping at a BDMV folder. A pure-Rust
  writer (`hadris-cd`) streams the staged tree from disk and emits the BD-R
  profile. Verified by authoring two titles, mounting the image and reading both
  with libbluray. macOS keeps `newfs_udf`, which adds the metadata partition
  that BD-RE needs; `KEEPSAKE_PORTABLE_UDF=1` forces the portable path for
  testing. Decrypted ISO export now tries the portable writer first on every
  platform, so it no longer depends on `hdiutil` or `mkisofs`.
- **Windows burning through IMAPI2.** `burn` now drives Windows' own COM writer
  stack from PowerShell, so no extra tool is required. It picks the first
  compatible recorder or the drive letter passed as `--device D:`, sets the
  write speed, refuses write-protected media and closes the session. The script
  is unit-tested; a physical write still needs a writer and a blank disc.
- **Browser fixture for interface work.** `cd app && npm run dev:mock` answers
  the Tauri commands with fixtures, so the shell can be laid out, clicked and
  screenshotted without a disc drive. It is loaded behind `import.meta.env.DEV`
  and never reaches a release bundle.
- `DESIGN.md` and `docs/design-skills.md` record the interface direction, the
  token set, the motion inventory and the skill routing used for UI work.

### Changed

- **Second interface pass.** A `?` shortcuts sheet, a live conic progress ring
  in the session card, and a status-bar shortcut link. The palette gained a
  shortcuts entry.
- **New desktop interface.** The app is now a sectioned shell: a rail for the
  eight stages, a focused workspace, a persistent session inspector with the
  primary action, progress and queue, and a status bar. Output settings, keys,
  episodes, authoring, artwork and menus each have their own page instead of
  sharing one long column. The disc page shows a three-step start panel before
  a disc is read.
- **Motion and input pass.** Page, panel and rail entrances, primary-button
  sheen, live progress and a command palette. `Cmd/Ctrl+K` opens the palette,
  `Alt+1..8` jumps between sections, and title rows and artwork tiles are
  keyboard operable. Every animation has a `prefers-reduced-motion` path.
- `app/src/styles.css` is rebuilt around a single token set (ink surfaces,
  laser spectrum, brass accent, mono data) instead of ad hoc colours.

- The GitHub repository and the local folder are now `keepsake-atelier`
  (`github.com/ghostyshell/keepsake-atelier`). The Cargo `repository` field and
  the app's updater endpoint point at it. Crate and binary names remain
  `keepsake-atelier`.
- **The desktop app is now Keepsake Atelier**, with a new app icon and in-app
  logo. Product name, window title, bundle identifier
  (`click.keepsakeatelier.app`) and the docs were updated. The engine and CLI
  binary are still `keepsake-atelier`, and the data directory
  (`~/Library/Application Support/keepsake-atelier`) is unchanged so existing keys,
  menus and settings carry over.
- The desktop app now surfaces the menu documents (parse, edit, save, apply),
  native key derivation for the inserted disc, BD+ VM installation, and the
  per-title stream list.
- `plan_backend` tries the fully native key derivation even when MakeMKV is not
  installed, and `keepsake-atelier aacs` prints the VID obtained from the LibreDrive
  vendor read. MakeMKV's `discatt.dat` is now only a fallback for a drive whose
  LibreDrive vendor read fails.

### Fixed

- **Windows ships an ffmpeg that can actually rip DVDs.** The gyan essentials
  build has libx264/x265 but no libdvdread/libdvdnav, so the `dvdvideo`
  demuxer was absent. The bundle now uses BtbN's GPL build and CI asserts the
  demuxer and the two encoders before packaging. The installer grows to about
  99 MiB as a result.
- **Windows CI no longer depends on `ntldd`.** The MSYS2 package download came
  back checksum-corrupt and failed the installer job before it started. The
  workflow now walks `ldd` output recursively, which ships with the MSYS2
  runtime.
- **The Windows CLI zip carries libaacs and libbdplus.** Both are `dlopen`ed by
  libbluray, so they never appear in the PE import table. The packaging step now
  copies them under the names libbluray looks up, plus their own dependency
  closure.
- **The Windows CLI has disc support again.** `build.rs` sent every Windows
  target through vcpkg, so the MinGW build silently compiled without libbluray
  and disabled disc features. MSVC keeps the static vcpkg library; MinGW now
  probes through pkg-config. The zip carries `libbluray-4.dll` and its full
  16-DLL closure, and CI fails if libbluray is missing from the PE imports.
- **The macOS DMG is self-contained.** It used to link the build machine's
  Homebrew libraries and failed with a dyld error on a clean Mac.
  `scripts/bundle-app.sh` rebases libbluray and its dependencies, fixes and
  copies the dlopen'ed libaacs/libbdplus from temporary copies, signs every
  dylib and the app, and refuses to ship if a `/opt/homebrew` or `/usr/local`
  reference remains. The app and doctor use the bundled paths. Verified by
  mounting the DMG and launching the app from the mounted volume.

### Verified

- **Windows artifacts are executed in CI.** The CLI zip runs `--version` and
  `doctor` with its shipped DLLs, and the NSIS installer is silently installed,
  its exe located, and the app required to stay alive for 15 seconds. Before
  this the installer and the CLI had only been built, never run.
- **Live disc pass (LG BD-RE BU40N, pressed AACS Blu-ray):** disc info and the
  60-title map, the menu document parse (BD-J first play/top menu + 49 nav
  titles), tree verification, and a lossless title rip (H.264 + AC-3 + PGS,
  24.5 s MKV) all worked. `hdiutil burn` sees the drive and a 17 MiB authored
  UDF 2.50 image passes `-sizequery`; an actual write still needs a blank
  BD-RE/BD-R (the inserted disc was a pressed BD-ROM).
- Native key recovery could not be exercised: this drive's stock firmware has
  no LibreDrive vendor read ("no LibreDrive VID record"), so `keys native`
  fails here while libaacs + the existing KEYDB entry decrypts normally.

### Added

- **Project site and release mirrors:** a GitHub Pages landing page under `docs/`
  (single-file HTML, icon, social preview, sitemap, manifest), a logo and
  download table in the README, and Support links for Discord, Ko-fi and ETH.
  Builds are published to GoFile (primary) and file.kiwi (backup), with the
  current links in `docs/downloads.json`.
- **Release publishing pipeline:** `scripts/publish-release.sh` builds the DMG
  and CLI tarball, uploads both mirrors, regenerates the README table and the
  site manifest, commits, pushes and removes the local artifacts when both
  mirrors succeeded. `scripts/refresh-release-links.sh` is the weekly entry
  point; `scripts/install-refresh-job.sh` installs it as a launchd user agent
  (Sunday 03:47 under `caffeinate -i`), so expiring free-tier links are renewed
  without a manual edit.
- **Documentation set:** `docs/features.md` plus task-shaped guides for install,
  ripping, authoring, menus, keys, DVD, TV, burning, MCP and troubleshooting, and
  `docs/publishing.md` for the release and mirror workflow.
- **Authored BD-J title menus (opt-in):** `author --menu` adds a real 1920x1080
  menu as first play and top menu. `bdjo.rs` writes `BDMV/BDJO/00000.bdjo`;
  `bdmv::install_menu` rewrites `index.bdmv` (BD-J first play/top menu, one HDMV
  object per title) and installs `JAR/00000.jar` + `JAR/menu.txt`. The single
  generic Xlet (`core/assets/menu/00000.jar`, rebuilt by `scripts/build-menu.sh`)
  reads the title rows and an optional background from `menu.txt` at runtime, so
  no per-disc Java is compiled. Up/Down/Enter/1-9 and mouse select a title and
  start it with `bd://1.PLAYLIST:0000N`; the menu returns when the title ends.
  `--menu-background` and `--menu-template` (saved menu document) are optional.
  The app Author card and MCP `author_start` expose the same flags.
  Verified with libbluray parsing the BDJO/index and with the Xlet running in a
  stubbed BD-J harness (render, selection, playlist request, return). Hardware
  playback is untested and libbluray on macOS cannot run BD-J itself (its JVM
  enters the AppKit loop inside `bdj_open`), so the menu stays opt-in.
- **BD-J checks in `verify`:** first play / top menu BD-J objects require their
  `BDJO/xxxxx.bdjo` and an application JAR; an authored `JAR/menu.txt` is checked
  against the playlists. Real pressed discs' runtime-selected playlists (GPR
  operands) now warn once instead of failing every title.
- **Queue all and a key-store browser:** the Titles card can queue every real
  title (>= 1 minute) with a "queued" badge per row, and the keys panel searches
  the 189k-entry KEYDB (name or id, first 20), imports another KEYDB.cfg and
  removes entries. `keys list --search` and the MCP `keys_list` `search` filter
  do the same from the CLI/agents.
- **MCP `clone_start`:** the 1:1 encrypted disc copy is now a job tool too.
- **DVD-Video support:** `dvd list` reads the VMG title table from
  `VIDEO_TS.IFO`, and `dvd rip --title N` copies MPEG-2 + AC-3/DTS with the
  bundled `dvdvideo` demuxer (no re-encode, optional VobSub subtitles). The app
  shows a DVD-Video card with a title list and rip button; MCP has `dvd_info`
  and `dvd_start`. No menus are ripped.
- **Bundled ffmpeg/ffprobe:** `scripts/build-ffmpeg.sh` builds x264 (master),
  x265 4.2, libdvdread 7.1.1, libdvdnav 7.0.0 and ffmpeg n9.0.2 into a static
  binary, then copies it into the app bundle. Adds the `dvdvideo` demuxer (DVD
  is no longer toolchain-blocked), GPL x264/x265 encoders, VideoToolbox and the
  native TrueHD/DTS/Atmos/AC-3/E-AC-3 codecs. `core::binaries` prefers the
  bundled helper (env override, executable dir, `.app/Contents/Resources`, dev
  tree, managed dir, PATH), and `doctor` reports the resolved build's features.
  See `docs/ffmpeg.md` (including the GPLv2+ distribution obligations).
- **System check:** `keepsake-atelier doctor` (and an app **System check**
  panel, plus an MCP `doctor` tool) lists the installed stack, the ffmpeg
  `dvdvideo` demuxer, tsMuxeR, KEYDB, drive and burning tools, then a capability
  matrix with the fix for each missing item, so DVD/burning limits are
  diagnosable rather than mysterious.
- **BDMV verification:** `keepsake-atelier verify <path>` (and **Verify** in the
  app, plus an MCP `verify` tool) parses index/MovieObject and every playlist
  directly, maps each title to its playlist, checks the clip files exist, and
  reports duration and mark counts. Non-zero exit and per-title errors on
  problems; BD-J titles are reported as unchecked.
- **Multi-title authoring and BD-25/50 splitting:** `author` takes several
  inputs and writes one title each into a BDMV folder, renumbering clips and
  generating `index.bdmv`/`MovieObject.bdmv` (one `PlayPL` object per title,
  `JumpTitle` first play/top menu). `--split <GB>` (and a split field in the app)
  packs whole inputs onto discs of at most that size under `<out>/DiscN`, never
  cutting a title. MCP: `author_start` with `inputs` and `split_gb`.
- **Menu templates feed the rip:** when a saved template's disc id matches the
  source, the selected title's name becomes `{title}` (a trailing `(1967)` is
  split off as `{year}`), and its chapter labels replace the ChapterDB names
  when the counts match. Editing a template now changes the next rip.
- **Multi-title ISOs, natively:** macOS's `newfs_udf` writes a UDF 2.50 image
  with a metadata partition, and `author` lays the merged multi-title BDMV into
  it via `hdiutil`, so `--out disc.iso` works for several inputs (and `--split`
  writes `set/DiscN.iso`). Verified with two titles (mount, `verify`, libbluray).
- **Native burning:** `burn` now prefers macOS's `hdiutil burn` (auto-detects the
  writer); `--device /dev/rdiskN` selects growisofs. **Burn ISO to disc** in the
  app and `burn_start` over MCP use the native path.
- **Burning (single-title ISOs):** `burn <image.iso> --device /dev/rdiskN
  [--yes]` writes with growisofs (cdrecord fallback), refusing a mounted device
  or missing image and requiring an explicit `--yes` (dry run by default). The
  app's ISO panel has a confirmed **Burn ISO to disc** button. Multi-title discs
  are folder-only because macOS has no UDF 2.50 image writer.
- **Native AACS2 unit-key derivation:** `AACS/Unit_Key_RO.inf` is parsed (AACS1
  48-byte records, AACS2 64-byte; `Content000.cer[0] == 0x10` marks AACS2) and
  each encrypted unit key is AES-128-ECB decrypted with the VUK. `keys native`
  and `keys from-makemkv` now write the `U` table into KEYDB automatically, and
  the count is printed/displayed. Unit tested with a synthetic UKF for both
  layouts.
- **AACS2/UHD unit-key workflow:** `keepsake-atelier keys show [--disc|--from]
  [--json]` prints a disc's KEYDB entry including unit keys, and the app's keys
  panel shows the VUK/unit-key state with an editor that saves the `U` table.
  Unit keys are imported, not derived; the entry must already exist.
- **1:1 disc clone** (`keepsake-atelier clone`, and "Clone 1:1" in the app): a raw
  sector copy that keeps AACS/BD+ intact, for an archival image or a burner.
  Uncached reads, byte-based progress, cancel removes the partial file. Unit
  tested against a file source.
- **Batch rip queue** in the desktop app: "Start rip" enqueues the selected
  title (or TV plan), and `+` on a title row queues that title. Jobs run one at
  a time, with a queue panel for state, progress, cancel, resume and cleanup.
  The queue persists to `queue.json` and is restored on launch, but never
  auto-starts.
- **Disc menu documents** (`keepsake-atelier menu`): parse a disc's menu into an
  editable JSON document, save it as a named template, and apply it to a
  different disc. The document holds the `BDMV/index.bdmv` navigation table
  (first playback, top menu, HDMV movie objects and BD-J object names) and the
  playable titles with editable names, main-feature flag and chapter labels.
  `apply` matches by playlist id, then duration, then position. See
  `docs/menu-templates.md`; the roadmap is in `docs/roadmap.md`.
- **Native VID acquisition, no certificate and no MakeMKV.** A LibreDrive drive
  (this BU40N) decrypts the AACS volume id in its own firmware and serves it over
  vendor `READ BUFFER (0x3c)` buffer id `0x4410`: a 36-byte record
  `00 22 | 00 50 | <16-byte VID> | 16 zeros`. `bf_aacs_vid_libredrive()` issues
  that read, and `bf_native_key()` prefers it, so `keepsake-atelier keys native` now
  derives VID, MK, and VUK with **no host certificate at all**. The certificate
  path remains as the fallback for a non-LibreDrive drive.
  (`makemkvcon` sends zero `SEND KEY`/`REPORT KEY` and writes nothing to the
  drive; the mechanism was captured by launching it under lldb and breaking on
  `SCSITaskClass::SetCommandDescriptorBlock`.)
- **Native key derivation for an unkeyed disc** (`keepsake-atelier keys native`): the
  drive is read for the volume id (LibreDrive vendor read, or AGID + host
  certificate when needed), the media key is recovered from the disc's own MKB
  with the KEYDB device keys, and the VUK is derived. No MakeMKV, no pre-existing
  KEYDB entry.
- **`keepsake-atelier bdplus [--install]`**: install the public BD+ VM data into
  libbdplus's config dir, so BD+ discs also come off MakeMKV.
- **Native libaacs decryption is now the default.** A disc whose key is in
  `KEYDB.cfg` decrypts and rips with no MakeMKV. MakeMKV's `libmmbd` is used
  only as a fallback (unkeyed disc, or BD+). Force either backend with
  `KEEPSAKE_DECRYPT=aacs|mmbd`.
- **Key store:** `keepsake-atelier keys add|list|show|remove|import` over `KEYDB.cfg`,
  disc id from `AACS/Unit_Key_RO.inf`, with unit tests. MCP tools `keys_list`,
  `keys_add`, `keys_remove`. `keys update` now merges: an entry added locally
  and not yet in the community database survives a refresh.
- **`keepsake-atelier rip --retries N`** (default 2) re-runs a failed rip end to end,
  covering the transient `Unable decrypt unit` seen on the first read after a
  drive replug.
- `keepsake-atelier libs` and `keepsake-atelier disc` print the chosen decrypt backend; the rip
  logs it with the disc id and keyed flag.
- `docs/decryption.md`: the AACS split, the working setup, the traps, and the
  remaining gap (native AGID drive auth).

### Fixed

- `mount_point_for_device` accepted only an exact `/dev/...` string, so a
  `/dev/diskN` source failed to resolve its mount point. That made a keyed disc
  look unkeyed (routing to the native attempt and libmmbd instead of libaacs).
  It now matches on the BSD name regardless of `/dev/` or `r` prefix.

### Added

- `keepsake-atelier keys from-makemkv`: recover a disc key from MakeMKV's `discatt.dat`
  VID plus offline MKB derivation (`VUK = AES-G(MK, VID)`), bypassing the host
  certificate entirely. Also caches the VID/RDK where libaacs expects them, and
  `plan_backend` falls back to it when native auth is refused.

### Fixed

- `calc_pk` narrowed `dev_key_v_mask` with a logical shift; libaacs uses an
  arithmetic shift, without which the loop never converges and media-key
  derivation hangs.

### Changed

- The libaacs backend is handed a *mounted* volume (libaacs resolves the drive
  via statfs and manages unmount/remount itself); an unkeyed disc now routes to
  libaacs, not libmmbd. The native path links libaacs's inner crypto/MKB/MMC
  helpers (`libaacs.a`) so the AACS maths is not reimplemented.
- Native libaacs cannot read a mounted volume on this stack, so a rip now
  unmounts the disc, reads the raw `/dev/rdiskN`, and remounts afterwards. The
  backend is resolved before the unmount and threaded through nested opens.
- Workspace formatted with `cargo fmt`.
