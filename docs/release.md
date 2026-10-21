# Release

[日本語](ja/release.md)

How releases are made, and signing, notarization and build provenance attestation.

## Tags and snapshot

- Pushing a `v<number>` tag (such as `v0.2.0`) makes GitHub Actions publish to the Release the app zip, the GTK4 app's tar.gz files (Linux x86_64,
  aarch64), plus engine-only
  tar.gz files (macOS arm64, Linux x86_64, Linux aarch64) and zip files (Windows x86_64, aarch64). The tag must match
  `workspace.package.version` in `Cargo.toml`, CI must have passed on the tagged commit, and `CHANGELOG.md` must have a
  section for the version with a short summary paragraph under its heading (the summary becomes the Release
  notes, with a link to the whole CHANGELOG).
- Everything is built by `.github/workflows/build.yml`: each engine on a machine of its own OS and CPU, run there once
  (`scripts/check-engine.sh`) before it is shipped; the GTK4 app on Ubuntu 24.04 for each Linux CPU with the Linux engines put in
  for SSH (`scripts/package-gtk4.sh`); the app compiled on macOS in parallel (`scripts/compile-app.sh`); then assembled and signed (`scripts/package-app.sh`). Locally, `scripts/build-app.sh`
  does the same in one go.
- The VSCode extension uses `vscode-v*` tags and publishes a .vsix with the engine bundled (see
  [VSCode extension](#vscode-extension) below).
- Whenever CI (`ci.yml`) passes on main, a pre-release named `snapshot` is recreated from that commit (app, engine,
  extension; the version is `<next patch>-snapshot.<commit count>+<sha>`, or `<version>-snapshot.…` between bumping
  the version and tagging it, so a snapshot always sorts below the stable release after it). The Windows engines are built alongside the
  macOS build, and a last job bundles and publishes everything.
- Changes go in [CHANGELOG.md](../CHANGELOG.md) (one file, with sections for the app and engine and for the VSCode
  extension). Before tagging, rename that section's `Unreleased` to the version and date, write a short summary under it, and commit it.
- After a release, check it on a real Windows machine with `scripts/check-windows.ps1` (see
  [windows.md](windows.md#checking-a-release-on-the-machine)).

## VSCode extension

| Channel | When it is built | Version |
| --- | --- | --- |
| stable | On pushing a `vscode-v*` tag (separate from the app and engine's `v*`) | `0.0.2` |
| snapshot | Whenever CI passes on main (put on the same `snapshot` pre-release as the app and engine) | `0.0.3-snapshot.<number of apps/vscode commits since the last release>+<sha>` |

A snapshot is treated as a pre-release of the next patch, so installing a stable release is judged newer than the snapshot.

Steps to cut a stable release:

1. In the "VSCode extension" section of [CHANGELOG.md](../CHANGELOG.md), change `Unreleased` to a heading with the version and date being released, write a short summary under it, and commit it
2. Include `npm version patch --no-git-tag-version` in the same commit as 1.
3. Push to main, and once CI has passed: `git tag vscode-v<version> && git push origin vscode-v<version>` (the release
   checks that CI passed on the tagged commit and takes the notes from that CHANGELOG section)

## Licenses

The app (`Contents/Resources`), every engine archive and the `.vsix` include `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`
and `THIRD-PARTY-NOTICES.md`, the licenses of the dependencies built into them (generated with cargo about from
`about.toml`; musl libc is added by hand in `about.hbs`). CI fails if a dependency uses a license not allowed in
`deny.toml`.

## Build provenance attestation

Files in a Release carry an attestation (artifact attestations) of which commit of this repository and which CI run
built them (for releases after v0.1.0).

```sh
gh attestation verify Amon-Hen-0.2.0.zip --repo fukusaka/amonhen
```

## Signing and notarization (macOS)

If the `release` environment's secrets hold a Developer ID Application certificate and an App Store Connect API key,
the app and the macOS engine are signed with Developer ID and submitted to Apple for notarization (the result is stapled
to the app). Even when downloaded with a browser, it opens as is. Only stable releases are notarized; snapshots are
signed but not notarized (they are installed with `scripts/install-app.sh`, which attaches no quarantine mark).

```sh
base64 -i DeveloperID.p12 | gh secret set AMONHEN_CERT_P12 --env release
gh secret set AMONHEN_CERT_PASSWORD --env release
gh secret set AMONHEN_CERT_NAME --env release --body "Developer ID Application: Name (Team ID)"
base64 -i AuthKey_XXXXXXXXXX.p8 | gh secret set AMONHEN_NOTARY_KEY --env release
gh secret set AMONHEN_NOTARY_KEY_ID --env release --body "<Key ID>"
gh secret set AMONHEN_NOTARY_ISSUER --env release --body "<Issuer ID>"
```

Recreate the Developer ID certificate before it expires (currently about 1 year) and replace `AMONHEN_CERT_P12` and
`AMONHEN_CERT_PASSWORD`. Without the secrets, builds are ad-hoc signed. Locally, passing a certificate name in
`AMONHEN_SIGN_IDENTITY` signs with that certificate. The Windows and Linux engines are not signed (downloading on
Windows with a browser triggers a SmartScreen warning; the one bundled with the VSCode extension runs without a warning).
