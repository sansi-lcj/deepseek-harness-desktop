# DeepSeek Harness Desktop

English | [中文](README.zh.md)

The desktop distribution of [DeepSeek Harness](https://github.com/deepseek-ai/DeepSeek-Harness): the official Web GUI wrapped in a self-contained Tauri 2 desktop app.

Double-click to run. The app bundles its own Node.js runtime, the dsh server, and the Web UI — nothing needs to be installed on the target machine.

## Download

Get the latest builds from [GitHub Releases](https://github.com/sansi-lcj/deepseek-harness-desktop/releases):

| Platform | Package |
| --- | --- |
| macOS (Intel + Apple Silicon) | `*_aarch64.dmg` / `*_x64.dmg` — signed and notarized |
| Windows (x64; arm64 deferred) | `*_x64-setup.exe` |
| Linux (x86_64 + arm64) | `*_amd64.deb` / `*_arm64.deb` |

## Features

- Self-contained: bundled Node runtime + dsh server + Web UI; no prerequisites
- Auto-update: checks GitHub Releases on startup, prompts, installs, and restarts
- Resilient: server restarts up to 3 times on unexpected exit; the runtime lives in App Support, so replacing the bundle never breaks a running instance
- Desktop behavior: single instance, native window, external links open in the default browser
- Cross-platform CI: pushing a `desktop-v*` tag builds, signs (and notarizes on macOS), and publishes all three platforms

## Stack

- Shell: Tauri 2 + Rust (edition 2024)
- Splash surface: Vite 6 + TypeScript
- Product UI: the official DeepSeek Harness Web GUI (Vite + React), served by the embedded `dsh web` server

## Development

```sh
git clone git@github.com:sansi-lcj/deepseek-harness-desktop.git
cd deepseek-harness-desktop
pnpm install
pnpm run build
pnpm desktop:dev      # run the desktop shell (debug)
pnpm desktop:build    # bundle locally
pnpm desktop:smoke    # headless smoke test
```

Prerequisites: Node.js (^22.19 or >=24), pnpm, Rust stable; macOS needs Xcode Command Line Tools.

See [apps/desktop/README.md](apps/desktop/README.md) for the shell architecture, environment overrides, and the release pipeline.

## Releases and auto-update

Push a `desktop-v*` tag and `.github/workflows/desktop-release.yml` builds on macOS, Windows, and Linux, then publishes the GitHub Release with per-platform assets and a merged `latest.json` that the in-app updater reads. Required secrets: `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`, `KEYCHAIN_PASSWORD`.

## Upstream

This repository is a fork of [deepseek-ai/DeepSeek-Harness](https://github.com/deepseek-ai/DeepSeek-Harness) whose default branch is `desktop-release`. `.github/workflows/sync-upstream.yml` merges upstream master into `desktop-release` daily (or on manual dispatch) and rebuilds the app; the fork adds `apps/desktop/`. The bundled server is the npm-published `@deepseek-ai/dsh` release (override with `DSH_DESKTOP_SERVER_VERSION`).

## License

[MIT](LICENSE)

Third-party dependencies and their licenses are disclosed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
