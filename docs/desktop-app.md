# OVC Desktop App

OVC Desktop provides the complete OVC interface as a native application for macOS, Windows, and Linux. The application includes its local API service and the compiled React UI, so it works without a CLI installation or a separately managed server.

## Install

Download the current packages from [GitHub Releases](https://github.com/Olib-AI/ovc/releases/latest).

| Platform | Package | Notes |
|----------|---------|-------|
| macOS Apple Silicon | `ovc-desktop-macos-arm64.dmg` | Signed, notarized, and stapled |
| macOS Intel | `ovc-desktop-macos-amd64.dmg` | Signed, notarized, and stapled |
| Windows | `ovc-desktop-windows-amd64.msi` | 64-bit installer |
| Linux | `ovc-desktop-linux-amd64.deb` | Debian and Ubuntu package |
| Linux | `ovc-desktop-linux-amd64.AppImage` | Portable executable |

## First Launch

Open OVC from the Applications folder, Start menu, or desktop launcher. OVC starts its private local service automatically. No terminal command is needed.

If the repository directory is empty, the app displays onboarding. Enter:

1. Your commit author name
2. Your commit author email
3. A name for the first repository
4. An encryption password and confirmation

The identity is saved inside the encrypted repository configuration. Repository passwords are not stored in the web interface.

## Repository Storage

By default, the desktop app stores repositories under the platform data directory in an `ovc/repos` folder. Set `OVC_REPOS_DIR` before starting the app to use another location.

The desktop service listens only on `127.0.0.1` and selects an available port for each launch. Its authentication secret and session are generated in memory for that launch.

## System Webview

OVC uses the web engine supplied by the operating system:

- macOS uses WKWebView
- Windows uses WebView2
- Linux uses WebKitGTK

Current Windows installations normally include WebView2. Linux packages declare the GTK and WebKitGTK runtime dependencies. The AppImage may still rely on compatible host graphics and webview libraries.

## Development

Build the frontend before running the desktop crate:

```bash
cd frontend
npm install
npm run build
cd ..
cargo run -p ovc-desktop
```

Create a platform bundle with `cargo-bundle`:

```bash
cargo install cargo-bundle --version 0.11.0
cargo bundle --release -p ovc-desktop
```

Tagged GitHub releases build all supported desktop packages. The macOS release job signs the app, creates a DMG, waits for Apple notarization, and staples the notarization ticket.

## CLI and Browser Use

The CLI remains available as a separate download. Use it for terminal workflows, automation, remote servers, and explicit browser hosting:

```bash
ovc serve --port 9742
ovc web
```

Desktop and CLI installations can coexist. They use the same repository format.
