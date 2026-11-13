# Consignes pour les agents — BlackHole

Ces consignes s'appliquent à tout le dépôt. Répondre en français, de façon concise.
Ce fichier est l'index de référence (règles courtes) ; `.github/copilot-instructions.md`
détaille les mêmes sujets (protocole des scripts UI, mémoire, Easel, release). En cas de
doute, suivre le plus strict. Voir aussi `README.md` (commandes), `docs/macos.md` et `docs/design-system/` (tokens, composants et règles visuelles de l’interface).
Maintenance : mettre à jour les deux fichiers ensemble quand l'architecture, les scripts
ou les commandes changent.

## Projet et architecture

BlackHole est un navigateur Rust 2021 utilisant winit et wry : WKWebView sur macOS,
WebView2 sur Windows. Les deux plateformes sont de premier rang.

- `crates/domain` : types et règles métier, sans I/O ni dépendance système.
- `crates/application` : cas d'usage et traits de persistance ; dépend du domaine.
- `crates/infrastructure` : persistance, réseau, import, coffre chiffré, catalogue
  d'extensions et mises à jour ; implémente les traits de l'application.
- `crates/app` : binaire `blackhole`, composition, fenêtres, vues et IPC.
  `src/main.rs` orchestre l'état, `src/views.rs` construit les vues,
  `src/ipc.rs` définit les commandes et `ui/` contient le HTML/CSS/JS embarqué.
- `crates/macos-url` et `crates/macos-webext` : intégrations natives. Malgré son
  nom, `macos-webext` héberge aussi les extensions Windows et un stub ailleurs.
- `vendor/wry` : dépendance locale via `[patch.crates-io]`. Préserver le pont
  IPC `__bhipc`, qui évite les collisions avec le global `ipc` des pages web.
- `crates/android` (cœur Rust de la version Android, `cdylib` appelé par JNI) et
  `android/` (coque Kotlin, projet Gradle) : voir la section Android et `docs/android.md`.

Garder les dépendances orientées vers les couches internes. Placer la logique
testable dans domain/application/infrastructure plutôt que dans le binaire.
L'interface est embarquée avec `include_str!` ; la reconstruire après modification.

## Invariants d'implémentation

- Isoler code et imports natifs avec `#[cfg(target_os = "macos")]` et
  `#[cfg(windows)]`. Appliquer les changements aux deux plateformes lorsque
  possible ; documenter les replis et les plateformes non vérifiées.
- Conserver la même API `ExtensionHost` pour macOS, Windows et le stub.
- Sous Windows, construire les vues avec `views::builder()` pour partager les
  réglages moteur compatibles au sein d'un dossier de données.
- Toute vue de page utilise le `Store` de son espace (`store_of_tab` /
  `store_of_space`) ; préserver l'isolation des cookies et du mode privé.
- Respecter le plafond d'onglets vivants, les vues différées et leur libération.
  Éviter les vues natives persistantes inutiles et les I/O sur le thread UI.
  Chaque vue = un renderer : pages de shell rares en `LazyView` (créées dans
  `ensure_panels`, libérées après 60 s masquées), plafond `max_live_tabs`,
  vues de réserve via le réglage `spare_views` (0–2), overlays recréés par `raise_palette`.
- Ne pas appeler `focus_window()` sur la fenêtre principale depuis un handler
  (panique winit sur macOS). La vue live de l'Easel doit être déclarée/droppée
  avant la fenêtre qui l'héberge. Fermer Réglages/Easel passe par `retire_window` : fenêtre
  masquée tout de suite, détruite au tour de boucle suivant (détruire la fenêtre clé fait planter
  AppKit au changement de fenêtre clé). Dans vendor/wry (WKWebView), jamais d'`unwrap()` sur
  `webview.URL()` : une vue qui n'a encore rien chargé n'a pas d'URL (`panic = "abort"`).
  Page d'une fenêtre secondaire : `views::panel_view` (`build_as_child`, redimensionnée sur
  `Resized`), jamais `build()` : sur macOS wry remplace la vue de contenu winit et winit panique
  (abort) quand la fenêtre perd le focus (`windowDidResignKey` → `reset_modifiers`).
- Onglets façon Arc (logique dans `domain/workspace.rs`, testée) : nouvel onglet en haut
  de la liste non épinglée ; à la fermeture, le partenaire de split puis l'onglet vu juste
  avant (`recent`, non persisté) puis le voisin prend le relais ; ⌘W sur un onglet épinglé
  ou de dossier appelle `release_tab` (il reste dans la barre). Dupliquer : `Workspace::duplicate_tab`,
  jamais `open` (qui réutilise l'onglet ouvert sur la même adresse). Libérer une vue d'onglet
  via `drop_view` (sonde média et suivi du PiP compris). Un PiP n'est lancé que pour
  l'onglet quitté qui n'est plus à l'écran (pas l'autre volet d'un split).
- Vues de réserve : créées sans URL (`spare_tab_view`), jamais sur `about:blank`, sinon
  cette page reste la première de l'historique de l'onglet (Précédent y mène).
- Accès rapide (onglets épinglés) : fermer (menu, ⌘W, clic molette, commande `close`) ne
  les retire jamais de la liste, ça les met en veille ; seul « Remove from quick access » les
  retire. Clic molette : veille pour un onglet d'accès rapide ou de dossier, fermeture sinon.
  Pas de bouton son sur les tuiles. La fonction « Lock URL » a été retirée (un ancien champ
  `locked` dans `workspace.json` est ignoré au chargement).
- PiP : le bouton « retour à l'onglet » de la fenêtre flottante affiche l'onglet de la vidéo
  (elle sort en lecture ; WebKit ne prévient la page qu'après l'animation, après la sonde : `pip_faded`,
  `pip::LEFT_EVENT_GRACE`) ; fermer la met en pause et ne change rien (`domain/pip.rs`,
  `pip_left`), sur macOS comme sous WebView2 (vérifié avec de vrais clics : la pause arrive
  avant `leavepictureinpicture`). Exception WebView2 : un flux en direct (`srcObject`, champ
  `stream`) n'est pas mis en pause à la fermeture, donc rien ne change d'onglet pour lui.
  Barre latérale masquée : un clic molette (`open_in_background`) la fait sortir `LINK_REVEAL`.
- Adresse saisie dans un onglet (`navigate`) : `Running::pending` la montre aussitôt dans la barre,
  avec la ligne de progression, jusqu'au début de la page, à l'échec, à l'invite d'authentification
  ou au téléchargement (`LOADING_MAX` au plus) ; sans elle un appareil lent du réseau local laissait
  l'ancienne adresse une vingtaine de secondes. Menu du clic droit de la barre latérale
  (`createMenu`) : « New black hole tab » (action `burner_tab`). Test : fin de `test-ui.ps1`
  (`ad1`, `ad2`, `bm1`, `bm2`).
  `paused` = une pause vue pendant le PiP (`media_devices.js`), pas `video.paused` à la sortie :
  WebKit met en pause une vidéo muette (tuiles d'un appel) qui revient dans un onglet masqué.
- Fenêtres Réglages et Easel : `panel_window_attributes` (sans bordure sous Windows, barre de
  titre transparente sur macOS) ; leur page dessine la barre du haut (38 px, teinte de l'espace,
  boutons) et la pilote par `UiCommand::PanelWindow`.
- Zoom de page : par site (`site_key`, sans `www.`), en pourcentage (`ZOOM_STEPS`, 100 jamais
  stocké, `MAX_SITE_ZOOMS`) dans `Settings::site_zoom`, gardé entre les lancements
  (`set_site_zoom`) ; `zoom_of` / `apply_zoom` dans l'app, appliqué à chaque changement d'adresse
  et à tous les onglets du site. Onglet trou noir : en mémoire seulement (`Running::zoom`).
- Pour les données utilisateur, utiliser `blackhole_infrastructure::write_later`
  et `flush_writes` avant relecture ou fermeture ; conserver les écritures atomiques.
- Enregistrer les préférences persistantes dans `Settings`, avec
  `#[serde(default)]` pour les nouveaux champs sérialisés.
- Confiner `unsafe` aux crates d'intégration `macos-url` et `macos-webext` ;
  conserver `unsafe_code = "forbid"` dans les autres crates du workspace.
- Préserver les restrictions de navigation : http(s) et schémas d'extensions
  autorisés pour les onglets ; aucune navigation externe des vues d'interface.
- Garder CSP, insertion des données externes par `textContent`, absence de
  télémétrie, téléchargements HTTPS sur hôtes attendus et vérification SHA-256.
  Les sources du catalogue d'extensions ont des hashes épinglés par plateforme.

## Commandes et validation

Depuis la racine du dépôt :

```sh
cargo run -p blackhole
cargo run --release -p blackhole
cargo clippy --workspace --all-targets
cargo clippy -p blackhole --features devtools
cargo test --workspace --locked
```

La CI teste sur macOS et Windows. Pour un changement de code, exécuter les tests
pertinents et Clippy sans avertissement ; signaler les vérifications impossibles.
Depuis Windows, vérifier aussi le code macOS avec `cargo clippy --workspace --all-targets
--target aarch64-apple-darwin` (faux `CC`/`AR` pour la cible, voir copilot-instructions) ;
MSRV 1.80 : pas d'API plus récente. Animation de la sidebar : la page glisse à largeur
fixe, la boucle est cadencée à 8 ms (`FRAME`), ne pas revenir à un redimensionnement par image.
Ajouter un test ciblé pour tout nouveau comportement logique, de préférence dans
les couches internes, avec les tests unitaires existants.

Pour une modification d'interface, ajouter ou adapter les scénarios du harnais
`devtools` (`crates/app/src/devtools.rs`) et inspecter les captures :

- macOS : `bash scripts/test-webext.sh`, `bash scripts/test-easel.sh`,
  `bash scripts/test-errors.sh` (pages d'erreur, serveurs locaux, sans réseau),
  `bash scripts/test-pip.sh` (PiP et carte lecteur, vidéo CC0 de MDN),
  `bash scripts/test-library.sh` (Bibliothèque, cartes de téléchargement, HOME jetable),
  `bash scripts/test-notices.sh` (notices : toast de la barre latérale, capsule de la barre du haut),
  `bash scripts/test-popup.sh` (popup avec `opener`, Trusted Types, serveur local),
  `bash scripts/test-settings.sh` (Réglages et icônes ; `BH_SHOTS_ONLY=1 BH_PREFIX=avant-` pour les seules captures),
  `bash scripts/test-i18n.sh` (langue anglais ↔ français en direct, captures `i18n-*`),
  `bash scripts/test-blocking.sh` (blocage natif et sonde média, serveur local, étape `state`).
- Windows : `scripts\test-ui.ps1`, `scripts\test-ext.ps1`,
  `scripts\test-easel.ps1`, `scripts\test-features.ps1` (liens propres, trou noir, split pilote,
  surlignages, Recall).

Ces scripts lancent une application graphique, utilisent des profils temporaires
qu'ils peuvent effacer et certains nécessitent le réseau. Garder la fenêtre
visible pour les captures. Les assertions sont indexées sur les résultats
`eval` : ajouter un `eval` décale les indices suivants, donc mettre à jour les
`Check` du script. Protocole du script : `wait MS`, `shot NOM`, `shots NOM`,
`cmd JSON`, `eval <cible> JS`, `quit`. Pour une animation, vérifier aussi l'état
intermédiaire et le mode animations désactivées.

Sous PowerShell : enchaîner avec `;` (pas `&&`), éditer avec les outils d'édition
(pas `-replace`/`Set-Content`), arrêter un process par `Stop-Process -Id`, jamais par nom.

## Raccourcis, recherche et glisser-déposer

- Windows : les vues d'onglet désactivent les raccourcis navigateur de WebView2
  (`with_browser_accelerator_keys(false)`), sinon Ctrl+F/G/P, F5, Alt+←/→ n'atteignent pas la
  page ; `ui/shortcuts.js` les reprend. Ctrl/⌘+F/G passent d'abord par la page (Google Docs
  garde sa recherche) ; `ui/find.js` surligne via CSS Custom Highlight sans voler le focus.
- Une vue masquée pleine taille capte les glisser-déposer de la page (macOS) : palette,
  Bibliothèque et panneau Easel sont parqués en 1×1 quand ils sont masqués.
- Cartes de la barre latérale (lecteur, téléchargements) : mises à jour en place, jamais
  reconstruites à chaque rendu (sinon l'animation d'entrée rejoue = scintillement). La carte
  lecteur garde 5 s le dernier état connu (`media::Linger`) quand une sonde ne voit plus rien.
  Une carte de téléchargement terminé (ou échoué) a un bouton de fermeture (`dismiss_download`) ;
  elle sort par `dl-out` (`dlLeave`), comme une carte dont le délai est écoulé.
- Logo et splash de l'accueil (`home.html`, repris dans `mobile.html`) : logo calme au repos, qui
  avale ses étoiles au survol (`.brand:hover`) ou au toucher (`.gulp` sur Android) ; le retour au
  repos a ses propres transitions, plus lentes. Le splash finit en atterrissant sur le logo de la
  page (position mesurée par le script) ; sur Android, sans page d'accueil à l'écran, il s'efface.
  Maquettes et variantes écartées : `docs/logo/`.

## Menus et vocabulaire

- Raccourcis affichés : toujours via `bhKeys('mod+shift+e')` / `bhKeyParts` (`ui/keys.js`, injecté
  dans les pages d'interface), jamais `⌘` ou `Ctrl` en dur ; la palette lit le champ `keys` des actions.
- Icônes : une seule table SVG, `ui/icons.js` (injecté avec `theme.js` dans toutes les pages d'interface) :
  `bhIcon('trash', 'sm')`, `bhIconHTML(…)` pour le HTML en texte, `<i data-icon="gear" data-size="xs">` dans le
  balisage. Échelle xs 12 / sm 16 (défaut) / md 20 / lg 24, trait 1,5 px non mis à l'échelle ; zone de clic ≥ 24 px.
  Ni emoji ni glyphe de police pour une icône d'interface (seuls les emoji choisis pour un espace ou un dossier
  restent). Exception : boutons de fenêtre Windows en 10 px. Menus : `icon: 'trash'` = un nom de cette table.
- Tokens du design system (artifact « BlackHole Design System ») : variables `--bh-*` dans `ui/theme.js`
  (sous `:where()`, la page garde la main) ; les `--bh-*` d'espace suivent `--accent`/`--glow`/`--tint` posés par
  la page. Réglages (`settings.html`) : rendu sur place (`put`/`morph`, pas de `replaceChildren` qui casse les
  animations et le focus), `body.noanim` et `prefers-reduced-motion` respectés.
- Mots : « quick access » (tuiles épinglées), « put to sleep » (l'onglet garde sa place), « close »
  (archive), « delete » (un conteneur et son contenu). Casse de phrase, sauf noms propres (Easel, Library).
- Action irréversible : `confirm: '…'` sur l'élément de menu ou l'action de palette, ou deux clics en
  3 s dans les pages ; les onglets d'un espace ou d'un dossier supprimé vont à l'archive.
- Annuler : `Workspace::snapshot_space` / `snapshot_folder` avant suppression, puis `undo_removal`
  (toast « Undo », 15 s) ; les entrées d'archive correspondantes sont retirées.
- Palette : une action `key:<raccourci>` rejoue le raccourci (`shortcut`), avec `keys` pour l'affichage.
  Aucune ligne sélectionnée à l'ouverture (`sel = -1`) : Entrée sur l'adresse intacte recharge la
  page (`submit`) ; la première ligne est sélectionnée dès qu'on tape.
- Déplacer un onglet d'espace (`move_tab_to_space`) libère sa vue : elle est recréée sur le profil
  (cookies) du nouvel espace. « Copy link » passe par `execCommand('copy')` dans la palette.

## Liens, boucliers et capture en direct (0.7.18)

- `page_guard.js` (cadre principal) : clic molette / Ctrl-⌘+clic sur un lien → `open_link`
  (onglet en arrière-plan, `open_in_background`) ; clic droit sur un lien → `link_menu` (notre
  menu : nouvel onglet, nouvelle fenêtre, fenêtre privée, Peek, copier). Maj+clic droit, un champ
  de saisie et les iframes gardent le menu du moteur. Une page ne peut pas ouvrir de fenêtre
  BlackHole elle-même : seule la commande d'interface `open_link` a un `target`.
- Popups de connexion (OAuth, Google GSI) : `domain::popup::new_window` décide d'un `window.open`
  (`Popup` si `about:blank`, taille/position demandées ou page de connexion connue ; `Tab` sinon ;
  `Deny` pour `javascript:`/`data:`/`blob:`/`file:`). `Popup` → `NewWindowResponse::Allow` : fenêtre
  du moteur sur la configuration de l'onglet (même `Store`), `window.opener` gardé, fermée par
  `window.close()` (macOS : `webViewDidClose:` dans vendor/wry ; WebView2 : sa fenêtre popup). macOS :
  la popup partage les scripts de l'onglet, mais wry ignore ses messages IPC (`owner`). Scripts
  injectés dans les pages : jamais `innerHTML` (Trusted Types de accounts.google.com), nœuds et
  `adoptedStyleSheets`. Test : `scripts/test-popup.sh`.
- Popups d'extension (Windows) : `open_ext_popup` sous le bouton (`ExtAction`), ajustée au
  contenu (`POPUP_MIN` / `POPUP_MAX`), coins arrondis (`round_ext_popup`, à refaire à chaque
  redimensionnement). Une fenêtre qu'une extension demande depuis un onglet (déverrouillage
  Bitwarden) repart en `Msg::OpenTab`, jamais `OpenTabFrom` : `is_extension_popout` en fait une
  popup sous le bouton de l'extension (`ExtAnchors` envoyé par `bar.html`, `ext_anchor`), pas un
  onglet. Nom du bouton : `__MSG_clé__` lu dans `_locales/<default_locale>` (`win::localized`).
  Test : `scripts\test-ext.ps1`.
- Recherche sur la sélection (0.7.19) : clic droit sur du texte sélectionné → `link_menu` avec
  `text` (non fiable) ; `domain::selection` (`clean_selection` : 500 caractères, espaces
  normalisés, caractères invisibles retirés ; `selection_action` : adresse http(s) via
  `omnibox::web_address` ou recherche avec le moteur des Réglages ; `excerpt`). Entrée
  `open_link` `target: "front"`, et `background` (Ctrl/⌘+clic, clic molette dans `palette.html`)
  → `target: "tab"`. Sur un lien, les entrées du lien suivent.
- Boucliers : `PrivacyPolicy::with_ads` (traqueurs `blocklist.txt` + pubs `adlist.txt`, chacun
  son interrupteur) et `Shields` (masquer les pubs, sauter les pubs YouTube, masquer les bandeaux
  cookies) appliqués par `ui/shields.js` à la création de la page ; réglages dans `Settings`
  (`ad_blocking`, `shields`), commande `SetShield`. Ne jamais mettre le domaine d'un site dans
  `adlist.txt` ; ne pas couper le son sur YouTube (la pub et la vidéo partagent le `<video>`).
- Blocage des requêtes dans le moteur, tiers seulement (`domain::is_third_party`), par
  `views::block_natively`. macOS : `WKContentRuleList` compilées par `macos-webext/rules.rs`,
  décompte par la méthode privée `_webView:contentRuleListWithIdentifier:performedAction:forURL:`
  de vendor/wry → `with_content_blocked_handler`. Windows : `win::block_hosts`, filtres
  `AddWebResourceRequestedFilter` sur les seuls hôtes listés, décision par la politique à chaque
  requête (interrupteurs et exceptions en direct), réponse 403. `page_guard.js` reçoit une liste
  vide et ne remplace aucune fonction de la page (`fetch`, `setAttribute`, setters `src`) : ne pas
  y revenir, ces enveloppes pèsent sur chaque appel des applis lourdes.
- Sonde média : seuls les onglets qui l'ont signalé (`PageEvent::Media` de `media_devices.js`,
  chargement, changement d'adresse), qui jouent ou sont en appel sont interrogés ; pas de script
  périodique dans les onglets silencieux. Pas de PiP forcé sur un onglet en appel (`in_call`).
  La croix de la carte lecteur d'un appel (caméra, micro ou `call`) quitte l'appel (`CALL_HANG_UP_JS`,
  `__bhRtc.hangUp` : bouton « quitter » de la page, sinon pistes arrêtées et connexions fermées).
- Renderer mort (`Msg::Crashed` ; macOS : gestionnaire de wry, Windows : `win::on_renderer_gone`,
  `ProcessFailed`) : onglet affiché rechargé une fois (`CRASH_LOOP`), onglet caché libéré.
- Capture en direct : la zone choisie attend dans `live_pending` ; un menu demande l'Easel
  (ou un nouveau) → `LiveCaptureTo`. Sans Easel dans l'espace, il est créé directement.
- Barres de défilement des pages d'interface : `ui/theme.js`, injecté avec `keys.js`.
- Meet et agenda : règles pures dans `domain/meetings.rs` ; lecture iCal (fuseaux `chrono-tz`,
  récurrences `rrule`, EXDATE, RECURRENCE-ID, annulés) et téléchargement dans
  `infrastructure/calendar.rs` (https, `calendar.google.com` seulement, sans redirection, taille
  plafonnée) ; adresses secrètes chiffrées (`calendars.bin`, `Vault::named`). App : `meet.rs`
  (synchro en thread toutes les 10 min, carte « prochaine réunion », rappel, `open_meet` qui
  réutilise l'onglet Meet d'accès rapide ou de dossier sauf s'il est en appel). La page des
  Réglages ne reçoit jamais l'adresse secrète, seulement le nom de l'agenda. La carte part
  5 min après le début (`CARD_KEEP`, `next_soon`) ; son icône est une croix au survol
  (`meeting_dismiss` : cette réunion seulement, en mémoire).
- Outils de développement des pages : `with_devtools(true)` sur les onglets (feature `devtools`
  de wry, active en release), raccourci `devtools` (F12, Ctrl+Maj+I, ⌘⌥I) géré par
  `shortcuts.js` puisque les raccourcis natifs de WebView2 sont coupés.
- Rapport de confidentialité : `BlockLog::for_site` (décompte borné par site), `privacy_grade`,
  menu `privacy_report` ancré au bouclier ; pubs et traqueurs distingués par `PrivacyPolicy`.
- Rangement : `Workspace::tidy_suggestion` / `tidy` / `untidy` (`site_of_host`), carte `#tidy`
  dans la barre latérale (dès `TIDY_FROM` onglets libres), Undo par toast.
- Copilote de réunion : `Meeting::notes_template`, `agenda_of` ; Join (`open_meet`) propose par
  toast (`meeting_notes`) l'Easel « Notes · titre · jour », rouvert sans question s'il existe, et
  l'affiche en split à droite de l'onglet Meet (`easel_beside`, `open_easel_side` ; fenêtre Easel
  si aucun onglet n'est dans la salle). Réglage `meeting_notes`. Agenda : `calendar::parse` écarte
  les événements annulés et ceux refusés par le propriétaire (`meetings::is_on_calendar`).
- Veille de zone : `domain/watch.rs`, `WatchStore` (`watches.json`), `app/watching.rs` (une vue
  cachée à la fois, lecture à 8 s, abandon à 14 s, toutes les 30 min), section Library « Watched
  Areas » (`checking` : « checking… » jusqu'à la réponse, « il y a » qui avance sur place). Démarrée
  depuis le menu du sélecteur de zone (`WatchArea`). Vers l'Easel : menu « Add to » (Easels de l'espace,
  « New Easel »), `watch_to_easel { easel | new }`.
- Comparateur : `ui/compare.js` (JSON-LD Product, Open Graph, tableau de caractéristiques) lancé
  dans chaque onglet chargé, `app/compare.rs`, palette en mode `compare` (données de page
  insérées par `textContent` uniquement).
- `ureq` utilise les certificats du système (`native-certs`) : nécessaire derrière un antivirus
  qui inspecte le HTTPS ; les téléchargements gardent leur vérification SHA-256.
- Continuer sur l'autre appareil : règles pures dans `domain/handoff.rs` (code d'appairage 60 bits
  Crockford, `resume_url` pour la position YouTube/Twitch, `is_fresh`, `Seen`) ; réseau dans
  `infrastructure/handoff.rs` (thread, UDP `PORT` 47807, ChaCha20-Poly1305, clé de paire dérivée du
  code étiré et de deux moitiés aléatoires, `devices.bin` via `Vault::named`). Aucun port ouvert sans
  appareil appairé ni appairage en cours ; réseau local seulement, sans serveur ; seule l'adresse
  (et la position vidéo) voyage, jamais cookies ni session. App : `handoff.rs` (`Nearby`, désactivé
  en fenêtre privée, `Config::loopback` en build `devtools`), Réglages > Devices, « Continue on … »
  dans le menu d'onglet et la palette. macOS : `NSLocalNetworkUsageDescription` dans
  `package-macos.sh`. Échange Windows ↔ Mac non testé.
- `test-easel.ps1` : un `eval` qui ne fait que piloter l'interface renvoie `__skip…` et n'est pas
  compté dans les indices des `Check`.

## Liens propres, split pilote, trou noir, surlignages, Recall, Wormhole

Spécifications : `docs/specs/0.8.6-features.md`. Scénarios Windows : `scripts\test-features.ps1`
(contrôles nommés `cl1:`, `bh2:`… lus par leur nom, 13 captures `f01`–`f13` ; réseau requis ;
le Wormhole y est absent, couvert par le test en boucle locale de `handoff`). macOS : à écrire.

- Liens propres : `domain/clean_link.rs`, bouclier `clean_links` (`Shields`, bit 8), appliqué par
  `PrivacyPolicy::evaluate` avant le passage en https ; la cible d'une redirection est rejugée
  (jamais un contournement des listes). `Msg::LinkCleaned` compte pour le rapport de confidentialité.
- Split pilote : `domain/pilot.rs`, `app/pilot.rs` (`Running::pilot`, session). La page ne fait
  que signaler un clic (`PageEvent::PilotLink`, drapeau `__bhPilot` posé par le shell) : c'est le
  shell qui vérifie le pilote et évalue l'adresse. Reposer le drapeau à chaque chargement.
- Onglet trou noir : `Workspace::open_burner` / `is_burner` (session, jamais écrit), `Store::Burner`
  (macOS : magasin éphémère par vue ; Windows : profil InPrivate `BlackHole-burn-<lancement>-<id>`,
  `delete_profile` dans `drop_view`, `remove_burner_profiles` au démarrage). Jamais mis en veille
  (`discard_idle_tabs`), quatre au plus ; ni historique, ni archive, ni synchro, ni Recall, ni
  surlignages. Toute nouvelle fonction qui garde une trace d'une page doit tester `is_burner`.
- Surlignages : `domain/highlights.rs`, `HighlightStore` (`highlights.json`, `write_later`),
  `ui/highlights.js` (CSS Custom Highlight, aucun nœud modifié, rien ne tourne sans surlignage),
  `app/highlights.rs`. La page répond aux questions du shell (`capture`, `at`) ; l'adresse vient du
  moteur ; l'id donné par la page est vérifié contre les surlignages de cette page.
- Recall : `domain/recall.rs`, `infrastructure/recall.rs` (fil `bh-recall`, 64 fichiers chiffrés
  `recall/NN.bin`, clé `Vault::shared_key` lue sur ce fil), `app/recall.rs`. `Settings::recall`
  (faux par défaut) et `recall_exclude`. Lecture 4 s après `Loaded`, page à l'écran seulement ;
  jamais avec un champ de mot de passe, en privé, en trou noir, sur une adresse locale. Effacer
  l'historique passe par `recall_forget`.
- Wormhole : `domain/wormhole.rs`, `infrastructure/wormhole.rs` (`Outbound` / `Inbound`), messages
  `beam` / `beam_reply` / `chunk` dans `infrastructure/handoff.rs` (`Tag`, fenêtre `BEAM_WINDOW`).
  Fichier reçu seulement après accord (`Event::BeamOffer` → `Request::BeamAnswer`), écrit en
  `.part` puis renommé une fois le SHA-256 vérifié ; texte reçu jamais copié sans clic. Android :
  texte affiché, fichier refusé.
- Menu de page (0.8.7) : un clic droit hors lien et hors sélection envoie aussi `link_menu`
  (`page_guard.js`, écouteur en phase bulle sur `window`, seulement si la page n'a pas pris
  l'événement : Docs, Sheets, Figma gardent leur menu ; vidéo, audio, canvas, champs de saisie,
  iframes et Maj+clic droit gardent celui du moteur). `page_menu_items` : précédent/suivant/
  recharger, « Add this page to the Easel », capture en direct (`live_pick`), veille de zone
  (`watch_pick` : le sélecteur puis `watch_area` sans autre question), copier le lien propre,
  rechercher, imprimer, inspecter ; `image` (non fiable, http(s)) ajoute ouvrir/copier l'adresse.
  Les entrées qui visent « la page affichée » ne sont proposées que pour l'onglet actif ; ni
  Easel ni veille pour un onglet trou noir (règle `domain::verbs`, voir « Ponts entre fonctions »).
  Depuis 0.8.7 les menus de lien et de sélection se terminent aussi par les outils BlackHole
  (`page_tool_items` : Easel, zone en direct, veille), et seule une vidéo/un son avec les
  contrôles du moteur (`controls`) ou un plug-in gardent le menu du moteur : une vidéo que la
  page joue elle-même (aperçu au survol d'une vignette YouTube) reçoit le menu de page.
- Téléchargement depuis un lien (Windows) : WebView2 termine la navigation en « connection
  aborted » (9) juste avant `DownloadStarting` ; `Msg::Failed` attend alors `ABORT_GRACE`
  (`aborted`) et n'affiche « Site injoignable » que si aucun téléchargement ne suit. Barre
  latérale masquée : `Sidebar::reveal` la fait sortir 4 s (début et fin d'un téléchargement),
  la bande `hotzone` reste fine pendant ce temps (la page garde ses clics), `release_due` la
  range sauf si le curseur est dessus (la copie flottante envoie `peek` on/off).
- Adresse d'origine (0.8.7, comme Arc) : `Tab::home` (`Home { url, title }`) pour un onglet
  d'accès rapide ou de dossier (`Tab::kept`), posée quand il le devient (`settle`), retirée
  quand il redevient libre. `Workspace::go_home` quand sa page s'endort (`drop_view`, plafond
  d'onglets vivants) et à `restore` ; `set_home` (« Keep this page as its address »). Menu
  d'onglet : `tab_home` / `tab_set_home`, proposés si `away`. `workspace.json` : `home`,
  `home_title` ; la synchro identifie une tuile par son origine. Android : au démarrage
  seulement (le cœur mobile n'appelle pas `go_home` à la mise en veille).
- Onglet ouvert depuis une page (`Msg::OpenTabFrom`, `PageEvent::OpenLink`) : sa première
  requête porte le `Referer` de la page d'origine, réduit à son origine (`popup::referrer`,
  jamais de https vers http). Windows : `win::referrer_once` (filtre `WebResourceRequested`
  sur les documents jusqu'à `ContentLoading`, redirections comprises : un en-tête donné à
  `NavigateWithWebResourceRequest` est perdu à la première redirection) ; macOS :
  `load_url_with_headers`, non testé. Sans lui Reddit affiche la publication au lieu de l'image.
- Barre latérale (`ui/index.html`) : `render` ne reconstruit tuiles, dossiers, onglets et
  espaces que si `listSignature` a changé (un compteur, une carte ou un téléchargement ne
  remplacent plus les lignes : c'était le scintillement) ; les appels de la page elle-même
  (`render(state)`, `flush`) reconstruisent toujours. Tout nouveau champ dont dépend une ligne
  doit entrer dans `listSignature`. Dossier sans page ouverte dans son arborescence : `.fh.idle`.
- Dossier replié (comme Arc) : il montre encore les pages ouvertes sous lui et les dossiers
  qui en contiennent (`.folder.awake`, règles CSS de `ui/index.html`) ; les onglets en veille
  restent cachés. `reveal_tab` ne déplie donc plus que la section, jamais les dossiers.
- Mouvement (0.8.7), mêmes règles sur le bureau et sur Android : menus (lignes en cascade
  `--i` / `enter(…, délai)`, fermeture en fondu avant de masquer : `close(then)` de
  `palette.html` exécute la commande de l'entrée après le fondu, `#pop.out` sur mobile),
  toasts (montée, ligne du temps restant `.tbar` / `#toast-bar`, sortie vers le bas), chiffres
  qui défilent (`#blockedn.roll`, `roll()` sur mobile : l'ancien chiffre est dessiné par
  `::before`, le texte relu reste le nouveau), téléchargement terminé (coche, onde, barre
  estompée), dossiers qui se déplient depuis leur en-tête et s'y replient (`motion.play`,
  `foldStart` / `foldPlay`). Bureau seulement : la surbrillance de l'onglet actif glisse
  (`.tab-glide`) ; sur mobile le tiroir se ferme au choix d'un onglet. Tout passe par
  `body.noanim` / `body.still` et `prefers-reduced-motion` (les appels `animate()` testent
  `motion.calm()` ou `still`).
- Trace de mise au point : `BH_TRACE=1` sur un build debug écrit sur stderr les navigations,
  demandes de nouvelle fenêtre et événements de page (`trace`, jamais en release).
- `json!` de `ui_settings` : ajouter les nouvelles clés par `state["clé"] = …` (la macro atteint
  sa limite de récursion).
- MSRV : pas de `is_multiple_of` ni `as_chunks`, même si Clippy les suggère.

## Ponts entre fonctions (0.8.8)

Module `app/src/bridges.rs` ; règles pures dans `domain/verbs.rs` et `domain/signals.rs`.

- Registre unique « que peut-on faire de cet objet » : `domain::verbs` (`Target` : page, onglet,
  lien, sélection, surlignage, téléchargement, résultat ; `Context` lu par le shell, jamais par la
  page : `front`, `web`, `burner`, `error`, `paired`, `ready` ; `Verb` : ouvrir, garder dans l'Easel,
  zone en direct, veille, surligner, envoyer). `Running::verb_context(tab)`, `front_page_allows`.
  Menus de page/lien/sélection/surlignage, menu d'onglet (`tabs[].send`), palette et commandes
  (`capture_to_easel`, `live_pick_start`, `continue_on`) demandent tous cette règle : tout nouveau
  menu ou raccourci qui agit sur une page passe par elle (un onglet trou noir ne garde ni n'envoie
  rien, raccourcis compris).
- Recherche unique : la palette reçoit `found` (surlignages, zones surveillées, archive,
  téléchargements, `found_json`) et interroge Recall dès 3 lettres hors de son mode (réponse
  acceptée en modes `new`/`url`) ; groupes de 3 lignes sous un en-tête (`.ghead`, sauté par ↑/↓,
  `step`). Ctrl/⌘+Entrée sur une ligne qui a `keep` l'envoie dans l'Easel (pied `#fkeep`).
- L'Easel converge : `Easels::add_text` (note déjà remplie) ; commandes `link_to_easel`,
  `found_to_easel` (Recall, archive), `watch_to_easel` (avant/après d'une zone), `compare_to_easel`
  (bouton du comparateur), `beam_to_easel` (texte reçu). Carte en direct : bouton de veille
  (`easel_watch`, `watched` dans l'état de l'Easel) qui crée ou retire la veille de la même zone
  (`Watches::find`, `watch_url`).
- Envoyer partout : `send_items` (trois appareils au plus), menu de page (« Send this page to »,
  `continue_on`), lien (`send_link` : seule l'adresse voyage), surlignage (`beam_text`), Library >
  Downloads (`send_download`, menu `origin: "library"`, fichier par le Wormhole).
- Signaux qui durent : `domain::signals` (une carte par sujet, quatre au plus, expirées après 24 h,
  2 h pour les notes de réunion, en mémoire seulement), cartes `#signals` de la barre latérale
  (mises à jour en place, sortie `dlLeave`), `signal_act` / `signal_dismiss`. Zone qui change
  (`service_watches`), texte reçu (`beam_text_received`), notes proposées (`open_meet`) ; la carte
  part quand on agit ailleurs (copie, notes prises, Library des zones vue). Le toast reste.
- Accueil : `now` de `ui_home` (`home_now_json`) : prochaine réunion (`calendar.card()`), zones
  changées (`Watches::changed`, quatre), onglet reçu d'un autre appareil (12 h).
- Authentification HTTP : l'origine exacte du défi (`challenge.url`, `origin_of` : https ou
  localhost) est gardée par invite (`auth_origins`) ; la palette ne reçoit que les noms
  (`auth.saved`), `auth_saved` fait passer le mot de passe du portefeuille au moteur.
- Test : `scripts\test-features.ps1` (`br*` trou noir, `us*` recherche, `sg*` signaux, `hn1`
  accueil ; étape harnais `beamtext DE|TEXTE` qui simule un texte reçu). Le bouton de veille des
  cartes en direct et les identifiants HTTP enregistrés ne sont pas couverts par un scénario.

## Notices (toasts et capsule de la barre du haut)

- Un seul point d'entrée : `Running::notify(kind, message, actions)` (`app/src/notices.rs` ; `notify_action`
  pour un bouton, `toast` = `Notice::Info`). Ne plus écrire `bh.toast(…)` depuis Rust.
- Les types sont déclarés à un seul endroit, `domain/notice.rs` (`Kind` : info, problem, download, done,
  watch, easel, device, password, undo, meeting ; icône de `ui/icons.js`, ton, durée 2,2 s ou 8 s avec
  boutons). Ajouter un type = une variante, son icône, `ALL` ; les tests vérifient l'icône.
- Un seul composant de dessin, `ui/notice.js` (`bhNotice(host, send)`, injecté par `ui_view`) : icône,
  texte (`textContent`), boutons, ligne du temps restant (`toast-time`). Deux hôtes : `#toast` de la barre
  latérale (`index.html`) et `#notice` de la barre du haut (`bar.html`), où le champ d'adresse devient la
  notice. Mêmes courbes et durées dans les deux.
- Où elle s'affiche : chaque hôte reçoit la notice et garde son échéance ; la barre du haut ne la dessine
  que si aucune barre latérale n'est à l'écran (`notice::in_top_bar` : ni ouverte, ni flottante), état
  poussé par `sync_bar_notice` (`bh.side(side, waiting)`, à chaque `layout` et `ui_state`). Basculer la
  barre pendant une notice la fait changer d'hôte, la ligne du temps reprend où elle en est.
- Un bouton pressé envoie `notice_done` avant sa commande : tous les hôtes retirent la notice.
- Point sur le bouton de la barre latérale (`body.waiting`) : téléchargements en cours et cartes
  `#signals` qui attendent, barre masquée seulement. Un téléchargement fait toujours sortir la barre
  (`reveal_sidebar`) : sa notice est alors dans la barre flottante, pas dans la capsule.
- Test macOS : `bash scripts/test-notices.sh` (contrôles nommés, captures `notice-*`, état intermédiaire
  et animations coupées). Windows : code commun, non testé ; `test-ui.ps1` lit toujours `#toast`.
  Android (`mobile.html`) garde son propre toast.

## Pages d'erreur et User-Agent

- Pages d'erreur (0.7.19) : un gabarit `ui/error.html` (style sci-fi, une variante par `kind` :
  unreachable, offline, not_found, redirects, certificate, clock, http_only, unauthorized, forbidden,
  server, blocked), rempli par `views::error_page(&json)` (données du site par `textContent`, `<`
  échappé). Classement pur dans `domain/error_page.rs` (`classify` des codes WebView2 / `NSError`,
  `classify_status`, `is_bare_error_body`, `hsts_preloaded`, `Bypasses`). App `errors.rs` :
  `Msg::LoadFailure` (wry `with_load_failure_handler`, avant `Failed`/`Finished`) → `on_failed` ;
  401/403/5xx seulement si le corps est vide ou une ligne de statut sans titre (sonde JS après
  `Loaded`) ; `CONNECT_MAX` (20 s) → « injoignable » ; https auto (`Msg::Upgraded`) en échec →
  `http_only` ; traqueur ouvert comme page (`wry::navigating_main_frame`) → `blocked`.
- Contournement (certificat, http, Shields) : le bouton de la page n'envoie que `PageEvent::ErrorMenu`
  (ouvre le menu du shell, seulement si l'onglet affiche notre page) ; seule l'entrée du menu
  (`UiCommand::ErrorProceed`) accorde l'exception, pour l'hôte et jusqu'à la fermeture (mémoire,
  jamais sur disque), refusée pour un hôte HSTS préchargé (liste partielle, aucun moteur n'expose la
  sienne). Certificat : wry `with_server_trust_handler` (WebView2 `ServerCertificateErrorDetected`,
  `AlwaysAllow` seulement pour un hôte accepté, sinon `Cancel` ; WKWebView `ServerTrust` →
  `credentialForTrust` pour un hôte accepté, sinon traitement par défaut). « Non sécurisé » dans la
  barre et le rapport de confidentialité (`errors::insecure`). Test : fin de `test-ui.ps1` (`eval`
  « errN: » hors indices, serveurs locaux 18418 et 18419 auto-signé, étape harnais `errpage KIND`).
  macOS : compilation croisée seulement ; détails du certificat (émetteur, dates) absents sur macOS.
- Boutons des pages d'erreur (0.8.7) : la page est une entrée d'historique de plus sous WebView2
  (`load_html`), posée après celle de l'adresse en échec ; `history.back()` y retourne et l'erreur
  revient, et recharger depuis la page empile deux entrées par essai. « Retour » et « Réessayer »
  passent donc par le shell (`PageEvent::ErrorBack { length }` / `ErrorRetry`, pris en compte
  seulement si l'onglet affiche notre page ; `UiCommand::Back` et `reload` y mènent aussi) :
  règle pure `error_page::Trail` (`steps`, `retry`, `back`, `after_retry`), `failed_entry` lu sur
  le moteur (`view.url()` = adresse en échec). Sans page avant l'échec : dernière page valable de
  l'onglet (`good`), sinon l'onglet se ferme (accès rapide : mis en veille). « Réessayer » revient
  à l'entrée en échec puis la recharge vraiment (le retour seul ressert la réponse d'erreur du
  cache). WKWebView (aucune entrée pour une chaîne HTML) : mêmes règles avec `own_entry = false`,
  non testé. Android : la page agit seule (`history`), « Allow this site » masqué (`allow: false`).
  Certificat refusé dans un onglet qui affichait une page : le `Failed` de la navigation annulée
  est ignoré (`settled`), sinon les deux pages d'erreur se remplaçaient en boucle.
- Un statut HTTP (401, 403, 404, 5xx) n'est jamais « injoignable » : WebView2 marque ces réponses
  `IsSuccess=false`, wry ne renvoie `Failed` que sans réponse HTTP (`HttpStatusCode == 0`).
- Authentification HTTP (Basic/Digest, ex. NAS Transmission) : `with_http_auth_handler` de wry
  (WebView2 `BasicAuthenticationRequested` avec report ; WKWebView `didReceiveAuthenticationChallenge`,
  identifiants `ForSession`, autres défis en traitement par défaut) → `Msg::Auth` → invite de la palette
  (mode `auth`, `UiCommand::AuthAnswer/AuthCancel`). Règles dans `domain/http_auth.rs` (realm, file,
  onglet affiché seulement, réponse commune au même realm, nouvel essai). Identifiants jamais stockés
  ni journalisés ; fermer la palette annule (page 401 du serveur). Peek et vues cachées annulent.
  Test : fin de `test-ui.ps1` (serveur local 401, `scripts/fixtures/http-auth.json`, `eval` « authN: »
  hors indices). Comportement macOS non testé.
- User-Agent macOS : `views::user_agent_for` choisit l'UA à chaque navigation principale
  via `wry::USER_AGENT_RESOLVER` (UA Safari forcé pour Meet, UA WebKit par défaut pour
  `docs.google.com` : le token Safari rend Docs flou). Windows non concerné.

## Mots de passe (0.7.19)

- Règles pures dans `domain/passwords.rs` : `Entry` (id stable, `modified` en ms, `device`,
  pierre tombale `deleted` aux secrets effacés, purge après 90 j), `origin_of` (origine exacte,
  https ou localhost seulement), `offer` (Save / Update / Nothing, liste « Never »), `merge`
  (dernière modification par id, appareil en départage), `generate`, `http_auth_candidates`
  (proposé par l'invite d'authentification HTTP, voir « Ponts entre fonctions »). `Drop` remet à zéro le
  mot de passe et les notes ; `Debug` ne les affiche jamais.
- Fichier `passwords.bin` (`infrastructure/wallet.rs`), séparé de `vault.bin` : contenu
  XChaCha20-Poly1305 sous une clé de données aléatoire, en-tête (format, appareil, KDF, clé
  enveloppée) authentifié. Clé enveloppée par un secret du trousseau (`keyring` « passwords-key » :
  Credential Manager / Keychain) ou par Argon2id (64 Mio, 3 passes) d'un mot de passe maître.
  `open_at_start` migre `vault.bin` au premier lancement puis le supprime. Fichier JSON écrit par
  `write_later`.
- App `passwords.rs` : `autofill.js` (cadre principal) ne reçoit que les identifiants ; le shell
  relit l'origine sur le moteur (`page_origin`) et ne remplit que sur un message portant le jeton
  à usage unique du dernier menu (une page qui poste `autofill_fill` elle-même n'obtient rien).
  `autofill.js` capture `Reflect.apply`, `JSON.stringify`, `addEventListener` et le pont natif
  (`chrome.webview` / `webkit.messageHandlers.ipc`) avant les scripts de la page. Soumission →
  `login_submitted` (`Secret` remis à zéro, jamais journalisé) → toast Save/Update + Never
  (`bh.toast` accepte une liste de boutons). Réglages > Passwords : liste sans secrets, un mot de
  passe envoyé à la page des Réglages seulement sur Show/Edit, copie par `arboard` (exclue de
  l'historique, effacée après 30 s), générateur, mot de passe maître, verrouillage
  (`Settings::password_lock_minutes`). Fenêtre privée : lecture seule. Import CSV → portefeuille.
- Test : fin de `test-ui.ps1` (`eval` « pwN: » hors indices, mots de passe de test générés).
  Trousseau macOS (Keychain) et `arboard` macOS vérifiés par compilation croisée seulement.

## Android (version allégée)

- Pas de wry sur Android (une WebView par Activity, tao, gestionnaires BlackHole absents) : la
  coque Kotlin (`android/app/src/main/java/app/blackhole/browser/`) pilote les WebViews ; le cœur
  `crates/android` réutilise `BrowserService`, le domaine et l'infrastructure sans les modifier.
  Ne jamais dégrader Windows/macOS pour Android ; `crates/app` et `vendor/wry` n'en dépendent pas.
- Protocole JSON (`crates/android/src/bridge.rs`) : `MobileCommand` (interface, origine des assets
  seulement, cadre principal), `PageEvent` (pages non fiables, `bhPage`, seulement `ErrorMenu`),
  `NativeEvent` (coque), `Effect` (réponses). Ajouter une commande = variante + test dans le cœur.
  Logique dans `core.rs` (testée sur l'hôte : `cargo test -p blackhole-android`), jamais dans Kotlin.
- `unsafe_code` est `deny` (pas `forbid`) dans ce seul crate : `#[no_mangle]` n'est permis que dans
  `src/jni.rs` (`cfg(target_os = "android")`).
- Pages : `crates/android/ui/mobile.html` (interface unique : barre du bas, tiroir, grille, palette,
  réglages, historique) et `page.js` ; `error.html` et `shields.js` du bureau sont partagés par
  `include_str!`. Une modification de ces deux fichiers vaut pour les deux apps ; les jetons et
  animations recopiés depuis `home.html`/`palette.html` sont à garder en phase.
- Mêmes règles que le bureau : navigations filtrées par le cœur (`Navigating` → `Cancel`/`Load`),
  exceptions de certificat/http accordées seulement par le menu de l'interface, données externes
  par `textContent`, https seulement pour les téléchargements, aucune sauvegarde des données,
  aucun secret (portefeuille non porté), aucune clé de signature dans le dépôt (release : secrets
  GitHub `ANDROID_KEYSTORE_BASE64`…, sinon APK debug publié avec un avertissement).
- Mises à jour de l'APK (`update.rs`) : le cœur décide (tag de `releases/latest`, APK signé comme
  l'app, SHA-256), la coque fait le réseau (`ureq` sans certificats sur Android) et l'installation
  (`PackageInstaller`). Favicons (`icons.rs`, `icons.json`, jamais en privé), autorisations de site
  (`permissions.rs`, accordées seulement par la réponse de l'interface), site bureau, recherche dans
  la page, impression, raccourci d'écran d'accueil, partage reçu, plein écran vidéo, fichiers.
- Rendu de `mobile.html` : sur place, comme les cartes du bureau. Chaque partie garde la signature
  de ce qu'elle a dessiné (`changed`) et les listes passent par `sync` (nœuds gardés par clé,
  animation d'entrée `.in` une seule fois) ; jamais de `replaceChildren` sur une liste tirée de
  l'état (clignotement, toucher ou bouton armé perdus). Le compteur du bouclier voyage seul
  (`NativeEvent::Blocked` → message `blocked`), pas avec tout l'état. `insets` sépare `bottom`
  (barres système) et `keyboard` : le clavier de nos champs recouvre la barre (la page garde sa
  taille), celui d'une page ou de la recherche dans la page la soulève (`--bb`).
- Textes de l'interface mobile : section `[mobile]` des catalogues (`en.toml` et `fr.toml`),
  jamais en dur. `data-i18n*` et `T('clé')` / `Tn('clé', n)` dans `mobile.html`, `self.t` / `tf` /
  `tn` dans le cœur, `toastKey("clé")` dans la coque ; le test `every_text_asked_for_is_in_the_catalog`
  lit ces écritures littérales. Langue : `Settings::language`, sinon `NativeEvent::Locale`.
- Un seul renderer pour toutes les WebViews : `onRenderProcessGone` doit rester géré sur les
  onglets (`NativeEvent::Crashed`) et sur la vue d'interface (`rebuildShell`), sinon l'app tombe.
- Popups (`popups.rs`) : ids à partir de `POPUP_BASE`, jamais dans le workspace ; `is_private`
  vaut pour tout onglet de session (privé ou popup), `in_private` pour la navigation privée.
  Authentification HTTP (`auth.rs`) : les identifiants ne passent que par `Effect::HttpAuth`.
  Menus, dossiers et rangement : `organize.rs` (rien pour un onglet privé ou une popup).
- Commandes : `cd android; .\gradlew.bat assembleDebug` (lance `cargo ndk`), `scripts\test-android.ps1
  [-Boot]` (adb, captures `%TEMP%h_android_shots`, contrôles nommés `bheval:` lus dans logcat).
  Toute nouvelle fonction d'interface mobile : un scénario et un `Check` dans ce script.

## Synchronisation des onglets (0.8)

- Entre appareils appairés (« Continuer sur l'autre appareil ») : accès rapide, onglets ouverts (hors
  dossiers) et espaces (nom, couleur). Règles pures `domain/sync.rs` (`Ledger::observe`, `merge` :
  dernière modification gagne, pierres tombales 30 j, `Plan` d'actions) et `Workspace::sync_view` /
  `apply_sync` (onglets ouverts en arrière-plan, l'onglet affiché ne change que s'il est fermé).
  `infrastructure/sync.rs` : `SyncEngine` (`sync.json`, format de transport validé, `decide`).
  Transport : `handoff` `Request::Snapshot` → `Message::Part` (fragments acquittés) → `Event::Snapshot`.
- Mode par appareil dans `devices.bin` (`ask` par défaut, `auto`, `off`, `Request::SetSync`) ; `ask`
  propose (toast Merge / Always / Later), une réponse à une fusion acceptée s'applique sans question.
  Desktop : `app/src/handoff.rs` (`sync_tick`, `on_ledger`), Réglages > Devices ; Android :
  `crates/android/src/devices.rs`. Jamais : fenêtres/onglets privés, historique, dossiers.
- Test : `scripts\test-sync.ps1` (Windows devtools ↔ émulateur, ports `BH_HANDOFF_PORT`/`_PEER_PORT`).

## Langues de l'interface (i18n)

- Catalogues : `crates/domain/i18n/en.toml` (référence et repli) et `fr.toml`, TOML plat
  (`[section]`, `clé = "texte"`), embarqués par `include_str!`. Logique pure dans
  `domain/i18n.rs` : `resolve` (réglage, puis langues du système, puis anglais), `Catalog::get`
  (repli anglais puis la clé), `format` (`{host}`, `{n}`), `plural` (`clé.one` / `clé.other` ;
  français : 0 et 1 au singulier), `message` (texte d'une couche sans langue, rendu plus tard).
- Réglage `Settings::language` (`"system"`, `"en"`, `"fr"`), Réglages > General, commande
  `set_language`. App : `app/src/i18n.rs` (`t`, `tf`, `tn`, `render`), `language_changed` envoie
  `bhI18n.set(catalogue)` à toutes les vues ouvertes (menus natifs et titres de fenêtres compris).
- Pages : `ui/i18n.js` injecté comme `keys.js` (`bhT`, `bhTn`, `data-i18n`, `data-i18n-title`,
  `-placeholder`, `-aria`, par `textContent`/attributs) ; une page qui dessine depuis un état écoute
  `bhlang` et rejoue son dernier rendu. Page d'erreur : chaînes `error.*` dans sa config (prochaine
  erreur). Scripts injectés dans les pages web (`find.js`, `autofill.js`, `live_picker.js`) :
  `views::page_script`, textes fixés à la création de la vue. Raccourcis toujours par `bhKeys`.
- Ajouter une chaîne : la même clé dans `en.toml` et `fr.toml` (mêmes `{paramètres}`), puis
  `t("section.clé")` en Rust ou `bhT('section.clé')` / `data-i18n` en page. Les tests vérifient la
  parité des catalogues et que toute clé écrite en dur existe. Texte français plus long : vérifier
  qu'il n'est pas tronqué (barre latérale à 200 px). Couches internes : `i18n::message(clé, …)`.
- Ajouter une langue : `crates/domain/i18n/<code>.toml` (toutes les clés), l'ajouter à `SOURCES`
  et `LANGUAGES` (nom dans sa propre langue), règle de pluriel dans `plural_form` et `ui/i18n.js`,
  taille du tableau `JSON` de `app/src/i18n.rs`.
- Changelog (`domain/changelog.rs`) : reste en anglais (notes techniques datées, écrites une fois
  à la livraison ; seul le cadre « Nouveautés » est traduit). Les noms (Easel, Boost, Air Traffic,
  Meet, Shields) restent. Android : mêmes catalogues (section `[mobile]`), voir la section Android.
- Scénarios : chaque script devtools fixe sa langue (`cmd {"cmd":"set_language",…}`) ;
  `scripts/test-errors.sh` en français, les autres en anglais ; `scripts/test-i18n.sh` bascule en
  direct et contrôle les textes.

## Livraison et entretien

- Préserver les changements utilisateur déjà présents ; limiter les modifications
  au périmètre demandé. Ne pas committer ni publier sans demande. Messages de commit
  au format `type(scope): résumé` (scopes : `tabs`, `pip`, `space`, `easel`, `ext`,
  `ui`, `ci`…), relus pour les fautes.
- Mettre à jour la documentation et les consignes si l'architecture ou les
  commandes changent. Éviter les modifications sans rapport dans `vendor/wry`.
- Ne pas modifier les sorties générées dans `target/`, `dist/` ou `tmp/`.
- Pour une livraison demandée, synchroniser la version workspace de `Cargo.toml`
  et `Cargo.lock`. Le workflow `.github/workflows/release.yml` peut publier sur
  push de `main` si la version n'a pas encore de tag, ou sur tag `vX.Y.Z`.
- `scripts/package-macos.sh` produit le bundle macOS ; `--install` l'installe.
- Mise à jour dans l'app (`infrastructure/updater.rs`) : dernière version lue par la redirection
  `github.com/<repo>/releases/latest` + `SHA256SUMS.txt` du tag, jamais l'API REST (quota anonyme
  par IP). Réglages > Général > Updates : vérification manuelle ; Installer fait décoller un vaisseau.
  L'installeur Windows utilise `scripts/windows-installer.iss` avec Inno Setup.
- Terminer par un résumé des changements, des vérifications effectuées et des
  limites, notamment pour l'autre système d'exploitation.
