# FerdiLight — Plan de développement

## Objectif

App Tauri v2 minimaliste type Ferdium. Sidebar avec icônes, multiwebview native isolée (cookies/sessions persistants par service). Front vanilla HTML/CSS/JS, pas de framework.

## Structure du projet

```
ferdilight/
├── src-tauri/
│   ├── src/
│   │   ├── main.rs          # setup app, commandes Tauri
│   │   ├── webviews.rs      # gestion création/show/hide des webviews
│   │   └── config.rs        # lecture services.json
│   ├── Cargo.toml
│   └── tauri.conf.json
├── src/
│   ├── index.html            # sidebar + container
│   ├── style.css             # sidebar style minimal, dark theme
│   └── main.js               # appels invoke Tauri pour switch webview
└── services.json              # [{id, name, url, icon}]
```

## Étapes de développement

### 1. Init projet
- `npm create tauri-app@latest ferdilight` avec vanilla template, Tauri v2
- Vérifier que tout compile et se lance

### 2. Configuration des services
- Créer `services.json` à la racine avec 3 services exemple :
  ```json
  [
    {"id": "gmail", "name": "Gmail", "url": "https://mail.google.com", "icon": "📧"},
    {"id": "slack", "name": "Slack", "url": "https://app.slack.com", "icon": "💬"},
    {"id": "github", "name": "GitHub", "url": "https://github.com", "icon": "🐙"}
  ]
  ```
- Implémenter `config.rs` : struct `Service`, lecture et parsing du JSON depuis le app data dir

### 3. Gestion des webviews natives (webviews.rs)
- Pour chaque service, créer une webview native Tauri v2 avec :
  - `data_directory` isolé par service (pour cookies/sessions persistants)
  - URL du service
  - Positionnée à droite de la sidebar (offset x = 48px)
  - Taille = fenêtre principale moins sidebar
- Fonctions :
  - `create_webview(app, service)` — crée la webview si pas encore créée (lazy loading)
  - `show_webview(id)` — `set_visible(true)` sur la webview cible
  - `hide_all_webviews()` — `set_visible(false)` sur toutes les webviews
  - `switch_to(id)` — hide all + show/create la cible

### 4. Commandes Tauri (main.rs)
- `#[tauri::command] get_services()` — retourne la liste des services
- `#[tauri::command] switch_service(id)` — appelle switch_to
- Setup : charger config, enregistrer commandes, créer fenêtre principale

### 5. Frontend (vanilla HTML/CSS/JS)
- `index.html` : sidebar fixe à gauche (48px de large), pas de contenu à droite (les webviews natives sont superposées)
- `style.css` :
  - Dark theme (#1a1a2e fond, #16213e sidebar)
  - Sidebar : flex column, icônes centrées, hover effect
  - Onglet actif : bordure gauche colorée
- `main.js` :
  - Au chargement : `invoke('get_services')` pour construire la sidebar
  - Au clic sur icône : `invoke('switch_service', { id })`
  - Mettre à jour l'état visuel de l'onglet actif

### 6. Resize handling
- Écouter l'événement de resize de la fenêtre principale
- Redimensionner toutes les webviews existantes en conséquence

### 7. Persistance état
- Sauvegarder le dernier service actif dans un petit fichier JSON dans app data dir
- Au lancement, rouvrir le dernier service actif

## Contraintes techniques

- Tauri v2 latest (pas v1)
- Rust edition 2021
- Pas de framework JS (vanilla uniquement)
- Dark theme minimal
- Mémoire minimale : webviews lazy-loaded, une seule visible à la fois
- Chaque webview a son propre data_directory pour isolation complète des cookies/sessions
- services.json éditable par l'utilisateur dans le app data dir
