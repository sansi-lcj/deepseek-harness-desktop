# @deepseek-ai/dsh-desktop

Desktop shell for DeepSeek Harness: a [Tauri 2](https://v2.tauri.app) window that boots the dsh web server and hosts the browser UI (the `apps/web` frontend).

## Stack

- Shell: Tauri 2 (Rust, edition 2024) + tauri-plugin-single-instance
- Splash surface: Vite 6 + TypeScript, `@tauri-apps/api` event bridge
- Product UI: unchanged `apps/web` (Vite + React), served by the embedded `dsh web` server

## Architecture

The shell owns the server as a managed child process (bundled into the app; nothing needs to be preinstalled):

1. Resolve a Node.js runtime and the server entry: explicit `DSH_DESKTOP_NODE` / `DSH_DESKTOP_SERVER_BIN` / `DSH_DESKTOP_REPO_ROOT` first (checkout mode), then the bundled app resources (`resources/node/node` and `resources/server/node_modules/@deepseek-ai/dsh/lib/bin.js`), then PATH and mise/nvm/Homebrew locations.
2. Spawn `node apps/cli/lib/bin.js web --host 127.0.0.1 --port 0` and watch stdout for the readiness line `dsh web: http://127.0.0.1:<port>` — the web bundle prints it as a supervisor signal.
3. Poll the TCP port until it accepts connections, then navigate the main window from the bundled splash page to the live URL.
4. On quit, signal the server (SIGTERM, then SIGKILL after 3 s) before the shell exits. On unexpected exit the shell restarts the server (up to 3 times), otherwise it rebuilds the splash with the failure detail.

The splash renders the `server-status` event (`starting` / `ready` / `exited`). External http(s) links open in the default browser; a second launch focuses the existing window. The bundled server runs with its working directory under the per-user App Support directory, so replacing the `.app` (e.g. during an update) never invalidates a running process.

## Auto-update

On startup the shell checks the GitHub Releases endpoint configured in `tauri.conf.json` (`plugins.updater.endpoints`). When a newer version exists it prompts with a native dialog, downloads the signed updater bundle, installs it, and restarts.

## Releases (CI)

Pushing a `v*` tag runs `.github/workflows/desktop-release.yml`: tauri-action builds on macOS, Windows, and Linux and publishes the GitHub Release assets plus the per-platform `latest.json` that the auto-updater reads. On macOS the shared composite action additionally runs the full signing/notarization chain:

1. Import the Developer ID Application certificate into a dedicated keychain, unlock it for the job lifetime, and derive `APPLE_SIGNING_IDENTITY` from the imported certificate so signing always uses an identity that matches the certificate.
2. Sign every Mach-O binary staged under `src-tauri/resources` (the bundled Node runtime and native addons) with hardened runtime and a secure timestamp, then verify each signature.
3. `tauri build` (inside tauri-action) signs the app bundle with hardened runtime, notarizes it, and staples the ticket into the app.
4. Notarize the DMG with `notarytool`, staple it, then verify the whole chain: `stapler validate` on both the DMG and the app, `codesign --verify --deep --strict` on the app, and `spctl --assess --type exec` on the app. The verified DMG replaces the unverified upload on the release.

Required secrets: `TAURI_SIGNING_PRIVATE_KEY`, `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`, `KEYCHAIN_PASSWORD`.

## Prerequisites

- Node.js (^22.19 or >=24), pnpm
- Rust stable (via mise or rustup)
- macOS: Xcode Command Line Tools; Windows: WebView2 (preinstalled); Linux: webkit2gtk-4.1

## Quick start

From the repository root, with the server already built:

```sh
pnpm install
pnpm run build
pnpm desktop:dev      # run the desktop shell (debug)
pnpm desktop:build    # bundle the macOS .app
pnpm desktop:smoke    # headless smoke: load the server page, report, exit
```

Artifacts land in `apps/desktop/src-tauri/target/release/bundle/macos/`: `DeepSeek Harness.app` and `DeepSeek Harness_0.1.0-rc.6_aarch64.dmg`. Double-click to run; the app carries its own Node runtime, dsh server, and Web UI, so the target machine needs nothing preinstalled.

## Environment overrides

| Variable | Meaning |
| --- | --- |
| `DSH_DESKTOP_NODE` | Node.js executable used to run the server |
| `DSH_DESKTOP_SERVER_BIN` | Path to the built `bin.js` |
| `DSH_DESKTOP_REPO_ROOT` | Repository checkout holding the built server |
| `DSH_DESKTOP_SMOKE` | `1` runs the load-and-exit smoke instead of staying open |

## Known Limitations and Deferred Work

- The bundled server is the npm-published `@deepseek-ai/dsh@0.1.0-rc.6` (override with `DSH_DESKTOP_SERVER_VERSION`); dev mode (`pnpm desktop:dev`) runs this checkout's build instead.
- Locally built bundles are unsigned; CI release builds are signed and notarized (see above).
- One window; multi-server setups are out of scope.
- npm blocks the `dsh-subprocess-local` postinstall, so `scripts/build-resources.mjs` restores the node-pty spawn-helper executable bit; a single-file server executable (see `scripts/build-exe-for-python-sdk.ts`) is the intended size-reduction follow-up.
