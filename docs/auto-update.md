# Mises à jour automatiques

Taurium utilise le plugin **updater** de Tauri v2. L'app vérifie au démarrage
un manifeste `latest.json` publié sur les *GitHub Releases*, compare la version,
et — sur action de l'utilisateur (Réglages ▸ Mises à jour) — télécharge
l'installateur, **vérifie sa signature**, l'installe et relance l'app.

## Mise en route (à faire une seule fois)

### 1. Générer la paire de clés de signature

```bash
npm run tauri signer generate -- -w taurium.key
```

Cela produit :
- une **clé privée** (`taurium.key`) + un mot de passe — **secrets, à ne jamais committer** ;
- une **clé publique** affichée dans le terminal.

> ⚠️ Sauvegarde la clé privée en lieu sûr. **Si tu la perds, tu ne peux plus
> livrer de mises à jour** aux utilisateurs déjà installés (signatures rejetées).

### 2. Renseigner la clé publique

Dans `src-tauri/tauri.conf.json`, remplace la valeur placeholder :

```jsonc
"plugins": { "updater": { "pubkey": "<CLÉ PUBLIQUE>", "endpoints": [ ... ] } }
```

### 3. Ajouter les secrets GitHub

Dans *Settings ▸ Secrets and variables ▸ Actions* du dépôt :

- `TAURI_SIGNING_PRIVATE_KEY` = contenu de `taurium.key`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` = le mot de passe choisi

Le workflow `.github/workflows/release.yml` (via `tauri-apps/tauri-action`)
signe alors les artefacts et génère `latest.json` automatiquement.

## Publier une version

1. Bumper la version dans `package.json`, `src-tauri/tauri.conf.json`,
   `src-tauri/Cargo.toml` (+ `Cargo.lock`).
2. Lancer le workflow **Release** (onglet Actions ▸ *Run workflow* ▸ version
   `vX.Y.Z`), ou pousser un tag `vX.Y.Z`.
3. La release est créée en **brouillon**. **Publie-la** : l'updater ne voit que
   la *dernière release publiée* (`releases/latest/download/latest.json`).

## Détails techniques

- Windows : la mise à jour passe par l'installateur **NSIS** ; l'app doit être
  installée via l'installateur (pas de version portable).
- La comparaison de version est **semver** : incrémente bien la version à
  chaque release, sinon aucune mise à jour n'est proposée.
- Le check de démarrage est silencieux et non bloquant (hors ligne / pas de
  manifeste = aucune erreur visible). Un point vert sur l'engrenage +
  un toast signalent une mise à jour ; l'installation est **manuelle**.
