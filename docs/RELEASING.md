# Releasing Orbit

Orbit ships as plain executables (a Windows `.zip` containing `orbit.exe`, a Linux `.tar.gz`).
There is no installer.

## Cutting a release

1. Bump the version in `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml` (they must match).
2. Merge to `main`, then push a tag such as `v1.2.0`.
3. `.github/workflows/release.yml` builds both executables, writes `SHA256SUMS`, signs it
   (`SHA256SUMS.minisig`) and publishes everything to a GitHub release.

## One-time setup: update signing

Orbit installs an update by itself only if `SHA256SUMS` carries a valid
[minisign](https://jedisct1.github.io/minisign/) signature for the public key embedded in the app.

1. Generate a key pair without a password: `minisign -G -W -p orbit.pub -s orbit.key`.
2. Put the whole content of `orbit.key` in the GitHub Actions secret `MINISIGN_SECRET_KEY`.
   Keep a safe offline copy; never commit it.
3. Put the base64 line of `orbit.pub` (the second line) in `src-tauri/update-public-key.txt`,
   replacing the comment-only placeholder, and release a version with it.

Until a key is embedded the app never installs updates itself; it offers the release page.
Releases made without the secret are unsigned and are treated the same way.
If the private key ever leaks, ship a new public key in a release that users install manually.

## How the in-app update works

- The check (startup, then daily; opt-out in Settings, plus "Check now") calls the public GitHub
  API `releases/latest` and sends nothing about the user.
- On "Update" the app downloads the zip, `SHA256SUMS` and `SHA256SUMS.minisig`, verifies the
  signature, compares the zip's SHA-256, takes a `pre-update` database backup, renames
  `orbit.exe` to `orbit.exe.old`, moves the new exe in and starts it. The new version deletes
  `orbit.exe.old` at startup. Any failure restores the old exe.
- If the folder is not writable the app offers the release page instead.
- Windows only for now.

## Signature format

Orbit accepts only modern (pre-hashed) minisign signatures: the second line of `SHA256SUMS.minisig` starts with `RU` (legacy signatures start with `RW` and are rejected). minisign 0.10 and newer sign pre-hashed by default; do not pass `-l`.
