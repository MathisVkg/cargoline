# Cargo Line

Cargo Line is an offline-first hauling contract manager for Star Citizen, built as a Windows desktop app.
It needs no account and no server, and it does not require game log monitoring.

> Unofficial fan project. Not affiliated with or endorsed by Cloud Imperium Games.

## Download and install

Get the latest version from the [Releases page](https://github.com/MathisVkg/cargoline/releases).
Each release has two downloads. Pick one:

| File | Choose it if |
|---|---|
| `Cargo-Line_<version>_x64-setup.exe` (**recommended**) | You want a normal install: a Start menu entry, no admin rights needed (it installs for your user only, in `%LOCALAPPDATA%\Cargo Line`). It updates itself from inside the app (Settings → Updates). |
| `Cargo-Line_<version>_x64-portable.zip` | You don't want to install anything. Extract the zip and run `cargo-line.exe`. Everything stays in its own folder, so you can keep it on a USB stick. Updates are done by hand. |

**"Windows protected your PC".** Cargo Line isn't code-signed yet, so Windows SmartScreen doesn't know it and
shows this blue warning the first time. Click **More info**, then **Run anyway**.

**Checking the download (optional).** Each release has a `SHA256SUMS.txt`. In PowerShell, run
`Get-FileHash <file> -Algorithm SHA256` and compare the result with the line for that file (case doesn't matter).

**Requirements**

- Windows 10 or 11, x64.
- Microsoft Edge WebView2 Runtime. Windows 11 already has it. If it's missing, the installer downloads and
  installs it for you. For the portable version, get it from
  [Microsoft's WebView2 page](https://developer.microsoft.com/microsoft-edge/webview2/) ("Evergreen Bootstrapper").
- Star Citizen in **borderless or windowed** mode, 16:9 or wider (16:10 works too), for reading contracts from a
  screenshot.

**Where your data lives**

- **Installed:** your contracts, history, screenshots and settings are in
  `%APPDATA%\io.github.mathisvkg.sccargoapp`. The WebView2 profile and temporary OCR files are in
  `%LOCALAPPDATA%\io.github.mathisvkg.sccargoapp`. Uninstalling keeps your data, unless you tick
  **Delete the application data** in the uninstaller. That box deletes those two folders (and the installer's own
  registry entries) and nothing else. A portable copy is never touched by the uninstaller.
- **Portable:** everything is in the `data\` folder next to `cargo-line.exe`. Nothing is written to `%APPDATA%`
  or `%LOCALAPPDATA%`. To move to another PC, close the app and copy the whole folder. To switch the portable
  copy to the normal app data folder, delete `portable.txt`.
- **Both:** Settings → Data → Export backup makes a single backup file that either version can restore.

Settings → About shows the version and which one you're running (`Installed` or `Portable`).

## How to use Cargo Line

### First-time setup

1. **Pick your ship** with the ship selector in the top bar. **Favourites** come first. If your ship isn't
   listed, choose **Add custom ship…** and give it a name and its cargo in SCU.
2. **Optional, recommended: connect the Game.log.** In **Settings → Game**, turn on **Game.log monitoring**,
   then click **Detect** (or **Browse…** to pick `Game.log` yourself). Cargo Line then notices the contracts you
   accept in the game. Turn on **Complete contracts finished in game** to have them completed for you, with
   **Ask me first** or **Automatically** (you can undo for 30 seconds).
3. **Screenshot reading:** run Star Citizen in **borderless** or **windowed** mode, 16:9 or wider. In
   **Settings → OCR Capture**, set the **Capture shortcut** (and check the capture source and delay).

### Adding a contract

- **From a screenshot:** open the contract in the game's mobiGlas, then press your capture shortcut, or click
  **Add Contract** (it takes a screenshot when it opens). The form fills in from the screenshot.
- **By hand:** click **Add Contract** and type the fields. Locations and commodities are searchable lists, and
  you can also type any name.
- **Fields marked "Check this value" or "Couldn't read this"** are the ones the reading wasn't sure of: look at
  them before you click **Add Contract** to save.

### The pages

- **Contracts:** your active contracts, **By Location** or **By Contract**. Tick **Pick Up** and **Turn In** as
  you go; the menu on each contract has **Edit contract**, **Mark complete** and **Abandon contract**.
- **Cargo:** what to pick up (**Pickup**) and where it goes (**Destination**), and the **Cargo arrangement** box
  view scaled to your ship.
- **History:** your finished contracts grouped in runs, with earnings and aUEC per hour. **Restore to active**
  brings a contract back; **Clear History** erases it.
- **Settings:** capture, Game.log and completing contracts from the game, updates, backup and restore
  (**Settings → Data**), and **About**.

### Tips and common problems

- **"Contract screen not recognised":** open the contract's details in the mobiGlas, make sure the game is in
  borderless or windowed mode and not covered by another window, then **Recapture**.
- **The Game.log isn't found:** start the game once, then click **Detect** again, or **Browse…** to
  `…\StarCitizen\LIVE\Game.log`.
- **Your data:** **Settings → Data → Open data folder** shows where it is. **Export backup…** saves everything
  (except screenshots) to one file; **Restore backup…** brings it back.
- **The welcome guide:** **Settings → About → Show the welcome guide** opens it again.

## Updates

- **What is checked:** the list of releases on this repository's GitHub Releases page. A beta build is offered
  newer betas and stable releases; a stable build only newer stable releases.
- **How often:** about 10 seconds after Cargo Line starts, at most once every 24 hours, plus whenever you click
  **Check now** in Settings → Updates. A failed automatic check is silent; it never slows the start.
- **Turning it off:** Settings → Updates → **Check for updates automatically**. Check now still works.
- **When there is a new version:** a quiet **Update available** line appears in the top bar (never a pop-up).
  **Skip this version** hides it until an even newer version comes out.
  - **Installed version:** **Download and install** downloads the installer and `SHA256SUMS.txt`, checks the
    installer's size and SHA-256, then asks before it closes Cargo Line, installs over the existing copy (no admin
    prompt, your shortcuts stay as they are) and opens it again. Your data folder isn't touched.
  - **Portable version** (and development builds): **Open download page**. Download the new zip and copy your
    `data` folder into it.
- **What the check protects against:** a damaged or incomplete download (the file is deleted and nothing is
  installed). It doesn't protect against a compromised GitHub account; code signing would cover that later.

**Network:** the update check is the only network access Cargo Line makes. It contacts GitHub only
(`api.github.com`, `github.com` release downloads and GitHub's download servers), sends nothing but standard
request headers (`User-Agent: Cargo-Line/<version>`, no account, no token, no data of yours), and can be turned off.

## Current status

**Beta.** What the app does:

| Area | What it does |
| --- | --- |
| Contracts | Add by hand or read from a screenshot (built-in OCR). Grouped by location or by contract. Edit, complete or abandon contracts. Pick Up / Turn In ticks. |
| Game.log | Optional. Detects the log, notices contracts accepted and completed in the game. |
| Ships | Catalog from the Star Citizen Wiki, favourites, custom ships, selector in the top bar. |
| Cargo | Pickup and destination summary, isometric box view. |
| History | Runs, earnings, aUEC per hour. Restore or clear. |
| Settings | Capture, Game.log, updates, data folder, backup and restore, About. |
| Updates | Checks GitHub for a newer version. The installed version updates itself in one click. |

See [`CHANGELOG.md`](CHANGELOG.md) for each version's changes.

## Credits and licence

- Cargo Line is free software under the GNU GPL v3.0 (`GPL-3.0-only`); see [`LICENSE`](LICENSE).
- Bundled components keep their own licences, including the OCR engine Tesseract (Apache-2.0); see
  [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).
- Ship, location and commodity data from the [Star Citizen Wiki API](https://api.star-citizen.wiki)
  (api.star-citizen.wiki), used under its terms: non-commercial, with attribution.
- Unofficial fan project, not affiliated with or endorsed by Cloud Imperium Games. Star Citizen® is a
  registered trademark of Cloud Imperium Rights LLC.
