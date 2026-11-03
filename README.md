<div align="center">

# 🕳️ BlackHole

**Un navigateur rapide, élégant et respectueux de votre vie privée.**
Écrit en Rust, il s'appuie sur le moteur web natif de votre système : WebKit sur macOS, WebView2 sur Windows.

![Rust](https://img.shields.io/badge/Rust-2021-orange?logo=rust)
![macOS](https://img.shields.io/badge/macOS-WKWebView-black?logo=apple)
![Windows](https://img.shields.io/badge/Windows-WebView2-0078D6?logo=windows)
![Télémétrie](https://img.shields.io/badge/télémétrie-aucune-brightgreen)

[Fonctionnalités](#-fonctionnalités) · [Installation](#-installation) · [Extensions](#-extensions) · [Vie privée](#-vie-privée) · [Développement](#-développement)

![Barre latérale, onglets épinglés et espaces](docs/screenshots/sidebar.png)

</div>

---

## 🖼️ Aperçu

| Page d'accueil | Nouveautés après mise à jour |
|---|---|
| ![Accueil](docs/screenshots/home.png) | ![Nouveautés](docs/screenshots/whatsnew.png) |

![Réglages](docs/screenshots/settings.png)

## ✨ Fonctionnalités

### Une navigation pensée pour s'organiser
- 🗂️ **Barre latérale à la Arc** : un bouton à côté du logo l'affiche en grand ou la masque entièrement, avec animation fluide ; masquée, elle réapparaît en survol quand le curseur touche le bord gauche, sans déplacer la page. Largeur réglable, page en carte surélevée. `Ctrl/Cmd + B` pour basculer ; le mode choisi est retenu au prochain démarrage.
- ⚡ **Performances** : le démarrage lit session, historique, icônes et extensions en parallèle (`thread::scope`) ; toutes les écritures disque passent par un thread d'arrière-plan qui fusionne les écritures d'un même fichier (`infrastructure::writer`, vidé avant chaque lecture et à la fermeture) ; l'interface n'est rafraîchie qu'une fois par tour de boucle et la table des favicons n'est renvoyée que lorsqu'elle change. Les vues web restent créées sur le thread principal (contrainte des moteurs).
- 🛡 **Bloqueur de traqueurs** : réglage « Block trackers » (Réglages → Privacy), actif par défaut ; coupé, la liste de blocage n'est plus appliquée (la mise à niveau https et le retrait des paramètres de suivi restent). Les pages déjà ouvertes gardent leur comportement jusqu'au rechargement.
- 📌 **Sections repliables** : « Quick access » (onglets épinglés) et « Pinned » (dossiers) se replient depuis leur en-tête ; l'état est mémorisé.
- 🔐 **Comptes par espace** : chaque espace a son propre stockage de cookies, donc une session (Gmail perso dans « Personal », compte pro dans « Travail »…) ne déborde jamais sur les autres. Le premier espace (« Personal ») garde le stockage historique, donc ses connexions existantes survivent à la mise à jour ; une fenêtre privée n'enregistre rien (macOS : stockage non persistant, y compris avec l'hôte d'extensions). Une popup de connexion ou de rafraîchissement s'ouvre dans l'espace actif, le dernier ouvert. macOS : `WKWebsiteDataStore` identifié (14+) ; Windows : un profil WebView2 par espace (les extensions ne sont chargées que dans le profil principal). Les données d'un espace supprimé restent sur le disque.
- 🌌 **Espaces** : des contextes cloisonnés, colorés, avec emoji. Chaque espace teinte l'interface à ses couleurs. Réorganisables par glisser-déposer ; balayage à deux doigts (ou molette horizontale) sur la barre latérale pour en changer.
- 📌 **Onglets** : épinglés en grille en haut de la barre latérale, séparés des onglets classiques par un séparateur (un onglet épinglé n'est pas forcément un favori, et inversement), dossiers et sous-dossiers (glisser un dossier sur un autre, ou clic droit → « New subfolder » ; les imports gardent l'arborescence `dossier_1/dossier_2`), glisser-déposer, mise en veille automatique des onglets inactifs (5/10/15 min). Ouvrir une adresse déjà ouverte bascule sur l'onglet existant.
- ⌨️ **Barre de commandes** (`Ctrl/Cmd + L`) : recherche, saisie d'URL, changement d'onglet, favoris, historique et actions rapides. `Ctrl/Cmd + T` ouvre un nouvel onglet sur la page d'accueil. `Ctrl/Cmd + N` ouvre une nouvelle fenêtre (processus séparé, démarrage maximisé) ; `Ctrl/Cmd + Maj + N` une fenêtre privée (profil temporaire supprimé à la fermeture, pas de mise à jour automatique).
- ◧ **Vue scindée** : deux pages côte à côte, mémorisée par espace.
- 👀 **Aperçu** : `Maj + clic` sur un lien l'ouvre dans une fenêtre flottante (`Échap` ferme, `Ctrl/Cmd + Entrée` l'ouvre dans un onglet).
- 🎨 **Boosts** : du CSS personnalisé par site, appliqué à chaque chargement.
- 🖼️ **Easel** (`Ctrl/Cmd + Maj + E`) : des tableaux libres, où poser des notes et des **captures de pages** (`Ctrl/Cmd + Maj + S`, ou « Capture this page to Easel » dans la barre de commandes : titre, texte sélectionné et lien vers la page). Cartes déplaçables, redimensionnables, colorées ; un clic sur le titre d'une capture rouvre la page dans un onglet. **Capture live** (`Ctrl/Cmd + Maj + L`, ou « Live capture » dans la barre de commandes) : on clique une zone de la page, elle s'affiche en direct et reste interactive dans une carte (▶ / ⏸ ; connexions partagées avec le navigateur, pisteurs bloqués ; une carte seulement partiellement visible est masquée). Un Easel est un onglet à part entière : cliqué dans le panneau de gauche (ou choisi dans la barre de commandes), il s'affiche dans la zone de contenu, comme une page web, avec ses propres commandes (nom, note, capture, suppression, « All Easels », fermer) ; `Ctrl/Cmd + Maj + E` ouvre la fenêtre qui liste tous les Easels. Enregistrés dans `easels.json`. Pas de partage pour l'instant.
- 🚦 **Routage des liens** : « toujours ouvrir ce site dans tel espace ».
- 🪟 **Glisser un onglet vers une autre fenêtre** (privée ou non, même espace ouvert deux fois) : on lâche l'onglet sur l'autre fenêtre ; il s'y ouvre dans l'espace actif et se ferme dans la source (seule l'adresse voyage, la page se recharge) ; relâché au-dessus d'aucune fenêtre, il devient une nouvelle fenêtre (même mode, privé ou non, via `--open <url>`). Les fenêtres se repèrent via un registre dans le dossier temporaire (`BlackHole-windows`).
- 🔊 **Médias** : un onglet qui lit du son affiche une icône 🔊/🔇 (clic pour couper) ; quand il n'est pas affiché, une mini-carte en bas du panneau (pause/lecture, muet, Picture in Picture, fermer ; clic sur le titre pour y retourner) le commande. Détection par sondage des pages toutes les 1,5 s ; les vidéos muettes d'ambiance sont ignorées.
- 📺 **Picture in Picture** : en quittant un onglet qui lit une vidéo (YouTube, Meet…), elle passe en fenêtre flottante, et revient dans la page au retour sur l'onglet ; « Picture in Picture » dans la barre de commandes l'active à la main. macOS : API WebKit ; Windows : API Chromium, qui peut exiger un geste utilisateur selon le site.
- ⚡ **Fluidité** : des vues d'onglet vierges sont préparées d'avance, ouvrir un onglet n'attend plus la recréation de la barre de commandes.
- ★ **Favoris** : import, dossiers, glisser-déposer depuis la barre latérale ou d'autres navigateurs.
- 🆕 **Nouveautés** : après une mise à jour, la page d'accueil présente le changelog, une seule fois au premier redémarrage.

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

- **Extensions natives (15.4+)** via `WKWebExtensionController` (crate `macos-webext`). Les favoris importés deviennent de vrais dossiers et onglets de l'espace actif (glisser-déposer, fermeture, suppression comme n'importe quel onglet) ; les dossiers se réordonnent par glisser-déposer. La **Bibliothèque** (bouton 🗂 en bas du panneau ou palette) regroupe médias, téléchargements, Easels & notes, espaces et onglets archivés (les onglets fermés sont archivés et rouvrables ; « Clear » ferme les onglets libres) ; les téléchargements vont dans le dossier Téléchargements et les programmes ne sont jamais lancés, seulement révélés. Les Easels sont des entrées du panneau de gauche (clic pour ouvrir dans la zone de contenu, glisser-déposer dans un dossier comme un onglet, clic droit pour supprimer) et de la barre de commandes (« Easel : nom »). Menus contextuels (⋯ / clic droit) sur le fond du panneau (nouvel onglet, dossier, Easel et liste des Easels, espace), les dossiers (emoji, replier, supprimer), onglets, accès rapides et espaces, en-tête de fermeture par panneau en vue scindée. uBlock Origin Lite est la version « Lite » (MV3, `declarativeNetRequest`) ; sa page d'options n'est pas exposée.
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
- Easel (bout en bout, les deux OS) : `scripts/test-easel.sh` (macOS) et `scripts\test-easel.ps1` (Windows).

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
