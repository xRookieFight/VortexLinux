# VortexLinux

Play and build on [Vortex](https://playvortex.io) from Linux. VortexLinux downloads the official Windows client and, if you want it, Vortex Studio, runs them through Wine or Proton, adds them to your app menu and makes `vortex://` and `vortex-studio://` links from the website open the right app.

> VortexLinux is an unofficial community project. It is not affiliated with or endorsed by Vortex. It never modifies the game, it only downloads the official build from the Vortex CDN and starts it.

## Requirements

* x86_64 Linux with working Vulkan drivers (`vulkaninfo --summary` should list your GPU)
* Wine 9 or newer, or a Proton build (Steam Proton, GE-Proton)

On Fedora, install the full Wine package set. `wine-core` alone is missing the files Wine needs to create a prefix, and setup hangs forever on the "updating Wine configuration" dialog.

```sh
sudo dnf install wine            # or at least: wine-core wine-common
```

On Debian and Ubuntu, `sudo apt install wine64` is enough. On Arch, `sudo pacman -S wine`.

## Install

Grab the latest binary from [Releases](https://github.com/xRookieFight/VortexLinux/releases) and run its installer:

```sh
curl -LO https://github.com/xRookieFight/VortexLinux/releases/latest/download/vortexlinux-x86_64-linux
chmod +x vortexlinux-x86_64-linux
./vortexlinux-x86_64-linux install
```

`install` copies the launcher to `~/.local/bin/vortexlinux`, downloads Vortex, adds a **Vortex** entry to your app menu and registers it as the handler for `vortex://` links. You can delete the downloaded file afterwards.

Want to make games too? Add Vortex Studio, it gets its own menu entry and handles `vortex-studio://` links:

```sh
vortexlinux install --studio
```

Or build it yourself (Rust 1.88+):

```sh
cargo build --release
./target/release/vortexlinux install
```

## Usage

Start Vortex or Vortex Studio from your app menu, or press **Play** on any game on playvortex.io and allow your browser to open the link. The first start takes a few extra seconds while the Wine prefix gets created.

The CLI does the same and a bit more:

| Command | What it does |
| --- | --- |
| `vortexlinux run [vortex://...]` | Start Vortex, optionally straight into a link |
| `vortexlinux run --studio [vortex-studio://...]` | Start Vortex Studio |
| `vortexlinux update [--studio] [--force]` | Download the latest client (or Studio) if ours is older |
| `vortexlinux doctor` | Check Wine, Vulkan, both apps and the prefix |
| `vortexlinux uninstall --studio [--purge]` | Remove only Studio, `--purge` also deletes its files |
| `vortexlinux uninstall [--purge]` | Remove everything, `--purge` also deletes downloads, prefix and config |

Both apps update themselves, so you rarely need `update`. It's there for when the built in updater fails. Running plain `install` again keeps Studio if it's already installed.

## Configuration

`~/.config/vortexlinux/config.toml` is created on install. Every key is optional.

```toml
runner = "wine"            # or "proton"
wine = "wine"              # name in PATH or full path
# proton = "/home/you/.steam/steam/steamapps/common/Proton - Experimental"
check_updates_on_launch = false

[env]
# anything here is passed to the game
# DXVK_HUD = "fps"
```

With `runner = "proton"`, VortexLinux uses [umu-launcher](https://github.com/Open-Wine-Components/umu-launcher) when `umu-run` is on your PATH and calls Proton's own `proton run` otherwise.

Files live in the usual places:

* `~/.local/share/vortexlinux/game` holds the client, `studio` holds Vortex Studio
* `~/.local/share/vortexlinux/prefix` is the Wine prefix both apps share (`compatdata` for Proton)
* Vortex's own logs end up in `prefix/drive_c/users/$USER/AppData/Local/Vortex/logs`

## How it works

The client and Studio are each a single Rust executable built on Bevy and wgpu, shipped separately with their own version API and download. That makes them friendly Wine guests: no anti cheat, no .NET, no installer. Two things need care:

* **Graphics backend.** wgpu picks DX12 first, and under Wine that goes through vkd3d, which reports a fake GPU and crashes on the first texture. VortexLinux disables Wine's `d3d12` so wgpu falls back to Vulkan and talks to your driver through winevulkan. It also sets `WGPU_BACKEND=vulkan`, which the client honors but Studio ignores, hence the DLL override.
* **Menu clutter.** Wine's `winemenubuilder` is disabled too, so it doesn't fill your menu with `wine-extension-*` entries.

Both overrides live in `WINEDLLOVERRIDES`. If you set that variable yourself in `[env]`, yours replaces ours completely, so keep `d3d12,d3d12core=` in it.

Links work the same way as on Windows, where the apps register `"Vortex.exe" "%1"` for `vortex` and `"VortexStudio.exe" "%1"` for `vortex-studio`. Here the desktop entries run `vortexlinux run %u` and `vortexlinux run --studio %u`, which check that the link uses the app's own scheme and pass it on.

## Troubleshooting

Run `vortexlinux doctor` first, it catches most problems.

* **"Updating Wine configuration" never finishes.** Your Wine install is missing its data files. On Fedora install `wine-common`.
* **Black window or a crash right after start.** Check `doctor` lists a real GPU under vulkan, not only `llvmpipe`. Install your distro's Vulkan driver (`mesa-vulkan-drivers`, `vulkan-radeon`, `nvidia-utils` and so on). If the app log mentions `backend: Dx12` or `HD Graphics 4000`, something replaced our `WINEDLLOVERRIDES`, see above.
* **Play on the website does nothing.** Run `xdg-mime query default x-scheme-handler/vortex`, it should print `vortexlinux.desktop` (`vortexlinux-studio.desktop` for `x-scheme-handler/vortex-studio`). If not, run `vortexlinux install` again.
* **Starting over.** `vortexlinux uninstall --purge && vortexlinux install` (keep a copy of the binary first, uninstall removes it).

When opening an issue, include the output of `vortexlinux doctor` and the latest file from the Vortex logs folder above. Studio prints its log to the terminal, so run `vortexlinux run --studio` from one and copy what it says.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

[MIT](LICENSE). Vortex and Vortex Studio are proprietary software owned by their developers, VortexLinux doesn't ship any part of them.
