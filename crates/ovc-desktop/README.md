# OVC Desktop

`ovc-desktop` is a small native Slint shell around OVC's existing React UI. It
starts the embedded OVC server on a random loopback port and displays it in the
operating system's webview. The browser engine is not bundled:

- macOS: WKWebView
- Windows: WebView2
- Linux: WebKitGTK

This keeps the desktop UI in sync with the browser UI and avoids shipping a
second frontend implementation.

The service is part of the desktop executable: `ovc-desktop` links `ovc-api`
and embeds the compiled React assets. It does not require `ovc serve`, a daemon,
or the CLI to be installed. Release installers may therefore update the GUI and
CLI independently without breaking application startup.

On a new installation, the embedded service creates a private session for the
desktop webview and the React UI opens its first-repository onboarding flow.
The author identity entered there is saved in the encrypted repository config.

## Run

Build the frontend once, then run the desktop binary:

```sh
cd frontend
npm ci
npm run build
cd ..
cargo run -p ovc-desktop
```

Set `OVC_REPOS_DIR` to choose the repository storage directory. Without it,
the app uses the platform data directory (`OVC/repos`). Existing LLM environment
variables supported by `ovc serve` are also honored.

## Linux prerequisites

Install WebKitGTK development packages before building. Package names vary by
distribution (for example, `libwebkit2gtk-4.1-dev` on Debian/Ubuntu). Wry child
webviews currently require an X11 session; Wayland users can run the application
through XWayland.

## Packaging

The crate contains `cargo-bundle` metadata and checked-in PNG/ICO application
icons derived from `examples/logo.svg`:

```sh
cargo install cargo-bundle
cargo bundle --release -p ovc-desktop --format osx
```

On macOS, run the generated `OVC.app` rather than the raw executable to get the
configured Dock and Finder icon. macOS does not support per-window title-bar
icons; `cargo-bundle` converts the PNG icon set into the bundle's native icon.

Tagged GitHub releases build the supported distribution formats automatically:

- macOS: signed, notarized, and stapled `.dmg` for Intel and Apple Silicon
- Windows: `.msi` installer
- Linux: `.deb` package and `.AppImage`
