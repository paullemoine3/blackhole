# BlackHole

Navigateur Rust inspiré d'Arc, basé sur le moteur web natif de l'OS via [`wry`](https://github.com/tauri-apps/wry) : **WKWebView** (macOS) et **WebView2** (Windows). Léger, rapide et compatible avec le web réel.

## Fonctionnalités
- Barre latérale à gauche, redimensionnant la page : rail de 84 px (couvre les boutons macOS fermer/réduire/agrandir) qui s'agrandit au survol (280 px) ou, au choix dans les Paramètres, toujours ouverte. `Cmd/Ctrl+B` (ou le bouton ⇤) bascule entre les deux modes.
- Spaces, onglets épinglés (clic droit sur un onglet), omnibox, session restaurée.
- **Paramètres** (⚙ ou ⌘,) : vraie fenêtre séparée — général (moteur de recherche, animations, navigateur par défaut), aide aux **raccourcis** (⌘L, ⌘T, ⌘\ split, ⌘B…), extensions, confidentialité. Persisté dans `settings.json`.
- **Extensions** : deux niveaux.
  - **Vraies WebExtensions (macOS 15.4+)** via l'hôte natif d'Apple (`WKWebExtensionController`, crate `macos-webext`). Un **catalogue** intégré (Paramètres → Extensions) propose l'installation en un clic de **uBlock Origin Lite**, **Dark Reader** et **SponsorBlock** : téléchargement HTTPS depuis la release officielle, **SHA-256 épinglé** vérifié avant installation, stocké dans `<données>/webext/`. Menus contextuels (⋯ / clic droit) sur onglets, accès rapides et espaces (suppression incluse), en-tête de fermeture par panneau en split view. Limites : uBOL est la version « Lite » (MV3, `declarativeNetRequest`), sa page d'options n'est pas exposée ; pas de popup d'extension ; l'adaptateur Windows (WebView2 `AddBrowserExtension`) reste à faire.
  - **Content scripts simples** (`manifest.json` + `content_scripts`) à déposer dans `<données>/extensions/<id>/`.
  - Test e2e : `scripts/test-webext.sh` (installe uBOL, vérifie le blocage réseau actif/inactif, retrait).
- **Navigateur par défaut (macOS)** : installer avec `scripts/package-macos.sh --install` (copie dans `/Applications`, requis pour apparaître dans Réglages Système → Bureau et Dock).
- **Bloqués** : le bouton 🛡 du bas de la barre latérale ouvre Paramètres → Confidentialité, avec la liste des hôtes bloqués (compteur, site d'origine) et un bouton de remise à zéro.
- **Espaces** : affichés comme des cartes (couleur en fond + emoji ou initiale) ; l'espace actif affiche son nom ; clic droit pour choisir un emoji.
- Barre latérale à animation continue (easing) ; animation du trou noir dans la palette de nouvel onglet.
- Raccourcis : `Cmd/Ctrl + L / T / W / R / B / [ / ]`. Geste trackpad à deux doigts : page précédente/suivante avec l'aperçu natif de la page (macOS), changement d'espace sur la barre latérale.
- Onglets : glisser-déposer (réordonner, dossiers, épinglés en grille), dossiers (double-clic pour renommer, clic droit pour supprimer), mise en veille des onglets inactifs (5/10/15 min dans les Paramètres). Comme dans Arc, ouvrir une adresse déjà ouverte (Command Bar, favori, lien, historique) bascule sur l'onglet existant, actif ou en veille, de l'espace courant (les espaces restent cloisonnés), sans le dupliquer (fragment `#…` ignoré).
- **Command Bar** (`Cmd/Ctrl + T` ou `L`) : panneau flottant centré — recherche/URL, « Switch to Tab », favoris, historique et actions rapides (nouvel espace, dossier, réglages, split, boost…).
- **Split View** : action « Split View with … » de la Command Bar (deux pages côte à côte ; cliquer l'onglet de droite échange les panneaux).
- **Peek** : `Shift+clic` sur un lien ouvre un aperçu flottant ; `Esc` ferme, `Cmd/Ctrl+Entrée` l'ouvre dans un onglet.
- **Boosts** : action « Boost <site> » — CSS personnalisé par site (presets inclus), appliqué à chaque chargement.
- **Air Traffic Control** : action « Always open <site> in <espace> » — les liens de ce site s'ouvrent dans leur espace.
- Chaque espace teinte la barre latérale et la barre du haut à sa couleur.
- **Emojis** pour les espaces (clic droit sur l'espace) et les dossiers (clic sur l'icône), via un sélecteur ; persistés.
- **Split view** persistée par espace : bouton ◧ sur un onglet, marqueur sur les deux onglets du duo, clic sur l'autre panneau = échange.
- **Navigateur par défaut (macOS)** : `scripts/package-macos.sh [--install]` produit `dist/BlackHole.app` (http/https), puis action « Make BlackHole the default browser ». Les liens externes (démarrage à froid ou app ouverte) ouvrent un onglet. Windows : à faire.
- Non implémenté : fenêtre flottante Little Arc.
- Animations « trou noir » au lancement et à l'ouverture d'un onglet (désactivables dans les Paramètres).

## Vie privée et sécurité
- DuckDuckGo par défaut, aucune télémétrie.
- Blocage des traqueurs à la navigation et dans la page (fetch, XHR, beacons, balises ; script `ui/page_guard.js`), liste dans `crates/infrastructure/blocklist.txt`.
- Mise à niveau HTTP → HTTPS, suppression des paramètres de tracking, seuls http(s) autorisés.
- `unsafe` interdit partout sauf dans le crate `macos-url` (Apple Events / navigateur par défaut), CSP et `textContent` dans l'UI, écritures atomiques.
- Topbar centrale (adresse/recherche), fond blanc par défaut des pages transparentes.
- Limite connue : wry n'expose pas de hook réseau, le blocage de traqueurs dans la page est donc best-effort. Suite prévue : `WKContentRuleList` (macOS) et interception de requêtes WebView2 (Windows).
- Servo a été retiré (trop d'incompatibilités CSS/JS).

## Architecture (clean architecture)
`domain` → `application` → `infrastructure` → `app` (composition root : winit + wry).

## Lancer
```
cargo run -p blackhole            # profil dev : profil dev optimisé
cargo run --release -p blackhole  # performances maximales
cargo test --workspace
```

### Harnais de test visuel (feature `devtools`)
`cargo run -p blackhole --features devtools` exécute le script `/tmp/bh_script.txt` (profil jetable `/tmp/bh_data`) et écrit des captures dans `/tmp/bh_shots/`. Captures macOS uniquement (CoreGraphics). Commandes : `wait MS`, `shot NOM`, `cmd JSON`, `eval chrome|bar|home|content JS`, `quit`.

## Roadmap
Blocage natif des traqueurs, popups, téléchargements, extensions, import/export de signets, packaging Mac/Windows.

## Popups d'extensions et extensions des apps installées

Les extensions actives ont un bouton (icône + badge) à droite de la barre du haut ; un clic ouvre leur popup native (`WKWebExtensionAction.popupPopover`). Paramètres → Extensions → « Trouvées dans vos apps » liste les extensions Safari embarquées dans `/Applications` et `~/Applications` (`*.app/Contents/PlugIns/*.appex`) et permet de les activer (persisté dans `app_extensions.json`). Limites : pas de Touch ID ni de native messaging (le handler vit dans le process de l'appex)  L'onglet actif est exposé aux extensions (objets `WKWebExtensionTab`/`Window`), ce qui permet à uBlock Origin Lite d'afficher le site courant et à la popup Bitwarden (build Firefox, user-agent Safari) de s'ouvrir. L'import (dialogue de fichier, trousseau) tourne hors du thread UI.

## Import depuis un autre navigateur

Les pages d'extension reçoivent une API `privacy` émulée (`privacy_shim.js`, valeurs gardées dans le stockage de l'extension) : WebKit ne l'a pas, et sans elle le réglage « gestionnaire de mots de passe par défaut » de Bitwarden se décoche seul. Les extensions sont chargées avec le token d'agent `Firefox/128.0` (popup et page d'arrière-plan cohérents, requis par Bitwarden), et les cadres d'extension (menu d'autofill, barre de notification) sont autorisés dans les pages. Les extensions sont servies sous le schéma `safari-web-extension://<id>` (et non `webkit-extension://`) : le menu d'autofill de Bitwarden refuse toute autre URL d'iframe et restait vide. Seules les extensions à page d'arrière-plan persistante (Bitwarden) reçoivent ce schéma : avec une URL de base personnalisée, WebKit ne livre plus les messages des content scripts aux extensions à service worker, et Dark Reader restait bloqué sur son style de secours (page sombre sans images ni textes).

**Remplissage automatique natif** : au focus d'un champ identifiant/mot de passe d'une page https (ou localhost), BlackHole propose les identifiants du coffre chiffré correspondant au site (menu dans un shadow root fermé). Il se désactive tant que l'extension Bitwarden est active, pour éviter deux menus sur les mêmes champs. Seuls les identifiants sont envoyés à la page ; le mot de passe n'y est injecté qu'après un clic de l'utilisateur sur une suggestion.

Paramètres → Import : détection automatique des profils Chrome, Edge, Brave, Vivaldi, Opera, Arc (Chromium) et Firefox, Waterfox, LibreWolf, Zen (Firefox). Les **favoris** sont lus en lecture seule dans le profil (copie temporaire de `places.sqlite`) ou via un export HTML, et deviennent cherchables dans la barre de commande et s'affichent dans la sidebar (section « Bookmarks », arborescence par dossiers, repliable). Les **mots de passe** s'importent depuis l'export CSV du navigateur d'origine (Chrome, Firefox, Safari, Bitwarden…) : ils sont rangés dans un coffre chiffré (ChaCha20-Poly1305) dont la clé est dans le trousseau système (Keychain / Gestionnaire d'identifiants). Pas encore d'autoremplissage.

### Drag & drop de bookmarks

Glisse un lien, un favori depuis un autre navigateur, un fichier `.webloc`/`.url` ou un export de favoris `.html` sur la barre latérale : ils sont ajoutés aux bookmarks (déposer sur un dossier du panneau ★ Bookmarks les range dedans).

Depuis le panneau ★ Bookmarks, glisse un favori sur la liste des onglets, un dossier ou les tuiles épinglées pour l'ouvrir à cet endroit (déposé sur les tuiles, il est épinglé).

**Vidéo / accélération matérielle** : WKWebView compose et décode déjà sur GPU (macOS), il n'existe pas de réglage à activer. Sous **Windows** (WebView2), Paramètres → Général → Performance propose « Hardware acceleration (GPU) » (activée par défaut, effet au prochain lancement ; désactivée = `--disable-gpu`). Ce réglage Windows n'a été que relu, pas exécuté. Essai écarté : annoncer `Safari/605.1.15` dans le user-agent rendait YouTube très lent et incomplet.

## Icônes de sites

La barre latérale et la page d'accueil affichent l'icône de chaque site (`favicon.js` relève le `<link rel="icon">` déclaré par la page, ou `/favicon.ico`). Aucun service tiers n'est interrogé : l'icône est chargée depuis le site déjà visité, sans referrer, et l'URL est mémorisée par hôte dans `favicons.json` (sans icône : initiale colorée ; l'icône remplace l'initiale). Les signets importés sont repliés par défaut dans la barre latérale.
