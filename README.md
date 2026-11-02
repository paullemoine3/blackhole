<div align="center">

# 🕳️ BlackHole

**Un navigateur rapide, élégant et respectueux de votre vie privée.**
Écrit en Rust, il s'appuie sur le moteur web natif de votre système : WebKit sur macOS, WebView2 sur Windows.

![Rust](https://img.shields.io/badge/Rust-2021-orange?logo=rust)
![macOS](https://img.shields.io/badge/macOS-WKWebView-black?logo=apple)
![Windows](https://img.shields.io/badge/Windows-WebView2-0078D6?logo=windows)
![Télémétrie](https://img.shields.io/badge/télémétrie-aucune-brightgreen)

[Fonctionnalités](#-fonctionnalités) · [Installation](#-installation) · [Extensions](#-extensions) · [Vie privée](#-vie-privée) · [Développement](#-développement)

</div>

---

## ✨ Fonctionnalités

### Une navigation pensée pour s'organiser
- 🗂️ **Barre latérale à la Arc** : un bouton à côté du logo l'affiche en grand ou la masque entièrement, avec animation fluide ; masquée, elle réapparaît en survol quand le curseur touche le bord gauche, sans déplacer la page. Largeur réglable, page en carte surélevée. `Ctrl/Cmd + B` pour basculer.
- 🌌 **Espaces** : des contextes cloisonnés, colorés, avec emoji. Chaque espace teinte l'interface à ses couleurs.
- 📌 **Onglets** : épinglés en grille, dossiers, glisser-déposer, mise en veille automatique des onglets inactifs (5/10/15 min). Ouvrir une adresse déjà ouverte bascule sur l'onglet existant.
- ⌨️ **Barre de commandes** (`Ctrl/Cmd + L`) : recherche, saisie d'URL, changement d'onglet, favoris, historique et actions rapides. `Ctrl/Cmd + T` ouvre un nouvel onglet sur la page d'accueil. `Ctrl/Cmd + N` ouvre une nouvelle fenêtre (processus séparé, démarrage maximisé) ; `Ctrl/Cmd + Maj + N` une fenêtre privée (profil temporaire supprimé à la fermeture, pas de mise à jour automatique).
- ◧ **Vue scindée** : deux pages côte à côte, mémorisée par espace.
- 👀 **Aperçu** : `Maj + clic` sur un lien l'ouvre dans une fenêtre flottante (`Échap` ferme, `Ctrl/Cmd + Entrée` l'ouvre dans un onglet).
- 🎨 **Boosts** : du CSS personnalisé par site, appliqué à chaque chargement.
- 🚦 **Routage des liens** : « toujours ouvrir ce site dans tel espace ».
- ★ **Favoris** : import, dossiers, glisser-déposer depuis la barre latérale ou d'autres navigateurs.

### Pensé pour chaque système
| | macOS | Windows |
|---|---|---|
| Moteur | WKWebView | WebView2 (Chromium) |
| Extensions | `WKWebExtensionController` (15.4+) | `AddBrowserExtension` |
| Installation | `BlackHole.app` / `.dmg` | Installeur et zip portable |
| Fenêtre | Barre système | Barre de titre intégrée à l'app |
| Mise à jour | Remplacement du `.app` | Installeur silencieux |

---

## 📦 Installation

Les versions sont publiées dans les [releases GitHub](../../releases).

- **Windows** : lancez `BlackHole-Setup-X.Y.Z-x64.exe` (ou utilisez le zip portable).
- **macOS** : ouvrez le `.dmg` / le zip et glissez `BlackHole.app` dans `/Applications`. Pour l'avoir comme navigateur par défaut, installez-le depuis `scripts/package-macos.sh --install`.

**Mises à jour intégrées** : quand une nouvelle version existe, un bouton « Update to vX.Y.Z » apparaît en bas du panneau de gauche. Un clic télécharge la release (HTTPS, hôte github.com uniquement), vérifie son SHA-256 puis met à jour et relance l'app.

---

## 🧩 Extensions

BlackHole exécute de **vraies WebExtensions**, installables en un clic depuis **Paramètres → Extensions** :

| Extension | Rôle |
|---|---|
| uBlock Origin Lite | Blocage de publicités |
| Dark Reader | Mode sombre sur tous les sites |
| SponsorBlock | Saute les segments sponsorisés |
| Bitwarden | Gestionnaire de mots de passe |

Chaque paquet est téléchargé depuis sa release officielle et vérifié par **SHA-256 épinglé** avant installation. Les extensions ayant un popup ont un bouton dans la barre du haut.

Vous pouvez aussi déposer de simples content scripts (`manifest.json` + `content_scripts`) dans `<données>/extensions/<id>/`.

**Limites** : sous macOS, pas de Touch ID ni de messagerie native ; sous Windows, l'API d'extensions de WebView2 n'expose ni badge ni onglet actif.

---

## 🔒 Vie privée

- Moteur de recherche **DuckDuckGo** par défaut, **aucune télémétrie**.
- Blocage des traqueurs (navigation, `fetch`, XHR, balises), liste dans `crates/infrastructure/blocklist.txt` ; compteur et journal dans Paramètres → Confidentialité.
- Passage automatique de HTTP vers HTTPS, suppression des paramètres de suivi.
- Mots de passe importés rangés dans un **coffre chiffré** (ChaCha20-Poly1305) dont la clé est dans le trousseau du système.
- Code sûr : `unsafe` interdit hors des crates d'intégration système (`macos-url`, `macos-webext`).

> Le blocage dans la page est au mieux de ses possibilités : `wry` n'expose pas de hook réseau.

---

## 📥 Import

Paramètres → Import détecte automatiquement les profils **Chrome, Edge, Brave, Vivaldi, Opera, Firefox, Waterfox, LibreWolf, Zen** et d'autres navigateurs Chromium. Favoris en lecture seule (ou via export HTML), mots de passe depuis l'export CSV.

---

## 🍎 Spécificités macOS

macOS reste une plateforme de premier rang : tout ce qui suit est conservé et fonctionne en parallèle de la version Windows.

- **Extensions natives (15.4+)** via `WKWebExtensionController` (crate `macos-webext`). Menus contextuels (⋯ / clic droit) sur onglets, accès rapides et espaces, en-tête de fermeture par panneau en vue scindée. uBlock Origin Lite est la version « Lite » (MV3, `declarativeNetRequest`) ; sa page d'options n'est pas exposée.
- **Popups d'extensions** : le bouton (icône + badge) à droite de la barre du haut ouvre le popup natif (`WKWebExtensionAction.popupPopover`). L'onglet actif est exposé aux extensions (`WKWebExtensionTab` / `Window`), ce qui permet à uBlock Origin Lite d'afficher le site courant.
- **Extensions des apps installées** : Paramètres → Extensions → « Trouvées dans vos apps » liste les extensions Safari de `/Applications` et `~/Applications` (`*.app/Contents/PlugIns/*.appex`) et permet de les activer (`app_extensions.json`). Limites : pas de Touch ID ni de native messaging.
- **Compatibilité Bitwarden / Dark Reader** :
  - API `privacy` émulée (`privacy_shim.js`) : WebKit ne l'a pas, et sans elle le réglage « gestionnaire de mots de passe par défaut » de Bitwarden se décoche seul.
  - Extensions chargées avec le token d'agent `Firefox/128.0` (requis par Bitwarden, build Firefox).
  - Cadres d'extension (menu d'autofill, barre de notification) autorisés dans les pages.
  - Extensions à page d'arrière-plan persistante (Bitwarden) servies sous `safari-web-extension://<id>` : le menu d'autofill refuse toute autre URL d'iframe. Les extensions à service worker ne reçoivent pas ce schéma (sinon WebKit ne livre plus les messages des content scripts et Dark Reader reste sur son style de secours).
- **Navigateur par défaut** : `scripts/package-macos.sh [--install]` produit `dist/BlackHole.app` (http/https) et l'installe dans `/Applications` ; ensuite, action « Make BlackHole the default browser ». Les liens externes (démarrage à froid ou app ouverte) ouvrent un onglet.
- **Gestes trackpad** : deux doigts pour page précédente/suivante (aperçu natif de la page) et changement d'espace sur la barre latérale.
- **Rail de 84 px** : couvre les boutons système fermer/réduire/agrandir.
- **Vidéo** : WKWebView compose et décode déjà sur GPU, aucun réglage n'est nécessaire. (Annoncer `Safari/605.1.15` dans le user-agent rendait YouTube très lent : écarté.)
- **Mise à jour** : remplacement de `BlackHole.app` puis relance ; désactivée hors bundle `.app`. Le bundle est signé ad hoc (pas de notarisation).
- **Autoremplissage natif** : au focus d'un champ identifiant/mot de passe d'une page https (ou localhost), BlackHole propose les identifiants du coffre (menu dans un shadow root fermé). Désactivé tant que l'extension Bitwarden est active ; le mot de passe n'est injecté qu'après un clic.
- **Tests** : `scripts/test-webext.sh` (installe uBOL, vérifie le blocage actif/inactif, retrait) et harnais `devtools` avec captures CoreGraphics (`/tmp/bh_script.txt` → `/tmp/bh_shots/`).

## 🛠️ Développement

Architecture en couches : `domain` → `application` → `infrastructure` → `app` (winit + wry).

```bash
cargo run -p blackhole             # développement
cargo run --release -p blackhole   # performances maximales
cargo test --workspace
```

**Tests d'interface** (feature `devtools`) :
- Windows : `scripts\test-ui.ps1` (scénarios, animations, captures) et `scripts\test-ext.ps1` (extensions).
- macOS : `scripts/test-webext.sh` et captures via le harnais `devtools`.

**Installeur Windows** :
```powershell
cargo build --release -p blackhole
iscc /DAppVersion=0.1.0 /DExePath=..\target\release\blackhole.exe scripts\windows-installer.iss
```

**Publier une version** : mettre à jour `version` dans `Cargo.toml`, committer et pousser sur `main` (ou pousser le tag `vX.Y.Z`). Le workflow `release.yml` teste, construit l'installeur Windows, le binaire macOS universel et publie les artefacts avec `SHA256SUMS.txt`.

---

## 🗺️ Roadmap
- Blocage réseau natif (`WKContentRuleList` sur macOS, interception de requêtes WebView2 sur Windows)
- Fenêtre flottante compacte
- Notarisation macOS

---

<div align="center">Fait avec ❤️ et Rust</div>
