# 🐾 PawPause — A Pomodoro Timer for the COSMIC Desktop

**PawPause** is a native [Pomodoro timer](https://en.wikipedia.org/wiki/Pomodoro_Technique) applet
for the [COSMIC desktop](https://system76.com/cosmic/) on Linux (Pop!_OS / Wayland). Instead of a
plain notification, your breaks are announced by a cat walking onto your screen as a transparent,
click-through wallpaper-layer overlay — then it curls up and rests until your break is over, and
walks back off right as it ends. PawPause also tracks your tasks and projects, and logs focused-time
statistics with interactive charts.

Built in Rust with [`libcosmic`](https://github.com/pop-os/libcosmic) — a lightweight, native
COSMIC panel applet, not an Electron app.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Made with Rust](https://img.shields.io/badge/Made%20with-Rust-orange.svg)](https://www.rust-lang.org/)
[![COSMIC Desktop](https://img.shields.io/badge/Desktop-COSMIC-7C3AED.svg)](https://system76.com/cosmic/)
[![Platform: Linux](https://img.shields.io/badge/Platform-Linux%20(Wayland)-blue.svg)](#requirements)

---

## Contents

- [Features](#features)
- [Requirements](#requirements)
- [Install](#install)
  - [Option A: Download a release (no compiling)](#option-a-download-a-release-no-compiling)
  - [Option B: Build from source](#option-b-build-from-source)
- [Usage](#usage)
- [Configuration](#configuration)
- [How the break overlay works](#how-the-break-overlay-works)
- [Development](#development)
- [Contributing](#contributing)
- [License](#license)

---

## Features

- **Pomodoro timer** — configurable work / short-break / long-break durations, with a
  session-count cycle before a long break, live in your COSMIC panel.
- **Animated break overlay** — during breaks, a cat clip plays as a transparent
  [`zwlr_layer_shell_v1`](https://wayland.app/protocols/wlr-layer-shell-unstable-v1) overlay (via
  [`mpvpaper`](https://github.com/GhostNaN/mpvpaper)) instead of a plain popup notification. Supports
  a two-clip walk-in → rest (looping) → walk-out flow, with the walk-out reusing the entry clip's
  own frames played in reverse — no extra assets needed.
  Alpha transparency works both for single video files (chroma-keyed) and PNG frame-sequence
  folders (real per-pixel alpha, e.g. for clips produced via AI background removal).
- **Optional wallpaper blur** — softly blurs your actual desktop wallpaper during breaks (not the
  cat), restored exactly when the break ends.
- **Tasks & Projects** — a full task manager with subtasks, per-task projects, and roll-up
  progress, in a standalone companion window launched from the applet.
- **Focus statistics** — hours focused, day streak, days accessed, a daily-goal progress
  indicator, and interactive canvas charts: weekly per-project breakdown, a 14-day focus trend, a
  12-week GitHub-style contribution heatmap, and laptop-usage-vs-focus-time comparison sourced from
  your real system boot history.
- **CSV export** of your logged focus sessions.
- Config, tasks, and stats persist as plain JSON under `~/.config/pawpause/` — easy to inspect,
  back up, or script against.

## Requirements

PawPause is built specifically for the **COSMIC desktop environment** on **Linux + Wayland**
(the default desktop on Pop!_OS 24.04+, also installable standalone on many other distros). It will
not run on X11-only sessions, GNOME, KDE, or macOS/Windows.

Runtime dependencies (install via your distro's package manager):

| Tool | Used for | Required? |
|---|---|---|
| [`mpvpaper`](https://github.com/GhostNaN/mpvpaper) | Playing the break-video overlay | Optional — overlay is skipped gracefully if missing |
| `ffmpeg` / `ffprobe` | Wallpaper blur, probing clip duration | Optional — blur/duration features degrade gracefully |
| `cosmic-randr` | Listing Wayland outputs for the overlay/settings | Ships with COSMIC |

## Install

### Option A: Download a release (no compiling)

Grab the latest prebuilt binaries from the
**[Releases page](../../releases/latest)** — no Rust toolchain required.

```sh
# Download and extract the latest release tarball, then:
tar xzf pawpause-*-x86_64-linux.tar.gz
cd pawpause-*-x86_64-linux

# Install the two binaries
mkdir -p ~/.local/bin
cp pawpause pawpause-applet ~/.local/bin/

# Install the desktop entries so the applet shows up in COSMIC's applet list
mkdir -p ~/.local/share/applications
cp *.desktop ~/.local/share/applications/
```

Then add **PawPause** to a COSMIC panel via **Settings → Desktop → Panel → Configure panel applets**,
or log out/in so COSMIC picks up the new applet entry.

### Option B: Build from source

Requires a working [Rust toolchain](https://rustup.rs/) (stable) and a C linker (`build-essential`
on Debian/Ubuntu-based distros, which Pop!_OS is).

```sh
git clone https://github.com/cityji/pawpause.git
cd pawpause/pawpause-applet

# Build both binaries in release mode
cargo build --release

# Install
mkdir -p ~/.local/bin ~/.local/share/applications
cp target/release/pawpause target/release/pawpause-applet ~/.local/bin/
cp data/*.desktop ~/.local/share/applications/
```

`libcosmic` is pulled directly from its `git` source, so the first build needs a working internet
connection and will take a while (it compiles the whole COSMIC toolkit stack). Subsequent builds
are incremental and much faster.

To build and run without installing, for development:

```sh
cargo run --bin pawpause-applet   # the COSMIC panel applet
cargo run --bin pawpause          # the standalone Tasks/Statistics window
```

## Usage

1. Add the **PawPause** applet to a COSMIC panel.
2. Click the panel pill to open the popup: **Start Pomodoro** begins a work session.
3. Use **Settings** in the popup to set work/break durations, pick your break video(s), choose the
   target Wayland output for the overlay, and set a wallpaper-blur amount.
4. Click **Open PawPause** to manage tasks/projects and view your focus statistics.

## Configuration

All state lives under `~/.config/pawpause/` as plain JSON:

| File | Contents |
|---|---|
| `config` | Timer durations, break video paths, blur %, daily focus goal |
| `tasks.json` | Tasks, subtasks, and projects |
| `sessions.json` | Logged focus sessions, used for statistics |

Every file is created with sensible defaults on first run, and a corrupt file falls back to
defaults (with a desktop notification) rather than silently discarding your data.

## How the break overlay works

PawPause keeps a single `mpvpaper` process alive for the whole break and drives the walk-in → rest
→ walk-out sequence over mpv's JSON IPC (`loadfile ... replace`), rather than killing and
respawning a process per phase — so there's no flicker or gap at each transition. The walk-out
clip is the entry clip's own frames, played back in a different order via mpv's `mf://@listfile`
input, so no extra video asset or memory-heavy frame buffering is needed to get a mirrored
"leaving" animation.

## Development

See [`pawpause-applet/`](pawpause-applet) for the Rust workspace. Two binaries share one library
crate:

- `pawpause-applet` — the always-running COSMIC panel applet (timer, overlay, wallpaper blur).
- `pawpause` — the standalone Tasks / Projects / Statistics window.

```sh
cd pawpause-applet
cargo test               # unit tests
cargo build --release    # release binaries
```

## Contributing

Issues and pull requests are welcome. If you're proposing a larger change, please open an issue
first to discuss the approach.

## License

[MIT](LICENSE)
