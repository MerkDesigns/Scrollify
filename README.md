![Scrollify](Assets/banner.png)

Scrollify is a small Windows tray app: hold a key and scroll the mouse wheel to change the volume of
Spotify (or any other app you choose) in the Windows volume mixer.

## Behavior

- `Alt + wheel up` raises the volume, `Alt + wheel down` lowers it. The key can be changed to Ctrl,
  Shift, Win, Mouse 4 or Mouse 5.
- Every app in the **Apps** list is changed together, on every active output device. The default
  list is `Spotify.exe`. Use **+ Add** to pick from the apps that are playing audio right now.
- Each notch changes the volume by the **Volume step** (0.5 to 12 dB).
- An optional popup at the bottom of the screen shows the new volume and fades out.
- By default, the scroll still reaches the window under the mouse. Turn on **Block scroll in apps**
  to stop that while the key is held.
- Left-click the tray icon to turn Scrollify on or off. Right-click it to open the settings panel.
  The panel closes when you click elsewhere, press `Esc` or right-click it.
- Only one copy of Scrollify runs at a time.

## Settings panel

- **ON / OFF**: turns Scrollify on or off.
- **Start with Windows**: starts Scrollify when you sign in.
- **Volume popup**: shows or hides the volume popup.
- **Block scroll in apps** (off by default): while you hold the key, the wheel only changes the
  volume and doesn't also scroll the app under the mouse. This uses a mouse hook, so see
  [Anti-cheat safety](#anti-cheat-safety) before turning it on.
- **Volume step**: how much each notch changes the volume (0.5 to 12 dB).
- **Hold while scrolling**: the key or mouse button to hold (Alt, Ctrl, Shift, Win, Mouse 4, Mouse 5).
- **Apps**: the apps whose volume changes. Click **×** to remove one, or **+ Add** to pick from the
  apps that are playing audio right now.
- **Quit**: closes Scrollify.

## Anti-cheat safety

Scrollify only uses ordinary, documented Windows APIs that many desktop apps use:

- **No input hooks by default.** The wheel is read with Raw Input (`RegisterRawInputDevices`), which
  receives a copy of mouse events. It cannot block, change or create input.
- **No simulated input.** Nothing calls `SendInput` or similar APIs.
- **No process access.** Scrollify never opens a handle to another process. It identifies apps from
  their audio session IDs, so it never touches a game's process.
- **No injection or overlays inside games.** The volume popup is a normal top-most window. It doesn't
  appear over exclusive-fullscreen games.
- No drivers, no admin rights and no global hotkeys.

Volume changes go through the Windows Core Audio session API, which is the same one the Windows volume
mixer uses.

### Block scroll in apps

Raw Input can't stop the scroll from reaching other apps, so **Block scroll in apps** needs a
low-level mouse hook (`WH_MOUSE_LL`). Some anti-cheats look for this kind of hook, so the setting is
off by default and its footprint is kept small:

- The hook is installed only while the key is held and removed as soon as it's released.
- It only swallows vertical wheel events while the key is held. Everything else passes through.
- To notice when the key goes down, Scrollify also reads keyboard Raw Input while this setting is on.
  It only checks the modifier key and doesn't store any keys.

Leave the setting off when you play games with strict anti-cheat.

## Requirements

- Windows 10 or 11

## Build

```powershell
cargo build --release
```

The portable executable is created at `target\release\scrollify.exe`. Icons and images are built into the executable.

## Shortcuts

On launch, Scrollify creates or refreshes `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Scrollify.lnk`
so it can be found with Windows search and pinned to Start.

**Start with Windows** manages `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\Scrollify.lnk`.
No registry changes, no administrator rights.

## Configuration

Settings are stored in `%APPDATA%\Scrollify\config.json`:

```json
{
  "enabled": true,
  "volume_step_db": 1.0,
  "modifier": "alt",
  "targets": ["Spotify.exe"],
  "show_overlay": true,
  "block_scroll": false
}
```

`modifier` is one of `alt`, `ctrl`, `shift`, `win`, `mouse4` or `mouse5`. Missing fields get their
defaults.

## Development

```powershell
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
```
