# Release

[日本語](ja/release.md)

How releases are made, and signing, notarization and build provenance attestation.

## Tags and snapshot

- Pushing a `v<number>` tag (such as `v0.1.1`) makes GitHub Actions publish to the Release the app zip, plus engine-only
  tar.gz files (macOS arm64, Linux x86_64, Linux aarch64) and zip files (Windows x86_64, aarch64). The tag must match
  `workspace.package.version` in `Cargo.toml`.
- The VSCode extension uses `vscode-v*` tags and publishes a .vsix with the engine bundled (see
  [VSCode extension](#vscode-extension) below).
- Whenever CI (`ci.yml`) passes on main, a pre-release named `snapshot` is recreated from that commit (app, engine,
  extension; the version is `<next patch>-snapshot.<commit count>+<sha>`). The Windows engines are built alongside the
  macOS build, and a last job bundles and publishes everything.
- Changes go in [CHANGELOG.md](../CHANGELOG.md) (one file, with sections for the app and engine and for the VSCode
  extension). Before tagging, rename that section's `Unreleased` to the version and date, and commit it.

## VSCode extension

| Channel | When it is built | Version |
| --- | --- | --- |
| stable | On pushing a `vscode-v*` tag (separate from the app and engine's `v*`) | `0.0.2` |
| snapshot | Whenever CI passes on main (put on the same `snapshot` pre-release as the app and engine) | `0.0.3-snapshot.<number of apps/vscode commits since the last release>+<sha>` |

A snapshot is treated as a pre-release of the next patch, so installing a stable release is judged newer than the snapshot.

Steps to cut a stable release:

1. In the "VSCode extension" section of [CHANGELOG.md](../CHANGELOG.md), change `Unreleased` to a heading with the version and date being released, and commit it
2. Include `npm version patch --no-git-tag-version` in the same commit as 1.
3. `git tag vscode-v<version> && git push origin main vscode-v<version>`

## Build provenance attestation

Files in a Release carry an attestation (artifact attestations) of which commit of this repository and which CI run
built them (for releases after v0.1.0).

```sh
gh attestation verify Amon-Hen-0.1.1.zip --repo fukusaka/amonhen
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
