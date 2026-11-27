# Changelog

All notable changes to Cargo Line are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [1.0.0-beta.3] - 2026-11-27

### Added

- **Welcome guide on first launch:** a short guide to getting started opens once (existing users see it once
  after updating). Settings → About → Show the welcome guide opens it again.
- **Help in the README:** a "How to use Cargo Line" section for players: first-time setup, adding contracts,
  the pages, and tips for common problems.

## [1.0.0-beta.3] - 2026-11-26

### Added

- **Updates:** update check, and one-click updates for the installed version. Cargo Line looks for
  a newer version on GitHub shortly after it starts (at most once a day; you can turn it off in
  Settings → Updates) and shows a quiet "Update available" line in the top bar. The installed
  version downloads the installer, checks it, closes, installs and opens again; the portable
  version links to the download page.

## [1.0.0-beta.1] - 2026-11-26

The first public beta. Windows 10/11 (x64), offline, no account. The app isn't code-signed yet,
so Windows SmartScreen shows a warning the first time (More info → Run anyway).

### Added

- **Contracts:** add hauling contracts by hand or by reading the in-game contract screen from a
  screenshot (keyboard shortcut, text recognition built in, nothing leaves your PC). Searchable
  location and commodity fields, grouping by location or by contract, edit, complete or abandon
  contracts one by one or all at once, and Pick Up / Turn In ticks.
- **Game.log:** optional monitoring of the game's log. Cargo Line finds the log for you, notices
  contracts you accept in the game (and can open Add Contract with a screenshot right away), and
  completes contracts in the app when you finish them in the game (or asks first, with Undo).
- **Ships:** the ship list from the Star Citizen Wiki, favourites, your own custom ships, and a
  ship selector in the top bar.
- **Cargo:** what to pick up and where to deliver it, and an isometric view of the boxes to load.
- **History:** finished contracts grouped into runs, with earnings and aUEC per hour; restore a
  contract to active or clear the history.
- **Settings:** capture and Game.log options, the data folder, backup export and restore (with a
  safety copy of your current data), licence, third-party notices and data credits.
- **Downloads:** an installer (current user, no admin rights) and a portable zip that keeps its
  data in its own folder.
