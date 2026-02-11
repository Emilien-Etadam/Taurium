# Taurium ⚡

> **[EN]** Your web apps, minus the Electron bloat.
>
> **[FR]** Vos web apps, sans la lourdeur d'Electron.

Taurium is a minimalist workspace browser built with **Tauri v2** and **Rust**. It aggregates all your essential services (Slack, WhatsApp, Gmail, Discord...) into a single, lightweight window.

Taurium est un navigateur d'espace de travail minimaliste construit avec **Tauri v2** et **Rust**. Il regroupe tous vos services essentiels (Slack, WhatsApp, Gmail, Discord...) dans une seule fenêtre ultra-légère.

---

## 📉 Benchmarks

Why switch? The numbers speak for themselves.
Pourquoi changer ? Les chiffres parlent d'eux-mêmes.

| Metric | Taurium (Rust/WebView2) | Ferdium (Electron) | Browser Tabs (Chrome) |
| :--- | :--- | :--- | :--- |
| **Installer Size** | **~5 MB** | ~250 MB | N/A |
| **RAM Idle** | **~80 MB** | ~600 MB+ | ~400 MB+ |
| **Startup Time** | **< 1s** | ~5-10s | ~2s |
| **Tab Management** | Auto-hibernation | Manual | Manual |

---

## ✨ Features / Fonctionnalités

**[EN]**
*   **Zero Frameworks:** Pure Vanilla JS/HTML/CSS frontend. No React/Vue overhead.
*   **Tab Hibernation:** Inactive services sleep after 10 minutes to save RAM.
*   **Native Integration:** Desktop notifications and unread badges detection (scrapes `(3)` from titles).
*   **Hot-Swap:** Add/remove services instantly without restarting.
*   **Customization:** Drag & drop ordering, custom icons (Emoji/PNG), accent colors.
*   **Shortcuts:** `Ctrl+1-9` to switch tabs, `Ctrl+,` for settings.

**[FR]**
*   **Zéro Framework :** Frontend en Vanilla JS/HTML/CSS pur. Pas de surcharge React/Vue.
*   **Hibernation :** Les services inactifs s'endorment après 10 min pour sauver la RAM.
*   **Intégration Native :** Notifications bureau et détection des badges non-lus (récupère le `(3)` du titre).
*   **Hot-Swap :** Ajoutez/supprimez des services à chaud sans redémarrer.
*   **Personnalisation :** Réorganisation par glisser-déposer, icônes custom (Emoji/PNG), couleurs d'accent.
*   **Raccourcis :** `Ctrl+1-9` pour naviguer, `Ctrl+,` pour les réglages.

---

## 🚀 Get Started

### Pre-requisites / Prérequis
*   Rust (latest stable)
*   Node.js (LTS) & npm
*   System WebView2 (Installed by default on Windows 11/Modern macOS/Linux)

### Installation

```bash
# Clone the repo
git clone https://github.com/Emilien-Etadam/Taurium.git
cd Taurium

# Install dependencies
npm install

# Run in development mode
npm run tauri dev

# Build for production
npm run tauri build
```

---

## 🛠️ Contributing

We keep it simple. No complex state management, just DOM manipulation and Rust bindings.
On garde ça simple. Pas de gestion d'état complexe, juste de la manipulation DOM et des bindings Rust.

**Structure:**

```text
Taurium/
├── src-tauri/      # Rust backend (Window management, tray, system calls)
├── src/
│   ├── index.html  # Main entry point
│   ├── style.css   # Global styles (CSS Variables)
│   ├── main.js     # Logic: Tab switching, hibernation, IPC
│   └── assets/     # Icons & static files
└── package.json
```

1.  Fork & Clone.
2.  Create a feature branch (`git checkout -b feature/amazing-feature`).
3.  Commit changes.
4.  Open a Pull Request.

---

## 📄 License

Distributed under the **MIT License**. See `LICENSE` for more information.

[GitHub Repository](https://github.com/Emilien-Etadam/Taurium)
