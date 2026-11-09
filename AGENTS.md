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
  ou de dossier appelle `release_tab` (il reste dans la barre). Libérer une vue d'onglet
  via `drop_view` (sonde média et suivi du PiP compris). Un PiP n'est lancé que pour
  l'onglet quitté qui n'est plus à l'écran (pas l'autre volet d'un split).
- Vues de réserve : créées sans URL (`spare_tab_view`), jamais sur `about:blank`, sinon
  cette page reste la première de l'historique de l'onglet (Précédent y mène).
- Accès rapide (onglets épinglés) : fermer (menu, ⌘W, clic molette, commande `close`) ne
  les retire jamais de la liste, ça les met en veille ; seul « Remove from quick access » les
  retire. Clic molette : veille pour un onglet d'accès rapide ou de dossier, fermeture sinon.
  Pas de bouton son sur les tuiles. La fonction « Lock URL » a été retirée (un ancien champ
  `locked` dans `workspace.json` est ignoré au chargement).
- PiP : quitter la fenêtre flottante ne change jamais d'onglet (la page ne distingue pas
  « fermer » de « retour à l'onglet ») ; la carte lecteur reste dans la barre latérale.
- Fenêtres Réglages et Easel : `panel_window_attributes` (sans bordure sous Windows, barre de
  titre transparente sur macOS) ; leur page dessine la barre du haut (38 px, teinte de l'espace,
  boutons) et la pilote par `UiCommand::PanelWindow`.
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
  `bash scripts/test-popup.sh` (popup avec `opener`, Trusted Types, serveur local),
  `bash scripts/test-settings.sh` (Réglages et icônes ; `BH_SHOTS_ONLY=1 BH_PREFIX=avant-` pour les seules captures),
  `bash scripts/test-i18n.sh` (langue anglais ↔ français en direct, captures `i18n-*`).
- Windows : `scripts\test-ui.ps1`, `scripts\test-ext.ps1`,
  `scripts\test-easel.ps1`.

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
- Capture en direct : la zone choisie attend dans `live_pending` ; un menu demande l'Easel
  (ou un nouveau) → `LiveCaptureTo`. Sans Easel dans l'espace, il est créé directement.
- Barres de défilement des pages d'interface : `ui/theme.js`, injecté avec `keys.js`.
- Meet et agenda : règles pures dans `domain/meetings.rs` ; lecture iCal (fuseaux `chrono-tz`,
  récurrences `rrule`, EXDATE, RECURRENCE-ID, annulés) et téléchargement dans
  `infrastructure/calendar.rs` (https, `calendar.google.com` seulement, sans redirection, taille
  plafonnée) ; adresses secrètes chiffrées (`calendars.bin`, `Vault::named`). App : `meet.rs`
  (synchro en thread toutes les 10 min, carte « prochaine réunion », rappel, `open_meet` qui
  réutilise l'onglet Meet d'accès rapide ou de dossier sauf s'il est en appel). La page des
  Réglages ne reçoit jamais l'adresse secrète, seulement le nom de l'agenda.
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
  Areas ». Démarrée depuis le menu du sélecteur de zone (`WatchArea`).
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
  (point d'accroche pour l'invite d'authentification HTTP, non branché). `Drop` remet à zéro le
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
  Meet, Shields) restent. Android : interface mobile en anglais ; page d'erreur partagée en anglais.
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
