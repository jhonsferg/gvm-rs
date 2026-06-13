<div align="center">

# 🐹 gvm - Go Version Manager

**A fast, cross-platform Go version manager written in Rust.**
Install, switch, and pin any Go release - no `sudo`, no system dependencies, no fuss.

[![License: MIT](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/built_with-Rust-orange?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-Windows_%7C_Linux_%7C_macOS-brightgreen?style=flat-square&logo=github)](https://github.com/jhonsferg/gvm/releases)
[![Version](https://img.shields.io/badge/version-1.2.0-blueviolet?style=flat-square)](https://github.com/jhonsferg/gvm/releases/tag/v1.2.0)
[![TLS](<https://img.shields.io/badge/TLS-rustls_(no_OpenSSL)-lightgrey?style=flat-square&logo=letsencrypt>)](https://github.com/rustls/rustls)

</div>

---

## ✨ What makes gvm different?

gvm is a Go version manager built from scratch in Rust. It was designed with a single goal: work everywhere, require nothing.

- **No Go required** - you don't need Go installed to install Go. gvm downloads the official toolchain directly from go.dev.
- **No `sudo`, no root** - everything lives under `~/.gvm` in your home directory.
- **Zero system dependencies** - a single static binary is all you need.
- **Truly cross-platform** - one codebase, one behavior across Windows, Linux, and macOS on both x86_64 and ARM64.
- **SHA-256 verified downloads** - every archive is checked against go.dev's official checksum before extraction.
- **Session-scoped activation** - `gvm shell <version>` activates a version for the current terminal only, without touching any files.
- **Full environment setup** - `gvm setup` configures everything: shell hook, login profile PATH (so GUI apps like VSCode find Go), and Windows registry. Works correctly after a fresh install or a shell change.
- **Self-updating** - `gvm upgrade` downloads and replaces the binary in-place.
- **Clean uninstall** - `gvm implode` removes everything gvm ever touched.

---

## 🚀 Features

- 📥 **Install any Go version** - by exact version, minor range, or `latest`
- 🔨 **Build from source** - compile any Go version from the official source tarball with automatic bootstrap detection
- 🌍 **Global default** - set a system-wide version with `gvm use`
- 📌 **Per-project pinning** - drop a `.go-version` file; gvm activates it automatically
- 🔐 **SHA-256 verification** - every download is checked against go.dev's official checksum
- 🐚 **Shell integration** - automatic `PATH` and `GOROOT` injection for PowerShell, Bash, Zsh, and Fish
- ⚡ **`gvm exec`** - run a command with any Go version without changing the global default
- 🩺 **`gvm doctor`** - diagnose your setup with actionable hints
- 🔄 **`gvm upgrade`** - self-update to the latest release from GitHub
- 💣 **`gvm implode`** - completely remove gvm and all installed versions cleanly
- 🏁 **Shell completions** - Bash, Zsh, Fish, and PowerShell
- 🖥️ **Cross-platform** - Windows, Linux, macOS × x86_64 and ARM64

---

## 📦 Installation

### 🪟 Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/jhonsferg/gvm/main/install/install.ps1 | iex
```

> Installs `gvm.exe` to `~\.local\bin`, then automatically runs `gvm setup` which adds the binary directory and `~\.gvm\current\bin` to your user `PATH` via the Windows registry, and injects the shell hook into your PowerShell profile.

### 🐧 Linux and 🍎 macOS

```sh
curl -fsSL https://raw.githubusercontent.com/jhonsferg/gvm/main/install/install.sh | sh
```

> Installs `gvm` to `~/.local/bin`, then automatically runs `gvm setup` which injects the `gvm env` hook into your shell profile (`~/.bashrc`, `~/.zshrc`, etc.) and adds a static `~/.gvm/current/bin` PATH entry to your login profile (`~/.profile` or `~/.zprofile`) so that GUI applications like VSCode and GoLand can find Go without needing an interactive shell.

### 📂 Custom install directory

```powershell
# 🪟 Windows
$env:GVM_INSTALL_DIR = "C:\tools\gvm"; irm .../install.ps1 | iex
```

```sh
# 🐧 Linux / 🍎 macOS
GVM_INSTALL_DIR=~/.bin curl -fsSL .../install.sh | sh
```

### ✅ Verify the installation

```sh
gvm doctor
```

---

## ⚡ Quick Start

```sh
# 📥 Install the latest stable Go release
gvm install latest

# 🌍 Activate it globally
gvm use latest

# 🔍 Check the active version
gvm current

# 📌 Pin a version for the current project
gvm local 1.22

# ⚡ Run tests with a different version, without changing the global default
gvm exec 1.21 go test ./...
```

---

## 📖 Commands

### 📥 `gvm install <version>`

Downloads and installs a Go release from go.dev. The archive is verified against the official SHA-256 checksum before extraction.

```sh
gvm install latest          # 🆕 latest stable release
gvm install 1.22            # 🔢 latest patch of Go 1.22
gvm install 1.22.4          # 🎯 exact version
gvm install 1.22.4 --force  # 🔄 reinstall even if already present
```

---

### 🔨 `gvm build <version>`

Compiles a Go release directly from the official source tarball (`go<X>.<Y>.<Z>.src.tar.gz`). The resulting toolchain is installed into `~/.gvm/versions/` alongside any binaries installed with `gvm install`.

> **Platform note:** Linux and macOS only. Windows support is planned for a future release.

```sh
gvm build 1.24.0               # build an exact release
gvm build 1.24                 # build the latest patch of Go 1.24
gvm build latest               # build the latest stable release
gvm build 1.24.0 --force       # rebuild even if already installed
```

**Disable CGO** (faster build, no C toolchain needed):

```sh
gvm build 1.24.0 --no-cgo
```

**Set a custom bootstrap compiler** (must be already installed via gvm):

```sh
gvm build 1.24.0 --bootstrap 1.22.6
```

**Pass extra environment variables** to `make.bash`:

```sh
gvm build 1.24.0 --env GOAMD64=v3
gvm build 1.24.0 --env GOAMD64=v3 --env CC=clang
```

#### Bootstrap compiler

Go has been self-hosted since version 1.5 - compiling it from source requires a working Go installation as a bootstrap compiler. `gvm build` resolves one automatically:

1. **`--bootstrap <version>`** - use a specific installed version (must be present via `gvm install`)
2. **Highest installed gvm version** - reused with no extra download
3. **Auto-download** - if no Go version is installed at all, gvm downloads the latest patch of the previous minor as a temporary bootstrap and removes it after the build

#### Time and disk requirements

Building Go from source takes **5-15 minutes** and requires approximately **3 GB** of free disk space for the source tree, build artifacts, and final installation.

---

### 🌍 `gvm use <version>` · `gvm default <version>`

Sets the global default Go version. The version must already be installed.

```sh
gvm use latest
gvm use 1.22
gvm use 1.22.4
```

> 💡 The change takes effect in any new terminal session, or immediately after reloading your profile.

---

### 📌 `gvm local <version>`

Writes a `.go-version` file in the current directory. gvm reads this file on every shell startup and activates the pinned version automatically.

```sh
# In your project root:
gvm local 1.21.9
```

> The file contains a plain version string (`go1.21.9`) and can be committed to version control so every contributor uses the same toolchain.

> ⚠️ If the pinned version is not installed, gvm prints a warning and falls back to the global default.

---

### 🗑️ `gvm uninstall <version>`

Removes an installed Go version from disk.

```sh
gvm uninstall 1.21.9
```

---

### 📋 `gvm list`

Lists all locally installed Go versions. The active version is highlighted.

```
  go1.23.0  (active)
  go1.22.4
  go1.21.9
```

---

### 🌐 `gvm list-remote`

Lists stable Go versions available for download from go.dev.

```sh
gvm list-remote          # 📄 latest patch per minor (compact view)
gvm list-remote --all    # 📜 every patch release
```

Already-installed versions are marked with `✓`.

---

### 🔍 `gvm current`

Prints the active Go version and where it came from.

```
go1.22.4  (local .go-version)
```

or

```
go1.23.0  (global)
```

---

### 📂 `gvm path [version]`

Prints the `bin/` directory of the active (or specified) version. Useful for scripting.

```sh
gvm path              # active version
gvm path 1.21         # specific version
export GOROOT=$(dirname $(gvm path))
```

---

### 🐚 `gvm env [--shell <name>]`

Emits shell commands that set `PATH` and `GOROOT` for the active version. This is what the shell hook calls on every prompt.

```sh
eval "$(gvm env)"               # 🔍 auto-detect shell
gvm env --shell bash
gvm env --shell zsh
gvm env --shell fish
```

```powershell
# 🪟 PowerShell
gvm env --shell powershell | Out-String | Invoke-Expression
```

---

### 🔧 `gvm setup [--shell <name>] [--reset]`

Performs **all environment configuration** for gvm. The install scripts run this automatically; you only need it manually after moving the binary, changing your shell, or troubleshooting.

```sh
gvm setup                    # auto-detect shell
gvm setup --shell zsh        # configure a specific shell explicitly
gvm setup --reset            # strip all previous gvm config and re-apply cleanly
gvm setup --shell bash --reset
```

What `gvm setup` configures:

| Platform | What it does |
| -------- | ------------ |
| **Linux / macOS** | Injects `# gvm init` + `# gvm wrapper` into the interactive profile (`~/.bashrc`, `~/.zshrc`, etc.). Also injects a static `export PATH` line into the login profile (`~/.profile` for bash, `~/.zprofile` for zsh) so `~/.gvm/current/bin` is visible to GUI apps (VSCode, GoLand, display managers) that don't source the interactive profile. |
| **Windows** | Injects `# gvm init` + `# gvm wrapper` into the PowerShell profile. Adds the gvm binary directory and `~\.gvm\current\bin` to the user `PATH` in the Windows registry (`HKCU\Environment`) so all apps - including GUI editors - see Go without requiring a shell session. |

**Shell validation:** if `--shell <name>` is passed, gvm checks that the shell is actually installed before writing anything. If not found, it exits with an error listing which shells are available on the system.

**`--reset` flag:** strips every `# gvm ...` block from all managed profiles (and the Windows registry) and re-applies configuration from scratch. Only gvm-managed content is touched - all other profile content is preserved.

> Re-running `gvm setup` without `--reset` is always safe - existing up-to-date blocks are left unchanged and stale ones are updated automatically.

---

### ⚡ `gvm exec <version> <command> [args…]`

Runs any command with a specific Go version injected into `PATH` and `GOROOT`, **without changing the global default**.

```sh
# 🏗️ Build with Go 1.21 while Go 1.22 is the global default
gvm exec 1.21 go build ./...

# 🧪 Run tests on multiple versions in CI
gvm exec 1.20 go test ./...
gvm exec 1.21 go test ./...
gvm exec 1.22 go test ./...

# 🔍 Check the exact Go binary
gvm exec 1.22.4 go version
```

> The exit code of the subprocess is forwarded to the calling process.

---

### 🩺 `gvm doctor [--shell <name>]`

Checks your gvm installation and reports issues with actionable hints:

- 🔍 `gvm` binary is in `PATH`
- 🌍 A global Go version is set
- 💾 The global version is installed on disk
- 📂 `GOROOT` resolves to a valid directory
- 🐚 The `gvm env` hook is present in the shell profile
- 📌 The local `.go-version` (if any) is installed

```sh
gvm doctor
gvm doctor --shell zsh
```

> Exits with code `1` if any issue is found - perfect for CI health checks.

---

### 🔄 `gvm upgrade [--force]`

Self-updates gvm to the latest release published on GitHub.

```sh
gvm upgrade           # 🔍 check and update if a newer version exists
gvm upgrade --force   # 🔄 reinstall the latest even if already up to date
```

> 🔒 On Unix the replacement is **atomic** (same-filesystem rename). On Windows the old binary is renamed first to free its name, then the new binary takes the original path. A rollback is attempted automatically if the replacement fails.

---

### 💣 `gvm implode [--force]`

**Completely removes gvm** and everything it manages from the system.

```sh
gvm implode           # 🗑️ shows a summary, asks for confirmation
gvm implode --force   # 💥 removes everything immediately, no questions asked
```

What gets removed:

- 📁 The entire `~/.gvm/` data directory (all installed Go versions)
- 🔧 The `gvm` binary itself
- 🐚 Every gvm-managed line from your interactive shell profile (`~/.bashrc`, `~/.zshrc`, PowerShell profile, etc.)
- 🔑 The static PATH entry from your login profile (`~/.profile`, `~/.zprofile`) on Linux/macOS
- 🗝️ The gvm entries from the Windows user PATH registry key (`HKCU\Environment`) on Windows

> ⚠️ This operation is **irreversible**. Your installed Go versions will be deleted. Use `gvm upgrade` instead if you just want to update.

---

### 🏁 `gvm completions <shell>`

Prints a shell completion script to stdout.

```sh
# 🐧 Bash
gvm completions bash > ~/.local/share/bash-completion/completions/gvm

# 🐚 Zsh
gvm completions zsh > "${fpath[1]}/_gvm"

# 🐟 Fish
gvm completions fish > ~/.config/fish/completions/gvm.fish

# 🪟 PowerShell
gvm completions powershell >> $PROFILE
```

---

## 🔢 Version Syntax

All commands that accept a version support these forms:

| Input      | Meaning                               |
| ---------- | ------------------------------------- |
| `latest`   | 🆕 Newest stable release              |
| `1.22`     | 🔢 Latest installed patch of Go 1.22  |
| `1.22.4`   | 🎯 Exact version go1.22.4             |
| `go1.22.4` | ✅ Same as `1.22.4` (prefix accepted) |

---

## 📌 Per-project Versions

Place a `.go-version` file in any directory:

```
go1.22.4
```

gvm walks up the directory tree from the current working directory (up to 20 levels) looking for `.go-version`. When found, it takes precedence over the global default.

> 🔗 The file is compatible with other tools such as [goenv](https://github.com/syndbg/goenv) and the VS Code Go extension.

---

## 🐚 Shell Integration

After running `gvm setup`, two things are configured in your shell:

**1. Interactive profile** - the `gvm env` hook, injected once by `gvm setup`:

| Shell         | Profile file                              | Hook                                                            |
| ------------- | ----------------------------------------- | --------------------------------------------------------------- |
| 🐧 Bash       | `~/.bashrc`                               | `eval "$(gvm env --shell bash)"`                                |
| 🐚 Zsh        | `~/.zshrc`                                | `eval "$(gvm env --shell zsh)"`                                 |
| 🐟 Fish       | `~/.config/fish/config.fish`              | `gvm env --shell fish \| source`                                |
| 🪟 PowerShell | `~/Documents/PowerShell/profile.ps1`      | `gvm env --shell powershell \| Out-String \| Invoke-Expression` |

On every new interactive shell session the hook:

1. 🔍 Reads the active version (`.go-version` → global default)
2. ➕ Prepends the version's `bin/` directory to `PATH`
3. 📂 Sets `GOROOT` to the version's root directory

**2. Login profile / registry** - a static PATH entry so GUI apps find Go:

| Platform | Where | What |
| -------- | ----- | ---- |
| 🐧 Linux (bash) | `~/.profile` | `export PATH="$HOME/.gvm/current/bin:$PATH"` |
| 🐚 Linux (zsh) | `~/.zprofile` | `export PATH="$HOME/.gvm/current/bin:$PATH"` |
| 🪟 Windows | `HKCU\Environment` | gvm dir + `~\.gvm\current\bin` added to user PATH |

This login profile entry is what makes `go` visible to VSCode, GoLand, and other GUI editors that launch outside of an interactive shell session.

> 🔇 No daemons, no background processes, no side effects.

---

## ⚙️ Configuration

| Variable  | Default  | Description                        |
| --------- | -------- | ---------------------------------- |
| `GVM_DIR` | `~/.gvm` | 📁 Root directory for all gvm data |

### 📂 Directory layout

```
~/.gvm/
├── version          # active global version (plain text)
├── current -> versions/go1.23.0/   # symlink/junction updated by gvm use
├── versions/
│   ├── go1.22.4/    # extracted Go toolchain
│   │   ├── bin/
│   │   ├── src/
│   │   └── ...
│   └── go1.23.0/
└── tmp/             # download staging area (cleaned after install)
```

The `current` symlink (junction on Windows) always points to the active version. The login profile PATH entry points to `~/.gvm/current/bin`, which means GUI applications always see whichever version was last activated with `gvm use` - no shell restart required.

---

## 🛠️ Building from Source

Requires [Rust](https://rustup.rs) 1.75 or newer. No system dependencies - TLS is handled by [rustls](https://github.com/rustls/rustls) (pure Rust, no OpenSSL needed).

```sh
git clone https://github.com/jhonsferg/gvm.git
cd gvm
cargo build --release
```

The binary is placed at `target/release/gvm` (or `gvm.exe` on Windows).

```sh
# ✅ Run the self-check after building
./target/release/gvm doctor
```

---

## 📦 Release Artifacts

Releases are automated via GitHub Actions. Pushing a version tag triggers cross-compilation for all supported targets:

| Artifact                 | Target                       | Notes            |
| ------------------------ | ---------------------------- | ---------------- |
| `gvm-windows-x86_64.exe` | `x86_64-pc-windows-msvc`     |                  |
| `gvm-linux-x86_64`       | `x86_64-unknown-linux-musl`  | ⚡ static binary |
| `gvm-linux-aarch64`      | `aarch64-unknown-linux-musl` | ⚡ static binary |
| `gvm-darwin-x86_64`      | `x86_64-apple-darwin`        |                  |
| `gvm-darwin-aarch64`     | `aarch64-apple-darwin`       | 🍎 Apple Silicon |

Each release also includes `checksums.txt` with SHA-256 hashes for all artifacts, plus SBOM files in CycloneDX and SPDX formats.

Releases are created automatically: every merge to `main` that passes CI triggers the auto-tag-and-release job, which bumps the version based on conventional commit prefixes (`feat` -> minor, `fix` -> patch) and dispatches the release build.

---

## 📄 License

MIT - see [LICENSE](LICENSE).

---

<div align="center">

Made with 🦀 Rust · Maintained with ❤️

</div>
