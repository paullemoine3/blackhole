<div align="center">

<img src="docs/title.svg" alt="BlackHole" width="640">

**Un navigateur rapide, élégant et respectueux de votre vie privée.**
Écrit en Rust, il s'appuie sur le moteur web natif de votre système : WebKit sur macOS, WebView2 sur Windows, WebView sur Android.

![Rust](https://img.shields.io/badge/Rust-2021-orange?logo=rust)
![macOS](https://img.shields.io/badge/macOS-WKWebView-black?logo=apple)
![Windows](https://img.shields.io/badge/Windows-WebView2-0078D6?logo=windows)
![Android](https://img.shields.io/badge/Android-WebView-3DDC84?logo=android)
![Télémétrie](https://img.shields.io/badge/télémétrie-aucune-brightgreen)

[Nouveautés](#-nouveautés) · [Fonctionnalités](#-fonctionnalités) · [Installation](#-installation) · [Extensions](#-extensions) · [Vie privée](#-vie-privée) · [Développement](#-développement)

![Barre latérale, dossiers, réunion en cours et page web](docs/screenshots/sidebar.png)

</div>

---

## 🆕 Nouveautés

- 📲 **0.8.4** : BlackHole pour Android se met à jour tout seul depuis les releases GitHub, barre d'adresse modifiable avec les icônes des sites, recherche dans la page, site bureau, ajout à l'écran d'accueil, impression, plein écran vidéo, envoi de fichiers et partage.
- 📱 **BlackHole pour Android** et **synchronisation des onglets** entre appareils appairés, sur le réseau local, sans compte ni serveur.
- 🛡️ **Shields** à la Brave : traqueurs, publicités, emplacements pub, pubs YouTube et bannières cookies, avec un rapport de confidentialité par site.
- 🌌 **Pages d'erreur animées** pour chaque cas : site injoignable, adresse introuvable, certificat, 401/403/5xx, hors ligne…
- 📅 **Google Agenda et Meet** : prochaine réunion dans la barre latérale, rappel au début, notes de réunion dans un Easel à côté de l'appel.
- 🔑 **Mots de passe** chiffrés sur l'appareil, proposés uniquement sur le bon site.
- 🩹 **0.8.1** : corrections de crashs macOS (fenêtre Réglages, premier chargement en échec), ports restreints, zones live de l'Easel.

## 🖼️ Aperçu

| Page d'accueil | Nouveautés après mise à jour |
|---|---|
| ![Accueil](docs/screenshots/home.png) | ![Nouveautés](docs/screenshots/whatsnew.png) |

| Easel avec une zone en direct | Page d'erreur |
|---|---|
| ![Easel](docs/screenshots/easel.png) | ![Site injoignable](docs/screenshots/errors.png) |

![Réglages, Shields](docs/screenshots/settings.png)

## ✨ Fonctionnalités

- 🗂️ **Barre latérale à la Arc** et 🌌 **espaces** colorés pour cloisonner vos contextes.
- 🧠 **Léger en mémoire** : peu d'onglets vivants (4 par défaut, réglable), voisins préchargés, anciens libérés.
- ⌨️ **Barre de commandes** (`Ctrl/Cmd + L`) : tout se fait au clavier, avec recherche dans tout l'historique.
- 🔁 **Onglets à la Arc** : nouveaux onglets en haut de la liste, `Ctrl + Tab` revient à l'onglet précédent, `Ctrl/Cmd + Alt + ↑/↓` parcourt la barre latérale, `Ctrl/Cmd + Maj + T` rouvre le dernier onglet fermé ; `Ctrl/Cmd + W` sur un onglet épinglé ou de dossier le met en veille sans le retirer.
- ◧ **Vue scindée** et 👀 **aperçu** flottant d'un lien (`Maj + clic`).
- 🎨 **Boosts** : CSS personnalisé par site.
- 🖼️ **Easel** : tableaux de notes et de captures de pages, y compris en direct ; 👁️ surveillance d'une zone de page.
- 📚 **Bibliothèque** : médias, téléchargements, historique et onglets archivés au même endroit.
- 🔐 **Comptes par espace** : chaque espace a ses propres cookies (perso et pro côte à côte).
- 📺 **Média** : mini-lecteur dans la barre latérale (lecture, son, micro et caméra d'un appel), Picture in Picture automatique (changement d'onglet ou perte de focus) avec retour à l'onglet.
- 🪟 **Glisser un onglet** vers une autre fenêtre pour l'y déplacer, ou 📲 l'envoyer sur un autre appareil.
- 🧰 **Outils de développement** des pages : `F12` ou `Ctrl + Maj + I` (`Cmd + Option + I` sur macOS).
- ⚡ **Démarrage et navigation rapides** : chargement parallèle, écritures disque en arrière-plan.
- ✨ **Animations soignées** : la liste d'onglets se réorganise en douceur, changement d'espace glissé, barre d'adresse et accueil animés (désactivables dans les réglages ; `prefers-reduced-motion` respecté).

| | macOS | Windows |
|---|---|---|
| Moteur | WKWebView | WebView2 (Chromium) |
| Extensions | `WKWebExtensionController` (15.4+) | `AddBrowserExtension` |
| Installation | `BlackHole.app` / `.dmg` | Installeur et zip portable |
| Mise à jour | Remplacement du `.app` | Installeur silencieux |

---

## 📦 Installation

Les versions sont publiées dans les [releases GitHub](../../releases).

- **Windows** : lancez `BlackHole-Setup-X.Y.Z-x64.exe` (ou le zip portable).
- **macOS** : ouvrez le `.dmg` et glissez `BlackHole.app` dans `/Applications` (ou `scripts/package-macos.sh --install`).

Un bouton « Update to vX.Y.Z » apparaît dans la barre latérale quand une version existe ; le téléchargement est vérifié par SHA-256.

---

## 🧩 Extensions

De **vraies WebExtensions**, installables en un clic depuis **Paramètres → Extensions** : uBlock Origin Lite, Dark Reader, SponsorBlock, Bitwarden. Chaque paquet vient de sa release officielle et est vérifié par **SHA-256 épinglé**.

Limites : pas de Touch ID ni de messagerie native sous macOS ; sous Windows, WebView2 n'expose ni badge ni onglet actif.

Sous macOS, les extensions Safari des apps installées peuvent aussi être activées (détails : [docs/macos.md](docs/macos.md)).

---

## 🔒 Vie privée

- Moteur **DuckDuckGo** par défaut (Brave Search, Startpage, Qwant, Mojeek ou Google au choix dans les Réglages), **aucune télémétrie**.
- **Shields** : traqueurs et serveurs publicitaires bloqués (liste dans `crates/infrastructure/blocklist.txt`), HTTPS automatique, paramètres de suivi supprimés.
- Mots de passe importés dans un **coffre chiffré** (ChaCha20-Poly1305), clé dans le trousseau système.
- `unsafe` interdit hors des crates d'intégration système (`macos-url`, `macos-webext`).

**Import** : Paramètres → Import détecte Chrome, Edge, Brave, Vivaldi, Opera, Firefox et dérivés (favoris, mots de passe via export CSV).

---

## 🛠️ Développement

Architecture en couches : `domain` → `application` → `infrastructure` → `app` (winit + wry). Détails dans `.github/copilot-instructions.md`.

```bash
cargo run -p blackhole             # développement
cargo run --release -p blackhole   # performances maximales
cargo clippy --workspace --all-targets
cargo test --workspace
```

**Tests d'interface** (feature `devtools`) : `scripts\test-ui.ps1`, `scripts\test-ext.ps1`, `scripts\test-easel.ps1` (Windows) ; `scripts/test-webext.sh`, `scripts/test-easel.sh`, `scripts/test-errors.sh`, `scripts/test-popup.sh`, `scripts/test-settings.sh`, `scripts/test-i18n.sh` (macOS). Les captures de ce README se régénèrent avec `scripts/readme-shots.sh`.

**Android** (version allégée, coque Kotlin + cœur Rust `crates/android`) : `cd android; .\gradlew.bat assembleDebug`, puis `scripts\test-android.ps1` (émulateur ou appareil USB). Outils et détails : [docs/android.md](docs/android.md).

**Installeur Windows** :
```powershell
cargo build --release -p blackhole
iscc /DAppVersion=X.Y.Z /DExePath=..\target\release\blackhole.exe scripts\windows-installer.iss
```

**Publier** : mettre à jour `version` dans `Cargo.toml`, committer et pousser sur `main` (ou pousser le tag `vX.Y.Z`). `release.yml` teste, construit les installeurs et publie avec `SHA256SUMS.txt`.

---

## 🗺️ Roadmap
- Blocage réseau natif (`WKContentRuleList` sur macOS, interception WebView2 sur Windows)
- Fenêtre flottante compacte
- Notarisation macOS

---

<div align="center">Fait avec ❤️ et Rust</div>
