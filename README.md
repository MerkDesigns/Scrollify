![Scrollify](Assets/banner.png)

Scrollify is a small Windows tray app that adjusts Spotify's Windows volume-mixer session with `Alt + mouse wheel`.

## Behavior

- `Alt + wheel up` increases Spotify's volume.
- `Alt + wheel down` decreases Spotify's volume.
- Left-clicking the tray icon enables or disables Scrollify.
- Right-clicking the tray icon opens or closes the settings panel.
- Mouse input always continues to the active application.

Scrollify changes only `Spotify.exe` audio sessions. It does not inspect or modify other processes, switch windows, or generate input.

## Requirements

- Windows 10 or 11
- Spotify for Windows
- Microsoft Edge WebView2 Runtime

## Build

```powershell
cargo build --release
```

The portable executable is created at:

```text
target\release\scrollify.exe
```

Icons and panel assets are embedded in the executable.

## Shortcuts

On launch, Scrollify creates or refreshes this per-user Start Menu shortcut:

```text
%APPDATA%\Microsoft\Windows\Start Menu\Programs\Scrollify.lnk
```

This makes Scrollify searchable in Windows and allows it to be pinned to Start.

The **Start with Windows** setting manages:

```text
%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\Scrollify.lnk
```

No administrator privileges are required.

## Configuration

Settings are stored in:

```text
%APPDATA%\Scrollify\config.json
```

## Development

```powershell
cargo fmt
cargo clippy -- -D warnings
```
