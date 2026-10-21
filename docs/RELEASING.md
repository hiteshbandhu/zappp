# Releasing zappp

Releases are built by GitHub Actions on an Apple Silicon runner, published as
a GitHub release, and installed through the Homebrew tap
[hiteshbandhu/homebrew-tap](https://github.com/hiteshbandhu/homebrew-tap)
(`hiteshbandhu/tap`), which also holds hallmonitor.

## Steps

1. **Bump the version** in `Cargo.toml` and add a `## <version>` section to
   `CHANGELOG.md` (it becomes the release notes).
2. **Commit and tag:**
   ```bash
   git commit -am "Release 0.2.0"
   git tag v0.2.0
   git push origin main v0.2.0
   ```
3. **Wait for the Release workflow** (`.github/workflows/release.yml`). It runs
   the tests, then `scripts/release.sh`, and publishes:
   - `zappp-<version>.dmg` — drag-to-Applications installer
   - `zappp-<version>.zip` — what Homebrew installs
   - `SHA256SUMS`
   - `zappp.rb` — the Homebrew cask, with the zip's checksum filled in
   - `zappp-launch.mp4` — the launch video
4. **Update the tap** with the generated cask:
   ```bash
   gh release download v0.2.0 -p zappp.rb -D /tmp/zappp-cask
   cd "$(brew --repo hiteshbandhu/tap)"
   cp /tmp/zappp-cask/zappp.rb Casks/zappp.rb
   git commit -am "zappp 0.2.0" && git push
   ```
5. **Check the install** on a clean account or Mac:
   ```bash
   brew update && brew upgrade --cask zappp   # or: brew install --cask hiteshbandhu/tap/zappp
   ```

## Building locally

```bash
scripts/release.sh                              # release build → dist/
ZAPPP_RELEASE_PROFILE=debug scripts/release.sh  # quick test of the packaging
```

## Signing

There's no Apple Developer ID yet, so releases are **ad-hoc signed** and not
notarized. Consequences:

- Gatekeeper blocks the downloaded app until the user approves it once
  (right-click → Open). The Homebrew cask removes the quarantine flag instead.
- macOS keys privacy permissions to the signature, which changes every release,
  so users may need to re-grant Input Monitoring and Accessibility after
  upgrading (remove zappp with **−** and allow it again).

With a Developer ID ($99/year Apple Developer Program), sign with
`ZAPPP_SIGN_IDENTITY="Developer ID Application: …"`, add hardened runtime and
`xcrun notarytool submit … --wait` + `xcrun stapler staple` to
`scripts/release.sh`, and drop the cask's quarantine step. Permissions then
survive updates.

## The launch video

`examples/launch_video.rs` renders it from code (SVG frames → resvg → ffmpeg):

```bash
cargo run --example launch_video -- docs/media/zappp-launch.mp4
cargo run --example launch_video -- --still 10.2 frame.png   # one frame
```

The timeline constants at the top of the file control each scene.
