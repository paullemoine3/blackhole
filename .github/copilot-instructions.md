# Instructions Copilot — BlackHole

Navigateur Rust (winit + wry) : WKWebView sur macOS, WebView2 sur Windows. **Les deux plateformes doivent toujours cohabiter : ne jamais dégrader l'une en travaillant sur l'autre.** Répondre en français, de façon concise.

## Architecture (clean architecture)
`domain` → `application` → `infrastructure` → `app` (composition root : winit + wry). Les dépendances vont uniquement vers la gauche.
- `domain` : types et règles métier, sans I/O ni dépendance système.
- `application` : cas d'usage (`Service`), orchestre le domaine via des traits.
- `infrastructure` : persistance, réseau, catalogue d'extensions (`catalog.rs`), mise à jour (`updater.rs`), import, coffre chiffré.
- `app` : fenêtres, vues web (`views.rs`), IPC (`ipc.rs`), état (`main.rs`), UI HTML/JS dans `crates/app/ui/`, harnais `devtools.rs`.
- `macos-url`, `macos-webext` : intégration système. `macos-webext` contient `lib.rs` (macOS), `win.rs` (Windows, WebView2) et un stub pour les autres cibles ; les trois exposent la même API `ExtensionHost`.

Mettre la logique testable dans `domain`/`application`/`infrastructure`, pas dans `app`.

## Code spécifique à une plateforme
- Séparer par `#[cfg(target_os = "macos")]` et `#[cfg(windows)]` ; chaque branche est autonome. Ne jamais mettre de code Windows dans une branche macOS ni l'inverse.
- Toute nouvelle fonctionnalité plateforme : fournir les deux branches (ou un repli explicite documenté) et garder le stub pour les autres cibles.
- Imports spécifiques : les placer dans le bloc `cfg` correspondant (sinon avertissements `unused_imports`/`dead_code` sur l'autre plateforme).
- Exemples : `ExtAction` (macOS : popover natif ; Windows : vue popup + scrim `extpopup.html`), fenêtre sans décorations + boutons dans `ui/bar.html` sous Windows (`body.windows`), `backdrop.rs` (softbuffer) contre les bords blancs, icône via `build.rs` (`winresource`).
- Windows : toutes les vues d'un même dossier de données doivent avoir les mêmes réglages moteur (extensions activées, GPU, arguments) → toujours passer par `views::builder()` (il fixe aussi `--process-per-site`).
- Mémoire : chaque vue = un renderer. Pages de shell rares (`library_panel`, `easel_panel`, `splitbar`) = `LazyView` créées à la demande (dans `ensure_panels`) et libérées après 60 s masquées ; les overlays (hotzone, flyout, palette) sont recréés au-dessus des nouveaux onglets via `raise_palette` (différé, `flush_raise`). Plafond `max_live_tabs` (domain `overflow_to_release`), préchargement des voisins (`preload_next_tab`), cible mémoire WebView2 « Low » sur les onglets masqués (`set_view_background`, rien à faire sur macOS). Vues de réserve (`spares`) : réglage `spare_views` (0, 1 ou 2 ; défaut 2, ~50 Mo chacune).
- **Parité systématique :** toute modification ou demande réalisable sur les deux OS est appliquée aux deux dans la même passe (logique commune dans domain/pplication/HTML, branches cfg pour le natif). Si un côté est impossible ou non testable, le dire explicitement et laisser un repli, jamais l'ignorer en silence. Ex. : un popout d'extension s'ouvre en popup sur Windows (show_ext_popup) et en popover natif sur macOS (show_native_popup).
- Pas de build macOS complet sous Windows (pas de SDK Apple), mais une vérification de types est possible : `rustup target add aarch64-apple-darwin`, puis `cargo clippy --workspace --all-targets --target aarch64-apple-darwin` avec `CC_aarch64_apple_darwin`/`AR_aarch64_apple_darwin` pointant vers un faux compilateur qui crée des fichiers vides (seuls les scripts de build C en ont besoin). La faire après toute modification sous `cfg(target_os = "macos")` ; le comportement reste non testé sur macOS, le signaler.
- MSRV `rust-version = "1.80"` : pas d'API plus récente (ex. `as_chunks`, 1.88) même si clippy la suggère.

## Sécurité et qualité
- `unsafe_code = "forbid"` dans le workspace, sauf crates `macos-url` et `macos-webext`.
- Navigation : seuls http(s) sont autorisés dans les onglets (plus les schémas d'extension `webkit-extension`/`safari-web-extension`/`chrome-extension`). Les vues d'UI ne naviguent jamais hors de `about`/`data`.
- Téléchargements : HTTPS uniquement, hôtes attendus, **SHA-256 épinglé** vérifié (catalogue, mises à jour). Une URL/hash par plateforme via `CatalogEntry::source()` (champ `windows`).
- Écritures de fichiers atomiques ; UI avec CSP et `textContent` (pas d'`innerHTML` sur des données externes) ; aucune télémétrie.
- `cargo clippy --workspace --all-targets` doit être sans avertissement, y compris `cargo clippy -p blackhole --features devtools`.
- Commentaires seulement quand ils clarifient un « pourquoi » non évident.

## Tests
Commandes de base (PowerShell : ajouter `$env:Path += ";$env:USERPROFILE\.cargo\bin"`) :
```
cargo clippy --workspace --all-targets
cargo test --workspace --locked
```
- Tests unitaires dans le crate concerné (domaine/application/infra en priorité). Tout nouveau comportement logique reçoit un test ciblé.
- **Scénarios UI Windows** (feature `devtools`, harnais `crates/app/src/devtools.rs`) :
  - `scripts\test-ui.ps1` : navigation, palette, paramètres, animations (sidebar, palette), bouton de mise à jour, maximize/restore, onglets façon Arc, recherche dans la page. 20 captures dans `%TEMP%\bh_shots`.
  - `scripts\test-ext.ps1` : installe Dark Reader depuis le catalogue et vérifie l'injection (utiliser une page claire comme info.cern.ch ; example.com gère déjà le mode sombre).
  - Profil jetable `%TEMP%\bh_data` ; le script lit `%TEMP%\bh_script.txt`.
  - Protocole du script : `wait MS`, `shot NOM`, `shots NOM` (fenêtre des paramètres), `cmd JSON` (un `UiCommand`), `eval chrome|bar|home|palette|peek|settings|content JS`, `quit`. Les contrôles lisent les `eval` par indice : **ajouter un `eval` décale les indices suivants**, mettre à jour les `Check`.
  - Les captures sont prises à l'écran : la fenêtre doit être visible et non recouverte. Vérifier visuellement les images.
  - Si l'appli reste en vie après un timeout, arrêter le process par son id (`Stop-Process -Id`) avant de relancer.
- **Easel** (page `ui/easel.html`, affichée soit dans la fenêtre dédiée « All Easels », soit dans la zone de contenu comme un onglet via `easel_panel`/`easel_tab` — `home_shown()` est vrai pendant un onglet Easel ; tout changement d'onglet doit remettre `easel_tab = false` ; logique dans `domain/easel.rs`, persistance `EaselStore`) : `scripts/test-easel.sh` (macOS) et `scripts\test-easel.ps1` (Windows), cibles devtools `eval easel`, `eval etab` (onglet) et `shote NOM`. Capture live : vue native enfant de la fenêtre qui héberge l'Easel (fenêtre principale en onglet, positions décalées de l'origine du contenu ; `EaselLayout.embedded` distingue la page émettrice ; `views::live_view`, recadrée par `ui/live_crop.js`, zone choisie par `ui/live_picker.js`), positionnée via `EaselLayout` ; elle doit être déclarée/dropée avant la fenêtre ; cible devtools `eval live`. Ne pas appeler `focus_window()` sur la fenêtre principale depuis un handler : panique winit sur macOS.
- **Sessions par espace** : `views::Store` (`Main` = espace id 1, `Space(id)`, `Private`) choisit le stockage de cookies de chaque vue d'onglet (macOS : `DataStore` de `macos-webext` / `with_data_store_identifier` ; Windows : `with_profile_name`). Toute nouvelle vue de page doit recevoir le `Store` de son espace (`store_of_tab` / `store_of_space`) ; les vues de réserve (`spares`) sont rangées par `Store`. Les cookies d'un test e2e persistent dans le stockage WebKit : utiliser des noms uniques.
- **Écritures disque** : jamais de `std::fs::write` sur le thread UI pour des données utilisateur ; passer par `infrastructure::write_later` (thread d'arrière-plan, `flush_writes` avant relecture et à la fermeture). Ne pas multiplier les `ui_state()` : les événements de page marquent `ui_stale`. Le bloqueur (`PrivacyPolicy::set_blocking`, `Settings::tracker_blocking`) est partagé par tous les clones de la politique.
- **Préférences d'interface** (mode de la barre latérale…) : à enregistrer dans `Settings` (champ `#[serde(default)]`) dès qu'elles changent, pour survivre au redémarrage.
- **macOS** : `scripts/test-webext.sh` (installe uBOL, vérifie le blocage actif/inactif, retrait) et le harnais `devtools` avec captures CoreGraphics (`/tmp/bh_script.txt` → `/tmp/bh_shots/`).
- Toute nouvelle fonctionnalité d'UI : ajouter un scénario (et un `Check`) dans le script de la plateforme concernée, avec capture ; toute animation : tester l'état intermédiaire et le mode animations désactivées.

## Extensions
- macOS : `WKWebExtensionController`. Windows : `ICoreWebView2Profile7::AddBrowserExtension` (zips Chromium dépaquetés dans `<données>/webext/unpacked/<id>`, `catalog::remove` supprime aussi ce dossier).
- Bitwarden : build Firefox sur macOS, build Chromium (`chromium`/`windows` dans `CatalogEntry`) sur Windows.
- Ajouter une extension au catalogue : URL de la release officielle, télécharger puis calculer le SHA-256, ajouter une entrée avec test (`catalog.rs` vérifie longueurs de hash et https).
- Limites WebView2 : pas de badge, pas d'onglet actif exposé aux extensions ; le popup est une vue `chrome-extension://<id>/<popup>` sur le même profil.

## Pages d'erreur et User-Agent
- Page « Site injoignable » (`ui/unreachable.html`) : via `PageLoadEvent::Failed` (vendor/wry ; WebView2 non testé) ou après `CONNECT_MAX` (20 s) ; test macOS `scripts/test-unreachable.sh`.
- User-Agent macOS : `views::user_agent_for` (via `wry::USER_AGENT_RESOLVER`, à chaque navigation principale) force l'UA Safari (requis par Meet) sauf sur `docs.google.com`, où le token Safari rend Docs flou. Les demandes de nouvelle fenêtre (pas de cadre cible) ne changent pas l'UA de l'onglet ouvrant. Windows non concerné.

## Menus et vocabulaire
- Raccourcis affichés : `bhKeys('mod+shift+e')` → « ⌘⇧E » sur macOS, « Ctrl+Shift+E » ailleurs ; `bhKeyParts` pour des `<kbd>` (`ui/keys.js`, injecté par `views::ui_view`, `settings_view`, `panel_view`). Jamais de `⌘`/`Ctrl` en dur. Les actions de palette passent `keys` (spécification) plutôt que `hint`.
- Menus (`cmd: 'menu'`, rendus par `palette.html`) : `icon` est un nom de la table `ICONS` (SVG en traits) ; `confirm: '…'` arme l'élément au premier clic. Actions de palette : `confirm` demande une seconde Entrée.
- Vocabulaire : « quick access » (tuiles épinglées), « put to sleep » (`close_instance`, l'onglet garde sa place ; c'est aussi le bouton « − » d'un onglet de dossier), « close » (`close`, archive), « delete » (conteneur + contenu, avec confirmation). Casse de phrase.
- Accès rapide : « Close tab » et la commande `close` (gardée côté Rust) ne retirent jamais une tuile, ils la mettent en veille ; seul « Remove from quick access » la retire. Clic molette (`auxclick`) : veille pour un onglet d'accès rapide ou de dossier, `close` sinon. Pas de bouton son sur les tuiles (il masquait l'icône).
- « Lock URL » a été retiré (domaine, IPC, interface) ; un ancien champ `locked` de `workspace.json` est ignoré au chargement (test dans `infrastructure`).
- PiP : `pip_returned` ne fait que libérer le suivi ; quitter la fenêtre flottante ne change jamais d'onglet (fermer et « retour à l'onglet » sont indiscernables pour la page, et fermer ne met pas en pause sous WebView2).
- Fenêtres Réglages et Easel : `panel_window_attributes` (Windows : sans bordure + ombre + icône ; macOS : barre de titre transparente, 84 px réservés aux boutons système). La page dessine la barre du haut (38 px, `--tint`/`--glow` de la couleur de l'espace, boutons réduire/agrandir/fermer, glisser = déplacer, double-clic = agrandir) via `UiCommand::PanelWindow { window, action }` ; l'état « agrandi » suit `WindowEvent::Resized`. Dans l'Easel embarqué en onglet, cette barre est masquée.
- Suppression d'un espace ou d'un dossier : onglets archivés (`archive_tab` cherche dans tous les espaces) puis `drop_view`. Renommer un dossier : `UiCommand::EditFolder` → `bh.editFolderId`.
- Annuler une suppression : instantané domaine (`snapshot_space` / `snapshot_folder` → `Removed`) gardé 15 s dans `Running::undo`, toast avec bouton (`bh.toast(m, {label, run})`), `UiCommand::UndoRemoval` → `undo_removal` (retire aussi les entrées d'archive). Les routes « Air Traffic » d'un espace supprimé ne reviennent pas.
- Menu d'onglet : `reload_tab`, `duplicate_tab`, `copy` (copie via la palette), `move_tab_to_space` (la vue est libérée : nouveau profil de cookies), `close_other_tabs` (onglets libres seulement, archivés). Menu d'espace : `set_space_color` (`#rrggbb`, pastilles `color` dans les menus).
- Palette : actions `key:<raccourci>` (rejouées par `shortcut`), `open_history` (`library_jump` → la Bibliothèque ouvre la section une fois).
- Sonde média : une vidéo coupée par le site compte si elle joue, ne boucle pas et couvre ≥ 25 % de la fenêtre (Twitch démarre muet) ; `MEDIA_MUTE_JS` rend alors le son à ce qui joue.

## Liens, boucliers et capture en direct (0.7.18)
- Liens dans les pages (`ui/page_guard.js`, cadre principal, capture avant la page, `composedPath` pour traverser les shadow roots) : clic molette ou Ctrl/⌘+clic → `PageEvent::OpenLink` → `open_in_background` (l'onglet s'ouvre en haut de la liste et charge, la page affichée reste, Air Traffic respecté sans changer d'espace) ; clic droit → `PageEvent::LinkMenu` → `link_menu` (menu à la position de la page, via `view_origin` + `show_menu`). Les entrées du menu sont des `UiCommand` (`open_link` avec `target` tab/window/private, `open_peek`, `copy`) : une page ne peut ni ouvrir de fenêtre ni choisir un Easel.
- Boucliers façon Brave : `PrivacyPolicy::with_ads(trackers, ads)`, interrupteurs `set_blocking` / `set_ad_blocking` et `Shields { hide_ads, video_ads, cookie_banners }` partagés par les clones (atomiques). `ui/shields.js` reçoit la configuration à la création de la vue (`views::shields_script`) : feuille construite (CSP) avec des sélecteurs génériques connus (AdSense/GPT, Taboola, Outbrain, rendus de pub YouTube ; CMP OneTrust, Cookiebot, Quantcast, Didomi, Usercentrics…), défilement rendu si un bandeau était là, saut des pubs YouTube (`.ad-showing` → fin de la pub + bouton Skip, sans toucher au son). Les vues de réserve sont recréées après un changement. Tests : domaine (listes et clones), application (persistance), infrastructure (listes intégrées sans domaine de site).
- Capture en direct : `live_picked` → menu « Add the live area to » (Easels de l'espace, puis New Easel) → `LiveCaptureTo { easel }` → `capture_live(…, easel)`.
- Barres de défilement : `ui/theme.js` (feuille construite, pouce translucide, couleur de l'espace au survol), injecté dans `ui_view`, `settings_view`, `panel_view`.
- `test-easel.ps1` : un `eval` qui renvoie `__skip…` est exclu des indices des `Check`.
- Meet et Google Agenda (sans OAuth) : l'utilisateur colle l'« adresse secrète au format iCal » de chaque agenda (Réglages > Calendar). `infrastructure::calendar` : `is_valid_source` (https, `calendar.google.com`, `/calendar/ical/…​.ics`), `CalendarSources` (fichier chiffré `calendars.bin`, même clé que le coffre via `Vault::named` / `read_json` / `write_json`), `fetch` (timeout, pas de redirection, 16 Mo max), `parse(ics, from, to)` (lignes repliées, TZID via `chrono-tz`, heures flottantes = fuseau local, RRULE + EXDATE via `rrule`, occurrences déplacées/annulées via RECURRENCE-ID, journées entières et événements sans salle Meet ignorés). `domain::meetings` : `Meeting`, `find_meet_link`, `room_code`, `upcoming`, `next_soon`, `starting`. App `meet.rs` : `Calendar` (thread de synchro, statut, carte), `service_calendar` (appelé par `about_to_wait`), `open_meet`, `meeting_actions` (palette : New Google Meet, Join …), `calendar_json` (Réglages, sans les adresses), `handle_calendar`. `toast_action` affiche un toast avec bouton (Join, Undo).
- Outils de développement : `with_devtools(true)` pour les vues d'onglet seulement (les pages d'interface restent en debug), feature `devtools` de wry dans `crates/app/Cargo.toml` ; raccourci `devtools` dans `shortcuts.js` (F12 / Ctrl+Maj+I / ⌘⌥I), action de palette `key:devtools`, entrée « Inspect page » du menu de l'onglet affiché.
- Rapport de confidentialité (bouclier de la barre latérale) : `UiCommand::PrivacyReport { x, y }` → `privacy_report` : `BlockLog::for_site(site)` (par site, 100 sites × 100 hôtes max), classement pub/traqueur via `PrivacyPolicy::evaluate`, note `privacy_grade(trackers, ads)` (A à E, un traqueur pèse deux pubs), menu ancré.
- Rangement automatique : domaine `tidy_suggestion(min)` (onglets libres groupés par `site_of_host`, sous-domaines et suffixes pays repliés), `tidy` (réutilise un dossier racine du même nom), `untidy`. App : `tidy_json` (carte dès 6 onglets libres, site à 3 onglets), commandes `Tidy` / `Untidy` / `DismissTidy`, action de palette `tidy`.
- Copilote de réunion : `Meeting.agenda` (description sans le bloc d'accès Google `-::~`) et `attendees` (CN ou adresse, ressources exclues) lus par `calendar::parse` ; `open_meet` d'une salle de l'agenda → `open_meeting_notes` (Easel « Notes · titre · jour », note `notes_template`, fenêtre Easel).
- Veille de zone : `domain::watch` (`Watches::add/due/record/check_soon/mark_read`, première lecture = référence, `Reading::Changed { before, after }`), `infrastructure::store::WatchStore` (`watches.json`, URL http(s) seulement), `app/watching.rs` (`watch_area`, `service_watches` : vue cachée `views::live_view`, `read_js(selector)`), Library « Watched Areas », toast « Open » sur changement.
- Comparateur : `views::COMPARE_JS` (`ui/compare.js`), `app/compare.rs` (`compare_start`, `service_compare`, attente 1,5 s), palette `mode: 'compare'` (`showCompare` / `drawCompare`, 5 colonnes cochées par défaut, prix le plus bas et meilleure note en vert), entrée « Compare with the other tabs » du menu d'onglet.
- Réseau : `ureq` avec `native-certs` (magasin de certificats du système) ; les téléchargements du catalogue et des mises à jour gardent leur vérification SHA-256.

## Raccourcis, recherche et glisser-déposer
- Windows : les vues d'onglet désactivent les raccourcis navigateur de WebView2 (`with_browser_accelerator_keys(false)` dans `views::build_tab`), sinon Ctrl+F/G/P, F5, Alt+←/→ n'atteignent jamais la page. `ui/shortcuts.js` les reprend (F5 → `reload`, Ctrl+F5 → `r`, Alt+←/→ → `[`/`]`).
- Ctrl/⌘+F/G sont traités après la page (écouteur en phase bulle) et seulement si elle ne les a pas pris (`defaultPrevented`) : Google Docs garde sa propre recherche.
- Barre de recherche (`ui/find.js`) : surlignage par l'API CSS Custom Highlight (`::highlight(bh-find)`), sans toucher la sélection ni le focus ; repli `window.find` sinon.
- Une vue masquée mais pleine taille capte encore les glisser-déposer de la page (macOS) : la palette, la Bibliothèque et le panneau Easel sont parqués en 1×1 quand ils sont masqués (`palette_rect`, `place_panels`).
- Animation de la sidebar : pendant le mouvement, la vue de page garde sa largeur finale et ne fait que glisser (`sliding`) ; redimensionner une WebView à chaque image la fait se remettre en page et saccader. La boucle passe en `ControlFlow::Poll` cadencé par `FRAME` (8 ms, `thread::sleep` précis) : le minuteur système derrière `WaitUntil` (15,6 ms sous Windows) rend les images irrégulières.
- Cartes lecteur et téléchargement (`renderMedia`, `renderDownloads` dans `ui/index.html`) : mises à jour en place (signature / nœuds indexés par id), la carte lecteur reste sur le même onglet tant qu'il est listé. Côté Rust, `media::Linger` garde le dernier état avec média 5 s (pubs, lecteur recréé) ; la sonde `MEDIA_PROBE_JS` compte une vidéo en mise en mémoire tampon comme en lecture.

## Release, installeur et mise à jour
- Workflow `.github/workflows/release.yml` : tests (Windows + macOS), installeur Inno Setup + zip portable, binaire macOS universel (`scripts/package-macos.sh`), `SHA256SUMS.txt`. Publie sur push `main` quand `v<version de Cargo.toml>` n'a pas de tag, ou sur tag `vX.Y.Z` (doit correspondre à `Cargo.toml`).
- Version : `Cargo.toml` (+ `Cargo.lock`). Pour publier : incrémenter, committer, pousser.
- Installeur Windows local :
  ```
  cargo build --release -p blackhole
  iscc /DAppVersion=X.Y.Z /DExePath=..\target\release\blackhole.exe scripts\windows-installer.iss
  ```
  Sortie dans `dist\`. L'icône vient de `scripts/make_icon.py` (PNG macOS + `.ico` Windows).
- Mise à jour dans l'app (`updater.rs`) : bouton `#update` en bas de la sidebar (le flyout de survol est une 2e vue chrome, recréée avant la palette dans `raise_palette`) ; Windows lance l'installeur en silencieux, macOS remplace le `.app`. Désactivée pour les builds `cargo` (dossier `target`) et `devtools`.

## Environnement PowerShell (Windows)
- Pas de `&&`/`||` : utiliser `;` et `if ($?)`. Le code de sortie 1 peut venir du stderr de cargo sans gravité.
- Éditer les fichiers avec les outils d'édition, jamais via `-replace`/`Set-Content` (BOM et CRLF cassés). Garder des `old_str` assez longs pour être uniques.
- Ne jamais tuer un process par nom : `Stop-Process -Id <n>`.

## Avant de terminer une tâche
1. Clippy et tests passent (workspace + feature `devtools`).
2. Scénarios UI Windows rejoués si l'interface a changé ; captures regardées.
3. Vérifier que le code macOS n'a pas été modifié par accident (cfg corrects).
4. README et ces instructions mis à jour si l'architecture, les scripts ou les commandes changent.
5. Ne pas commiter sans demande ; si commit, ajouter le trailer `Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>`.
6. Résumer en français, avec les limites (non testé sur macOS, etc.).
