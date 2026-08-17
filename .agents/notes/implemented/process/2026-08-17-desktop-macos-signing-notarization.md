# Agent Note: desktop macOS release signing, notarization, and verification

Status: implemented

English | [中文](2026-08-17-desktop-macos-signing-notarization.zh.md)

## Problem

The desktop release pipeline ([`.github/workflows/desktop-release.yml`](../../../../.github/workflows/desktop-release.yml) plus the shared [`.github/actions/build-desktop`](../../../../.github/actions/build-desktop/action.yml) composite action) builds every matrix job with an explicit `tauri build --target <triple>`. Two hand-written steps in the composite action still read the native-output bundle paths `target/release/bundle/...`, which only exist for builds without `--target`; cross-target bundles land under `target/<triple>/release/bundle/`. The macOS notarization step therefore globbed a directory that did not exist and could never submit the DMG, and the CI artifact-upload step uploaded nothing. On top of that, the macOS chain had no verification at all: a job could turn green while shipping a DMG that Gatekeeper rejects, and a misconfigured `APPLE_SIGNING_IDENTITY` secret would fail the tauri CLI's certificate-identity match without a clear diagnosis.

## Decision

The composite action owns one signing/notarization chain per macOS matrix job, with every hand-written path derived from the `target` input:

1. **Certificate import.** Decode `APPLE_CERTIFICATE` into a dedicated `build.keychain`, make it the default, unlock it, extend the lock timeout to 3600 s (`security set-keychain-settings`), import with the codesign partition list, and fail with a named `::error::` when any required secret is missing. The step then derives the signing identity from the imported certificate itself (`security find-identity -v -p codesigning`, the `Developer ID Application` entry) and writes it back to `APPLE_SIGNING_IDENTITY` through `$GITHUB_ENV`, so signing always uses an identity byte-identical to the certificate. This sidesteps the tauri CLI's refusal to sign when its own import of `APPLE_CERTIFICATE` does not contain the configured identity.
2. **Resource signing.** Every Mach-O under `src-tauri/resources` (the staged Node runtime and native addons) is signed before the bundle is built with `codesign --force --options runtime --timestamp`, then verified with `codesign --verify --strict`. This step is required because the tauri bundler signs app binaries, frameworks, and sidecars but not arbitrary `Resources` files, and notarization rejects unsigned executable code anywhere in the bundle.
3. **App signing and notarization.** `tauri build` (invoked by `tauri-apps/tauri-action@v1`) signs the app bundle with hardened runtime — the tauri default — and, because the workflow job now exports `APPLE_ID`/`APPLE_PASSWORD`/`APPLE_TEAM_ID` (the env names `tauri-bundler`'s `notarize_auth` reads), notarizes and staples the app itself. Those three env vars are also passed explicitly in the tauri-action step's `env:` block rather than relying on job-env inheritance.
4. **DMG notarization and verification.** The final step locates the bundles under `target/${{ inputs.target }}/release/bundle`, submits the DMG with `xcrun notarytool --wait --timeout 20m`, staples it, and then asserts the whole chain: `stapler validate` on both the DMG and the app, `codesign --verify --deep --strict` on the app, and `spctl --assess --type exec` on the app. Only a DMG that passes all checks replaces the one tauri-action uploaded (`gh release upload --clobber`). A green job now proves the artifact Gatekeeper will accept.

The previous `NOTARY_ID`/`NOTARY_PASSWORD`/`NOTARY_TEAM_ID` aliases were removed in favor of the `APPLE_*` names, which both the workflow step and the tauri CLI consume; the release workflow's job `env:` block maps the secrets once. The CI (non-publish) macOS job keeps `apple-signing: false` and stays unsigned by design, and its artifact upload now reads the same per-target path.

## Alternatives considered

**Let tauri-action or the tauri CLI handle notarization end to end.** Rejected: tauri-action@v1 does no signing/notarization of its own, and the CLI notarizes only the app bundle, never the DMG. A DMG-only distribution (the macOS download path) would then be notarized nowhere, so the explicit `notarytool` submission on the DMG is not redundant.

**Sign the staged resources after bundling.** Rejected: at that point the resources are already copied and sealed into the app bundle; post-sealing edits invalidate the bundle signature and force a rebuild of the whole chain.

**Skip the post-staple verification to save job minutes.** Rejected: the verification assertions are what turn a successful notarization into evidence the artifact passes Gatekeeper; without them a broken chain (e.g. a CLI upgrade that stops stapling) ships silently green.

**Trust the `APPLE_SIGNING_IDENTITY` secret to match the certificate.** Rejected: the tauri CLI compares its imported certificate's identity against the configured value and errors when the strings disagree; deriving the value from the imported certificate removes an entire failure class that only ever surfaces after an hour-long build.

## Consequences

Each macOS release job now performs two notarization submissions: the app inside `tauri build` and the DMG in the workflow step, adding roughly the notarization turnaround (typically 5–20 minutes) to each mac job. That buys notarized and stapled products on both distribution paths — the direct DMG download and the auto-updater `.app.tar.gz`, whose inner app carries the stapled ticket — plus verification assertions that fail the job when any link of the chain breaks. The manual resource-signing step also depends on `file` being present on the runner (it is, on GitHub-hosted macOS images).
