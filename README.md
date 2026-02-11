<p align="center">
  <img src="https://img.shields.io/badge/built%20with-Tauri%20v2-blue?style=flat-square" />
  <img src="https://img.shields.io/badge/platform-Windows-0078D6?style=flat-square&logo=windows" />
  <img src="https://img.shields.io/badge/license-MIT-green?style=flat-square" />
</p>

# Taurium

> **FR** Marre de jongler entre 15 onglets ? Taurium regroupe tous vos services web dans une seule app, legere et rapide.
>
> **EN** Tired of juggling 15 browser tabs? Taurium brings all your web services together in one lightweight, fast app.

WhatsApp, Slack, Gmail, Discord, Telegram, Messenger, Google Messages... tout au meme endroit. / ...all in one place.

```
 +------+------------------------------------------+
 | [WA] |                                          |
 | [SL] |                                          |
 | [GM] |          WhatsApp Web                    |
 | [DC] |                                          |
 | [TG] |                (2) new messages           |
 |      |                                          |
 | [--] |                                          |
 |  [*] |                                          |
 +------+------------------------------------------+
   48px              content area
```

---

## Why Taurium? / Pourquoi Taurium ?

|  | Taurium | Ferdium | Browser tabs |
|---|---|---|---|
| RAM usage / Conso RAM | ~80 MB | ~500 MB+ | ~200 MB/tab |
| Startup / Demarrage | < 1s | 5-10s | - |
| App size / Taille | ~5 MB | ~250 MB | - |
| Notifications | Native desktop | Built-in | Per-site |
| Customizable / Personnalisable | Yes | Yes | No |

Taurium is built with **Tauri v2** + **Rust** instead of Electron — no bundled Chromium, it uses your system's WebView2.

---

## Features / Fonctionnalites

- **All your services, one window** / Tous vos services, une fenetre
- **Native notifications** with unread badge detection / Notifications natives avec detection des badges
- **Add any website** as a service (emoji or PNG icon) / Ajoutez n'importe quel site web
- **Drag & drop** to reorder services / Glisser-deposer pour reorganiser
- **Hot-reload** — add or remove services without restarting / sans redemarrer
- **Hibernation** — inactive tabs sleep after 10 min to save RAM
- **Theming** — sidebar color, accent color, icon size / Couleurs et taille personnalisables
- **Keyboard shortcuts** — `Ctrl+1-9` switch, `Ctrl+,` settings

---

## Getting Started / Demarrage rapide

### Download / Telecharger

Grab the latest `.exe` installer from [Releases](https://github.com/Emilien-Etadam/Taurium/releases).

### Build from source / Compiler depuis les sources

**Prerequisites**: [Node.js](https://nodejs.org/) >= 18, [Rust](https://rustup.rs/), Windows Build Tools

```bash
git clone https://github.com/Emilien-Etadam/Taurium.git
cd Taurium
npm install
npm run tauri dev       # dev mode
npm run tauri build     # production build
```

The installer lands in `src-tauri/target/release/bundle/nsis/`.

---

## Contributing

PRs welcome! The codebase is intentionally small and simple:

```
src/                  # Frontend (vanilla JS/HTML/CSS)
  main.js             # Sidebar logic
  settings.js         # Settings panel
src-tauri/src/        # Backend (Rust)
  lib.rs              # Commands & app setup
  webviews.rs         # Webview management
  config.rs           # Config & preferences
```

```bash
npm run tauri dev     # Start with hot-reload
```

---

## License

MIT — do whatever you want with it.
