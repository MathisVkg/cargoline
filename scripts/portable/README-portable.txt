Cargo Line - portable version
=============================

Run it
  Extract the whole zip to a normal folder first (right-click the zip, Extract All).
  Then start cargo-line.exe. Nothing is installed.
  Windows may show "Windows protected your PC" because the app isn't code-signed yet:
  click More info, then Run anyway.

Where the data lives
  Everything Cargo Line saves (contracts, history, screenshots, settings, the WebView2
  profile) is in the data\ folder next to cargo-line.exe. Nothing is written to %APPDATA%.
  portable.txt is what turns this on; without it the app uses %APPDATA% like the installer.

Moving to another PC
  Close Cargo Line, then copy the whole folder, data\ included. You can also use
  Settings > Data > Export backup and restore it on the other PC.

Updates
  The portable version doesn't update itself. Download the new zip from
  https://github.com/MathisVkg/cargoline/releases, extract it, and copy your data\ folder
  into the new folder (or restore a backup).

Requirements
  Windows 10 or 11 (x64) with the Microsoft Edge WebView2 Runtime. Windows 11 already has it;
  otherwise get it from https://developer.microsoft.com/microsoft-edge/webview2/

Cargo Line is free software under the GNU GPL v3.0 (see LICENSE). Third-party components are
listed in THIRD_PARTY_NOTICES.md. Unofficial fan project, not affiliated with Cloud Imperium Games.
