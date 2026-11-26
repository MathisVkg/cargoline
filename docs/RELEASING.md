# Making a release

Releases are built by `.github/workflows/release.yml`. It runs `scripts/build-release.ps1`, the same
script you can run locally. A tag push creates a **draft** GitHub Release, which you check and then publish by
hand. Nothing is published automatically, and an existing release is never overwritten.

## Steps

1. **Set the version.** It is stored in `package.json`; `tauri.conf.json` reads it from there.

   ```powershell
   node scripts/set-version.mjs 1.0.0-beta.2
   ```

   This writes `package.json`, `package-lock.json` and `src-tauri/Cargo.toml`, and updates `Cargo.lock`
   (`cargo metadata --offline`). It refuses anything that isn't valid semver: no leading `v`.

2. **Update `CHANGELOG.md`.** Add a `## [1.0.0-beta.2] - <date>` section with a short, user-facing list.
   The release notes are taken from that section, and the build fails if it is missing or empty.

3. **Commit and push**, then run the workflow by hand: GitHub → Actions → Release → **Run workflow** (on
   `main`). This builds without creating a release. When it finishes, download the `cargo-line-<version>`
   artifact (kept for 7 days) and check:
   - the setup exe installs, starts, and Settings → About says `Installed`;
   - the portable zip extracts to one `Cargo-Line-<version>-portable/` folder, and About says `Portable`;
   - `SHA256SUMS.txt` matches (`Get-FileHash <file> -Algorithm SHA256`);
   - `release-notes.md` is the right CHANGELOG section.

4. **Tag and push the tag.** The tag must be `v` + the version, or the workflow fails before building:

   ```powershell
   git tag v1.0.0-beta.2
   git push origin v1.0.0-beta.2
   ```

5. **Check the draft release** (GitHub → Releases): the title, the notes, the three files, and the
   pre-release flag. A version with a suffix such as `-beta.1` is marked as a pre-release automatically. Then
   click **Publish release**.

## If something goes wrong

- **The tag doesn't match:** delete the tag (`git push origin :refs/tags/v<version>`), fix the version,
  then tag again.
- **A release for the tag already exists** (also a draft): the workflow stops. Delete that release on GitHub
  if it should be rebuilt, then re-run the workflow.
- **Cache:** the Tesseract OCR bundle is cached on the hash of `scripts/prepare-tesseract.ps1` and
  `scripts/tesseract/`. Changing either rebuilds it with vcpkg (the slow part).

## Not done yet

- The installer and the exe aren't code-signed, so SmartScreen warns (see the README).
- There is no update check yet. The app already knows whether it is the installed or the portable copy
  (`mode` in `get_about_info`), for when there is one.
