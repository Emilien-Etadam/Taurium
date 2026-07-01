# Taurium ⚡

> **The anti-electron workspace browser.**
> *Le navigateur d'espace de travail anti-electron.*

Taurium aggregates your web services (WhatsApp, Slack, Discord...) into a single, high-performance window. Built with **Tauri v2 + Rust** — each service runs in its own native webview, not a bundled Chromium.

## ✨ Features

*   **Multi-service sidebar** — add any web app; switch with a click or `Ctrl+1`–`Ctrl+9`.
*   **Service catalog** — add popular services (Telegram, Teams, Notion, GitHub…) from a built-in list, or define your own.
*   **Isolated sessions** — each service has its own cookie/session store, so multiple accounts don't collide.
*   **Notifications & unread badges** — desktop notifications and sidebar badges derived from page titles.
*   **Memory-friendly** — services are lazy-loaded and inactive ones hibernate after 10 minutes.
*   **Per-service tweaks** — custom zoom, custom user-agent (applied immediately on save), emoji or image icons.
*   **Customizable UI** — dark theme, adjustable icon size, sidebar and accent colors.

## 🖥️ Supported platforms

*   **Windows** and **Linux** — built and released.
*   **macOS is not currently supported.** (Session isolation relies on a per-webview data directory that isn't wired up on macOS, so it's excluded from CI and releases for now.)

## 🚀 Get Started

### Prerequisites
*   Rust (latest stable)
*   Node.js (LTS) & npm
*   Linux only: WebKitGTK & related dev libraries (see `.github/workflows/ci.yml` for the exact `apt` list). Windows uses the system WebView2 runtime (usually pre-installed).

### Installation

```bash
# 1. Clone the repo
git clone https://github.com/Emilien-Etadam/Taurium.git
cd Taurium

# 2. Install dependencies
npm install

# 3. Run in development mode
npm run tauri dev

# 4. Build for production
npm run tauri build
```


## 📄 License

Distributed under the **MIT License**. See `LICENSE` for more information.

[GitHub Repository](https://github.com/Emilien-Etadam/Taurium)
