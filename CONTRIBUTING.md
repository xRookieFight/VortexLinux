# Contributing

Thanks for wanting to help. Bug reports, distro notes and pull requests are all welcome.

## Reporting bugs

Open an issue using the bug report template. The two things that help most are the output of `vortexlinux doctor` and the newest log from `~/.local/share/vortexlinux/prefix/drive_c/users/$USER/AppData/Local/Vortex/logs`. Mention your distro, GPU and how Wine or Proton was installed.

If the game itself misbehaves the same way on Windows, that's a Vortex bug and belongs with the Vortex team, not here.

## Development

You need Rust 1.88 or newer. Wine is only needed to actually launch the game.

```sh
cargo build
cargo test
cargo fmt --all
cargo clippy --all-targets -- -D warnings
```

CI runs the last three on every pull request, so running them locally saves a round trip.

To try changes without touching your real install, point the XDG dirs somewhere else:

```sh
export XDG_DATA_HOME=/tmp/vl/data XDG_CONFIG_HOME=/tmp/vl/config
cargo run -- update
cargo run -- run
```

`install` and `uninstall` still write to `~/.local/bin`, keep that in mind.

## Layout

| File | Purpose |
| --- | --- |
| `src/main.rs` | CLI and the top level flows for each subcommand |
| `src/app.rs` | Everything that differs between the client and Studio: URLs, file names, schemes |
| `src/paths.rs` | Every file and directory we use, resolved from XDG vars |
| `src/config.rs` | `config.toml` parsing and the default template |
| `src/updater.rs` | Version API, download and safe extraction of `Vortex.exe` |
| `src/runner.rs` | Wine and Proton command building, prefix setup |
| `src/desktop.rs` | Desktop entry, icon and `vortex://` registration |
| `src/doctor.rs` | Environment checks |

## Guidelines

* Keep changes focused, one purpose per pull request.
* Add or update unit tests next to the code you change. Tests must not hit the network or need Wine.
* Anything that ends up on a command line, in a URL or in a file path comes from outside (the API, the browser, the config) and gets validated first. Follow how `App::validate_url` and `parse_version` do it.
* Supporting another Vortex program means a new `App` variant, not new special cases spread around the code.
* Comments explain why, not what. If a Wine quirk forced your hand, say which one.
* Update the README when behavior or configuration changes.

## Commits

Short imperative titles in sentence case, no trailing period, for example `Add Flatpak Wine detection`. No body needed for most changes.

## Releases

Push a tag like `v0.2.0` after bumping the version in `Cargo.toml`. The release workflow builds a static x86_64 binary and attaches it to a GitHub release together with its checksum.
