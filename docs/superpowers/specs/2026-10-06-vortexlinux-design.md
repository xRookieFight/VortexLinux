# VortexLinux design

## Goal

Run the Windows build of the Vortex client (https://playvortex.io) on Linux through Wine or Proton, with a one command setup: menu entry, working `vortex://` links, and the game installed and kept runnable.

## What we know about the client

* Distributed as `https://cdn.playvortex.io/releases/client/<version>/Vortex-Windows.zip`, containing a single portable `Vortex/Vortex.exe` (unsigned, about 160 MB).
* Current version is served by `https://playvortex.io/api/client-version` as `{"version":"0.6.3"}`.
* Rust + Bevy + wgpu. It picks Vulkan, DX12 or GL at runtime, so Wine's own winevulkan is enough. No anti cheat found in the binary.
* The exe is the whole client, menus and login included, and it has its own in app updater.
* It registers a `vortex://` URL scheme and stores settings under `%APPDATA%`.

## Shape

One Rust binary, `vortexlinux`, with subcommands:

* `install` copies itself to `~/.local/bin/vortexlinux`, downloads the game if missing, writes the desktop entry and icon, registers `x-scheme-handler/vortex`.
* `run [URL]` makes sure the game and prefix exist, then starts `Vortex.exe` with the optional `vortex://` URL. This is what the desktop entry calls.
* `update [--force]` checks the API and replaces `Vortex.exe` when the remote version differs from the one we recorded.
* `uninstall [--purge]` removes the desktop entry, icon and scheme registration. `--purge` also deletes the prefix and game files.
* `doctor` reports runner, Vulkan, game and prefix status.

Vortex updates itself, so our updater is the bootstrap and fallback path. `check_updates_on_launch` (off by default) lets `run` call it every time in case the in app updater misbehaves under Wine.

## Modules

* `config`: `~/.config/vortexlinux/config.toml`. Fields: `runner` (`wine` or `proton`), `wine` (binary, default `wine`), `proton` (Proton install dir), `check_updates_on_launch`, `env` (extra environment table). Missing file means defaults.
* `paths`: XDG locations. Data root `~/.local/share/vortexlinux` holding `game/Vortex.exe`, `game/version`, `prefix/` (Wine) and `compatdata/` (Proton).
* `updater`: fetch version, validate it as `N.N.N`, download the zip to a temp file next to the target, extract only `Vortex/Vortex.exe`, rename into place, write `game/version`.
* `runner`: builds the `Command`. Wine: `WINEPREFIX=prefix`, runs `wineboot -u` once when the prefix has no `system.reg`. Proton: uses `umu-run` with `PROTONPATH` and `WINEPREFIX` if it is on `PATH`, otherwise `<proton>/proton run` with `STEAM_COMPAT_DATA_PATH` and `STEAM_COMPAT_CLIENT_INSTALL_PATH`.
* `desktop`: desktop entry text, icon (favicon.ico from the site, largest frame saved as PNG), `xdg-mime` and `update-desktop-database` calls.
* `doctor`: plain checks, printed one per line.

## Error handling

* `vortex://` arguments are accepted only with that scheme, no control characters, max 2048 bytes. Anything else is rejected before it reaches Wine.
* Network failure in `run` with the game already installed only logs a warning and launches.
* Downloads go to a `.part` file and are renamed only after a full extract, so an interrupted update never leaves a broken exe.
* Missing runner binaries produce an error that names the binary and points to `doctor`.

## Testing

Unit tests for version and URL validation, zip extraction (zip built in the test), config parsing and defaults, desktop entry content, and the commands the runners build (args and env). No network in tests. The real launch is verified by hand.

## Addendum: Vortex Studio (2026-10-07)

Studio ships as its own build: `https://playvortex.io/api/studio-version`, `https://cdn.playvortex.io/releases/studio/<version>/VortexStudio-Windows.zip` with `VortexStudio/VortexStudio.exe` inside, and the `vortex-studio://` scheme. Everything app specific lives in `src/app.rs` as `App::Client` and `App::Studio`, and every subcommand takes `--studio`. Studio installs into `studio/` next to `game/` and shares the Wine prefix. It is opt in through `install --studio`, and a plain `install` keeps it once present.

Studio ignores `WGPU_BACKEND` and would still pick DX12, so the launch environment also disables Wine's `d3d12` and `d3d12core`, which makes wgpu fall back to Vulkan for both apps. Menu icons are now extracted from each exe instead of the site favicon.
