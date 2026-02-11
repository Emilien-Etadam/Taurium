# Taurium

**FR** | Une alternative minimaliste a [Ferdium](https://ferdium.org/), construite avec [Tauri v2](https://v2.tauri.app/).
Regroupez tous vos services web (WhatsApp, Slack, Gmail, Discord...) dans une seule fenetre avec une sidebar d'icones.

**EN** | A minimalist alternative to [Ferdium](https://ferdium.org/), built with [Tauri v2](https://v2.tauri.app/).
Group all your web services (WhatsApp, Slack, Gmail, Discord...) in a single window with an icon sidebar.

---

## Features / Fonctionnalites

- Sidebar with service icons (emoji or custom PNG)
- Desktop notifications with badge count detection
- Lazy-loading webviews (loads on first click)
- Hibernation of inactive tabs (10 min)
- Drag & drop service reordering
- Hot-reload: add/remove services without restart
- Customizable sidebar color, accent color, icon size
- Native context menu (reload, open in browser)
- Keyboard shortcuts: `Ctrl+1-9` switch service, `Ctrl+,` settings

## Screenshot

```
 [S]  |                                  |
 [W]  |       < service webview >        |
 [G]  |                                  |
 [D]  |                                  |
      |                                  |
 [*]  |__________________________________|
```

## Installation

### Prerequisites / Prerequis

- [Node.js](https://nodejs.org/) >= 18
- [Rust](https://rustup.rs/) (stable)
- Windows: [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) + WebView2 (included in Windows 10/11)

### Dev

```bash
git clone https://github.com/Emilien-Etadam/Taurium.git
cd Taurium
npm install
npm run tauri dev
```

### Build / Compilation

```bash
npm run tauri build
```

The installer will be in `src-tauri/target/release/bundle/`:
- **Windows**: `nsis/Taurium_0.1.0_x64-setup.exe` and `msi/Taurium_0.1.0_x64_en-US.msi`

## Release Windows

### Manual release / Release manuelle

```bash
npm run tauri build
```

Then distribute the `.exe` or `.msi` from `src-tauri/target/release/bundle/`.

### Automated release with GitHub Actions

Create `.github/workflows/release.yml`:

```yaml
name: Release

on:
  push:
    tags:
      - "v*"

jobs:
  build-windows:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4

      - name: Setup Node
        uses: actions/setup-node@v4
        with:
          node-version: 20

      - name: Setup Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Install dependencies
        run: npm install

      - name: Build Tauri app
        uses: tauri-apps/tauri-action@v0
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        with:
          tagName: ${{ github.ref_name }}
          releaseName: "Taurium ${{ github.ref_name }}"
          releaseBody: "Download the .exe installer below."
          releaseDraft: true
```

Then to create a release:

```bash
# Update version in package.json and src-tauri/tauri.conf.json
git tag v0.1.0
git push origin v0.1.0
```

GitHub Actions will build and attach the Windows installer to a draft release.

## Tech Stack

- **Frontend**: Vanilla JS, HTML, CSS
- **Backend**: Rust + Tauri v2 (multi-webview, unstable feature)
- **Notifications**: tauri-plugin-notification
- **Config**: JSON files in app data directory

## License

MIT
