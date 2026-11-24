# Instructions Copilot — BlackHole

Navigateur Rust (winit + wry) : WKWebView sur macOS, WebView2 sur Windows, WebKitGTK sur Linux (aperçu depuis 0.8.16). **Les deux plateformes doivent toujours cohabiter : ne jamais dégrader l'une en travaillant sur l'autre.** Répondre en français, de façon concise.

## Architecture (clean architecture)
`domain` → `application` → `infrastructure` → `app` (composition root : winit + wry). Les dépendances vont uniquement vers la gauche.
- `domain` : types et règles métier, sans I/O ni dépendance système.
- `application` : cas d'usage (`Service`), orchestre le domaine via des traits.
- `infrastructure` : persistance, réseau, market d'extensions signé (`market.rs`) et paquets installés (`catalog.rs`), mise à jour (`updater.rs`), import, coffre chiffré.
- `market-tool` (binaire `bh-market`) : tient `market/market.json` (suivi des releases amont, audit, signature) ; utilisé par `.github/workflows/market.yml`, jamais embarqué.
- `app` : fenêtres, vues web (`views.rs`), IPC (`ipc.rs`), état (`state.rs`, construit par `boot.rs`), boucle (`app.rs`), aiguilleurs (`commands.rs`, `messages.rs`), modules `shell/` (interface), `tabs/` (vues de pages), `features/` (une fonctionnalité par module), UI HTML/JS dans `crates/app/ui/`, harnais `devtools.rs`. `main.rs` ne contient que les modules et `main()`.
  - Chaque module étend `Running` par son propre `impl Running`. Une `UiCommand` est traitée par le gestionnaire de son module (`handle_tabs`, `handle_organize`, `handle_settings`, `handle_onboarding`, `handle_extensions`, `handle_imports`, `handle_easel`…) ; `handle_ui` route et reste exhaustif.
  - L'état d'une fonctionnalité est une sous-structure de son module (`Updates`, `Market`, `Downloads`, `Auth`, `Pip`, `MediaWatch`, `Imports`, `Palette`, `Onboarding`), pas une suite de champs de `Running`.
  - Détail : section « Organisation de `crates/app/src` » d'`AGENTS.md`.
- `macos-url`, `macos-webext` : intégration système. `macos-webext` contient `lib.rs` (macOS), `win.rs` (Windows, WebView2) et un stub pour les autres cibles ; les trois exposent la même API `ExtensionHost`.
- `android` (`crates/android`, cdylib + rlib) : cœur de la version Android, au-dessus de `application`/`infrastructure` ; coque Kotlin dans `android/` (Gradle). Voir la section Android.

Mettre la logique testable dans `domain`/`application`/`infrastructure`, pas dans `app`.

## Code spécifique à une plateforme
- Séparer par `#[cfg(target_os = "macos")]` et `#[cfg(windows)]` ; chaque branche est autonome. Ne jamais mettre de code Windows dans une branche macOS ni l'inverse.
- Toute nouvelle fonctionnalité plateforme : fournir les deux branches (ou un repli explicite documenté) et garder le stub pour les autres cibles.
- Imports spécifiques : les placer dans le bloc `cfg` correspondant (sinon avertissements `unused_imports`/`dead_code` sur l'autre plateforme).
- Exemples : `ExtAction` (macOS : popover natif ; Windows : vue popup + scrim `extpopup.html`), fenêtre sans décorations + boutons dans `ui/bar.html` sous Windows (`body.windows`), `backdrop.rs` (softbuffer) contre les bords blancs, icône via `build.rs` (`winresource`).
- Windows : toutes les vues d'un même dossier de données doivent avoir les mêmes réglages moteur (extensions activées, GPU, arguments) → toujours passer par `views::builder()` (jamais `--process-per-site` : voir 0.8.27).
- Mémoire : chaque vue = un renderer. Pages de shell rares (`library_panel`, `easel_panel`, `splitbar`) = `LazyView` créées à la demande (dans `ensure_panels`) et libérées après 60 s masquées ; les overlays (hotzone, flyout, palette, `Option<WebView>`) sont créés juste après les premières vues au lancement puis recréés au-dessus des nouveaux onglets via `raise_palette` (différé, `flush_raise`). Plafond `max_live_tabs` (domain `overflow_to_release`), préchargement des voisins (`preload_next_tab`), cible mémoire WebView2 « Low » sur les onglets masqués (`set_view_background`, rien à faire sur macOS). Vues de réserve (`spares`) : réglage `spare_views` (0, 1 ou 2 ; défaut 2, ~50 Mo chacune).
- **Parité systématique :** toute modification ou demande réalisable sur les deux OS est appliquée aux deux dans la même passe (logique commune dans domain/pplication/HTML, branches cfg pour le natif). Si un côté est impossible ou non testable, le dire explicitement et laisser un repli, jamais l'ignorer en silence. Ex. : un popout d'extension s'ouvre en popup sur Windows (show_ext_popup) et en popover natif sur macOS (show_native_popup).
- Pas de build macOS complet sous Windows (pas de SDK Apple), mais une vérification de types est possible : `rustup target add aarch64-apple-darwin`, puis `cargo clippy --workspace --all-targets --target aarch64-apple-darwin` avec `CC_aarch64_apple_darwin`/`AR_aarch64_apple_darwin` pointant vers un faux compilateur qui crée des fichiers vides (seuls les scripts de build C en ont besoin). La faire après toute modification sous `cfg(target_os = "macos")` ; le comportement reste non testé sur macOS, le signaler.
- MSRV `rust-version = "1.80"` : pas d'API plus récente (ex. `as_chunks`, 1.88) même si clippy la suggère.

## Sécurité et qualité
- `unsafe_code = "forbid"` dans le workspace, sauf crates `macos-url` et `macos-webext`.
- Navigation : seuls http(s) sont autorisés dans les onglets (plus les schémas d'extension `webkit-extension`/`safari-web-extension`/`chrome-extension`, et depuis 0.8.27 un `file:` local que le moteur affiche lui-même : `domain::downloads::viewable_file`, jamais HTML/SVG/XML). Les vues d'UI ne naviguent jamais hors de `about`/`data`.
- Téléchargements : HTTPS uniquement, hôtes attendus, **SHA-256 épinglé** vérifié (market, mises à jour). Une URL/hash par plateforme : table `platforms` (`macos`, `windows`) de chaque entrée de `market/market.json`, `Entry::package()`.
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
  - `scripts\test-ui.ps1` : navigation, palette, paramètres, animations (sidebar, palette), bouton de mise à jour, maximize/restore, onglets façon Arc, recherche dans la page, menu de la sélection, appareils, authentification HTTP (serveur 401 local), mots de passe, pages d'erreur (serveurs locaux http et https auto-signé), lien de téléchargement et barre latérale masquée, menu de page, lignes de la barre latérale gardées (`dlN:`, `cmN:`, `sbN:`, `anN:` lus par leur nom), recherche à travers les nœuds, Mod+1, classement de la palette, Bibliothèque sans résultat, Échap et éditeur de mot de passe des Réglages (`fn1`, `ky1`, `pa2`, `lbN`, `esN`). 47 captures dans `%TEMP%\bh_shots`.
  - `scripts\test-ext.ps1` : installe Dark Reader depuis le catalogue et vérifie l'injection (utiliser une page claire comme info.cern.ch ; example.com gère déjà le mode sombre).
  - Profil jetable `%TEMP%\bh_data` ; le script lit `%TEMP%\bh_script.txt`.
  - Protocole du script : `wait MS`, `shot NOM`, `shots NOM` (fenêtre des paramètres), `cmd JSON` (un `UiCommand`), `eval chrome|bar|home|palette|peek|settings|content JS`, `quit`. Les contrôles lisent les `eval` par indice : **ajouter un `eval` décale les indices suivants**, mettre à jour les `Check`.
  - Les captures sont prises à l'écran : la fenêtre doit être visible et non recouverte. Vérifier visuellement les images.
  - Si l'appli reste en vie après un timeout, arrêter le process par son id (`Stop-Process -Id`) avant de relancer.
- **Easel** (page `ui/easel.html`, affichée soit dans la fenêtre dédiée « All Easels », soit dans la zone de contenu comme un onglet via `easel_panel`/`easel_tab` — `home_shown()` est vrai pendant un onglet Easel ; tout changement d'onglet doit remettre `easel_tab = false` ; logique dans `domain/easel.rs`, persistance `EaselStore`) : `scripts/test-easel.sh` (macOS) et `scripts\test-easel.ps1` (Windows), cibles devtools `eval easel`, `eval etab` (onglet) et `shote NOM`. Capture live : vue native enfant de la fenêtre qui héberge l'Easel (fenêtre principale en onglet, positions décalées de l'origine du contenu ; `EaselLayout.embedded` distingue la page émettrice ; `views::live_view`, recadrée par `ui/live_crop.js`, zone choisie par `ui/live_picker.js`), positionnée via `EaselLayout` ; elle doit être déclarée/dropée avant la fenêtre ; cible devtools `eval live`. La palette et l'aperçu (peek) ne masquent les vues live que si l'Easel est dans la fenêtre principale (onglet ou côte à côte) ; `apply_slots` les replace à l'ouverture/fermeture de ces surcouches. Ne pas appeler `focus_window()` sur la fenêtre principale depuis un handler : panique winit sur macOS. Fermeture des fenêtres Réglages/Easel : `retire_window` (masquée, détruite au tour suivant ; `window_event` ignore toute fenêtre autre que les trois connues), sinon crash AppKit (`resignKeyWindow` sur un délégué libéré). Leur page est une vue enfant (`views::panel_view`, `set_bounds` sur `Resized`) : `build()` remplace la vue de contenu winit et fait paniquer winit à la perte de focus (`windowDidResignKey`).
- **Sessions par espace** : `views::Store` (`Main` = espace id 1, `Space(id)`, `Private`) choisit le stockage de cookies de chaque vue d'onglet (macOS : `DataStore` de `macos-webext` / `with_data_store_identifier` ; Windows : `with_profile_name`). Toute nouvelle vue de page doit recevoir le `Store` de son espace (`store_of_tab` / `store_of_space`) ; les vues de réserve (`spares`) sont rangées par `Store`. Les cookies d'un test e2e persistent dans le stockage WebKit : utiliser des noms uniques.
- **Écritures disque** : jamais de `std::fs::write` sur le thread UI pour des données utilisateur ; passer par `infrastructure::write_later` (thread d'arrière-plan, `flush_writes` avant relecture et à la fermeture). Ne pas multiplier les `ui_state()` : les événements de page marquent `ui_stale`. Le bloqueur (`PrivacyPolicy::set_blocking`, `Settings::tracker_blocking`) est partagé par tous les clones de la politique.
- **Préférences d'interface** (mode de la barre latérale…) : à enregistrer dans `Settings` (champ `#[serde(default)]`) dès qu'elles changent, pour survivre au redémarrage.
- **macOS** : `scripts/test-webext.sh` (coupe d'abord les bloqueurs intégrés — `set_tracker_blocking` et le bouclier `ads` — qui bloquent les mêmes hôtes ; installe uBOL, vérifie le blocage actif/inactif, retrait), `scripts/test-errors.sh` (serveurs locaux 47996-47998 : connexion refusée, nouvel essai, DNS `.invalid`, port restreint en nouvel onglet et dans un onglet ouvert, connexion coupée, 500/403 vides, certificat auto-signé, adresse invalide, puis page normale et appli toujours vivante), `scripts/test-popup.sh` (serveur local 47990 : popup de connexion avec `opener` et `window.close()`, page sous Trusted Types), `scripts/test-blocking.sh` (serveur local 47985 : requêtes tierces bloquées et comptées, autre tiers chargé, fonctions de la page natives, sonde média sur signal ; l'étape `state` donne `blocked`, `mediaHints`, `mediaPlaying`), `scripts/test-easel.sh` (après `live_pick`, la zone passe par le menu : `live_capture_to`), `scripts/readme-shots.sh` (captures du README dans `docs/screenshots/`, 1600 px ; le titre animé est `docs/title.svg`) et le harnais `devtools` avec captures CoreGraphics (`/tmp/bh_script.txt` → `/tmp/bh_shots/`).
- Toute nouvelle fonctionnalité d'UI : ajouter un scénario (et un `Check`) dans le script de la plateforme concernée, avec capture ; toute animation : tester l'état intermédiaire et le mode animations désactivées.

## Extensions
- macOS : `WKWebExtensionController`. Windows : `ICoreWebView2Profile7::AddBrowserExtension` (zips Chromium dépaquetés dans `<données>/webext/unpacked/<id>`, `catalog::remove` supprime aussi ce dossier).
- Bitwarden : build Firefox sur macOS, build Chromium sur Windows (`platforms.macos` / `platforms.windows` de son entrée).
- Ajouter une extension au market : une entrée dans `market/market.json` (id, nom, descriptions `en`/`fr`, `platforms` avec URL de release GitHub et SHA-256, `source` = dépôt, préfixe de tag, motif d'asset par plateforme), puis `cargo run -p blackhole-market-tool -- audit --write` (télécharge, vérifie le hash, remplit taille et permissions) ; le test `builtin_list_is_valid_and_covers_both_systems` valide la liste.
- Limites WebView2 : pas de badge, pas d'onglet actif exposé aux extensions ; le popup est une vue `chrome-extension://<id>/<popup>` sur le même profil.

## Ponts entre fonctions (0.8.8)

Module `app/src/features/bridges.rs` ; règles pures dans `domain/verbs.rs` et `domain/signals.rs`.

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
  localhost) est gardée par invite (`auth.origins`) ; la palette ne reçoit que les noms
  (`auth.saved`), `auth_saved` fait passer le mot de passe du portefeuille au moteur.
- Test : `scripts\test-features.ps1` (`br*` trou noir, `us*` recherche, `sg*` signaux, `hn1`
  accueil ; étape harnais `beamtext DE|TEXTE` qui simule un texte reçu). Le bouton de veille des
  cartes en direct et les identifiants HTTP enregistrés ne sont pas couverts par un scénario.

## Notices (toasts et capsule de la barre du haut)

- Un seul point d'entrée : `Running::notify(kind, message, actions)` (`app/src/shell/notices.rs` ; `notify_action`
  pour un bouton, `toast` = `Notice::Info`). Ne plus écrire `bh.toast(…)` depuis Rust.
- Les types sont déclarés à un seul endroit, `domain/notice.rs` (`Kind` : info, problem, download, done,
  watch, easel, device, password, undo, meeting ; icône de `ui/icons.js`, ton, durée `stay_ms` : 2,2 s,
  8 s avec boutons, 6 s pour un problème, au moins 60 ms par caractère, 15 s au plus ; le temps
  s'arrête sous le pointeur ou le focus, `held`). Ajouter un type = une variante, son icône, `ALL` ; les tests vérifient l'icône.
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
- Test macOS : `bash scripts/test-download.sh` (téléchargement lancé par une page déjà affichée : redirection par script ou 302 ; WebKit termine la navigation en `WebKitErrorDomain` 102, gardé dans `Errors::failures` pour que `on_failed` n'affiche pas « Site injoignable » ; Windows non testé).
- Test macOS : `bash scripts/test-notices.sh` (contrôles nommés, captures `notice-*`, état intermédiaire
  et animations coupées). Windows : code commun, non testé ; `test-ui.ps1` lit toujours `#toast`.
  Android (`mobile.html`) garde son propre toast.

## Pages d'erreur et User-Agent
- Pages d'erreur (0.7.19) : gabarit unique `ui/error.html` (mêmes couleurs, typographie et étoiles que l'ancienne page « Site injoignable » ; une scène SVG animée propre à chaque variante, `data-art` = `kind` : vaisseau près du trou noir pour `unreachable`, satellite coupé de sa planète, écran radar de bord, vaisseau en boucle entre deux trous de ver (SMIL `animateMotion`), blocus de vaisseaux autour d'une station au cadenas brisé pour `certificate`, horloge de bord folle, sas ouvert qui perd ses données pour `http_only`, sas verrouillé à scanner, dôme d'énergie pour `forbidden`, station en surchauffe, drone traqueur pris dans le filet des Shields ; teinte `body.danger|calm|amber|green` ; `prefers-reduced-motion` et réglage `animations` → `body.still` et `pauseAnimations()` pour le SMIL), variantes `kind` : `unreachable`, `offline`, `not_found` (DNS), `redirects`, `certificate` (raison `expired`/`not_yet_valid`/`untrusted`/`self_signed`/`name_mismatch`/`revoked`/`invalid`, section « Avancé » : hôte, raison, émetteur, sujet, dates), `clock`, `http_only`, `unauthorized` (« Se connecter » = recharger, la demande d'identifiants revient si le serveur en envoie une), `forbidden`, `server`, `blocked`. `views::error_page(&json)` (JSON dans le script, `<` échappé, tout en `textContent` ; `window.__bhRetry`).
  - Domaine `error_page.rs` : `classify(Failure::WebView2(status) | Failure::NsError(domain, code))` (annulé, attente d'identifiants, `WebKitErrorDomain` 101–103/204 → `None`), `CertProblem::refine(issuer, subject)` (auto-signé), `ErrorPage::with_clock` (horloge hors 2026–2100 → `clock`), `after_https_upgrade`, `classify_status` (401, 403, 5xx), `is_bare_error_body` (corps vide ou simple ligne de statut sans `<title>` ; une page du serveur avec titre reste affichée, comme les pages de test 401/403 de l'authentification HTTP), `hsts_preloaded` (extrait : TLD Google `.dev`, `.app`… et quelques sites), `Bypasses`.
  - vendor/wry : `LoadFailure { url, domain, code, http_status }` via `with_load_failure_handler` (Windows : `NavigationCompleted` non réussie, `domain` `webview2` ou `http` ; macOS : `NSError` de `didFail(Provisional)Navigation`, statut ≥ 400 de `decidePolicyForNavigationResponse` du cadre principal), appelé avant `PageLoadEvent` ; `ServerTrustChallenge` via `with_server_trust_handler(|c| -> bool)` (Windows : `ICoreWebView2_14::ServerCertificateErrorDetected`, émetteur/sujet/dates de `ICoreWebView2Certificate`, `ALWAYS_ALLOW` si vrai, sinon `CANCEL` ; macOS : défi `NSURLAuthenticationMethodServerTrust` traité avant Basic/Digest, `credentialForTrust` par `msg_send` si vrai, sinon `PerformDefaultHandling`, `error` = 0 et pas de détails) ; `navigating_main_frame()` (macOS : cadre visé par `decidePolicyForNavigationAction`, Windows : toujours vrai).
  - App `errors.rs` : `Msg::LoadFailure` (gardé jusqu'à `Failed`, ou affiché tout de suite pour un certificat refusé signalé « annulé »), `Msg::ServerTrust`, `Msg::Upgraded` (https automatique, aussi pour une adresse `http://` tapée) → `http_only`, `Msg::BlockedPage` (traqueur ouvert dans le cadre principal), sonde `PROBE_JS` après `Loaded` pour 401/403/5xx, `CONNECT_MAX` (20 s) → `unreachable`. Sécurité : la page d'erreur est chargée dans l'onglet ; son bouton risqué n'envoie que `PageEvent::ErrorMenu {x, y}`, pris en compte seulement si l'onglet affiche notre page (`shown`, effacé à `LoadStart`) ; le menu du shell porte `UiCommand::ErrorProceed { tab }`, seul à accorder l'exception (`EXCEPTIONS` en mémoire : certificats et http par hôte via `Bypasses`, refus HSTS ; hôtes débloqués des Shields) ; `errors::with_exceptions` dans le gestionnaire de navigation des onglets. « Non sécurisé » : `ui_bar` (`insecure`, pastille dans `bar.html`) et rapport de confidentialité.
  - Tests : domaine ; `test-ui.ps1` (serveur http 18418 : 401/403/502 vides, boucle de redirection ; https 18419 avec certificat auto-signé créé en mémoire par `CertificateRequest` ; `.invalid` pour le DNS ; traqueur `stats.g.doubleclick.net` ; étape harnais `errpage offline|clock|http_only|redirects|expired` ; `eval` « errN: » via `ErrEval`, hors indices). macOS : `scripts/test-errors.sh`. Ports restreints (liste « bad ports » de Fetch, `error_page::is_restricted_port`) : WebKit les refuse sans appeler le délégué de navigation, l'onglet resterait vide ; `refuse_restricted_port` affiche « Site injoignable » avant tout chargement (création de vue, `navigate`, `Redirect`). Un lien de page vers un tel port n'est pas couvert.
  - Boutons (0.8.7) : sous WebView2 la page d'erreur (`load_html`) est une entrée d'historique de plus, après celle de l'adresse en échec quand celle-ci a reçu une réponse ou la page d'erreur du moteur (pas quand la navigation a été annulée : certificat refusé, Shields). `history.back()` retombait donc sur l'adresse en échec (l'erreur revenait : « Retour en lieu sûr » et le Précédent du shell semblaient morts) et `location.href = adresse` empilait deux entrées par essai. Désormais le gabarit envoie `PageEvent::ErrorBack { length: history.length }` et `ErrorRetry` (config `shell: true`, `steps`), pris en compte seulement si l'onglet affiche notre page (`shown`) ; `UiCommand::Back` (→ `views::error_back` → `__bhBack`) et `views::reload` (`__bhRetry`) suivent le même chemin. Règle pure `domain::error_page::Trail { own_entry, failed_entry, stale }` : `steps()` (entrées jusqu'à la page d'avant), `back(length)` → `Go(n)` (`history.go(-n)`) ou `Leave` (dernière page valable de l'onglet, `Errors::good`, sinon `UiCommand::Close` : un onglet d'accès rapide est mis en veille), `retry()` → `ReloadEntry` (retour à l'entrée en échec puis `reload()` une fois la copie du cache affichée, `Errors::reloading` : un retour seul ne recontacte pas le serveur), `Replace` (`location.replace`, `after_retry` compte la page d'erreur restée dessous si le moteur annule encore) ou `Push`. `failed_entry` = `view.url()` égale à l'adresse en échec au moment d'afficher la page ; `own_entry` = `cfg!(windows)` (WKWebView ne crée pas d'entrée pour une chaîne HTML : hypothèse non testée ; filet `Errors::escaping` : si le retour retombe sur l'adresse en échec dans les 30 s, l'onglet recule encore d'un cran au lieu de réafficher l'erreur). Une page d'erreur revue par l'historique (`navigation.type == back_forward`) n'est plus suivie par le shell : elle agit seule (`history.go(-steps)`, `location.href`), comme sur Android où la config n'a pas `shell`. Android : `Navigating` venant de notre page d'erreur retire l'erreur de l'onglet (sinon une page qui charge après « Réessayer » restait prise pour l'erreur), et la page « bloqué » reçoit `allow: false` (le cœur mobile n'a pas d'exception pour les Shields : bouton masqué). Certificat refusé dans un onglet qui affichait déjà une page : WebView2 termine la navigation annulée par un `Failed` portant l'adresse de la page précédente ; `Errors::settled` l'ignore et le défi est retiré de `certs` une fois la page montrée (sinon « Site injoignable » et la page de certificat se remplaçaient sans fin). Test : `test-ui.ps1`, `err22`–`err49` (chaque bouton des variantes 401, 403, 5xx, DNS, injoignable, bloqué ; compteur de requêtes `/hits` du serveur local ; onglet sans historique fermé).
- Statut HTTP ≠ panne réseau : WebView2 renvoie `IsSuccess=false` pour toute réponse 4xx/5xx ; `NavigationCompleted` (vendor/wry) lit `ICoreWebView2NavigationCompletedEventArgs2::HttpStatusCode` et ne signale `Failed` que sans réponse HTTP (ni annulation, ni `VALID_AUTHENTICATION_CREDENTIALS_REQUIRED`). La page du serveur s'affiche.
- Authentification HTTP (Basic/Digest) : `WebViewBuilder::with_http_auth_handler(|HttpAuthChallenge, HttpAuthResponder|)` dans vendor/wry. Windows : `ICoreWebView2_10::add_BasicAuthenticationRequested` + `GetDeferral` (sans gestionnaire, WebView2 ouvrirait sa propre boîte). macOS : `webView:didReceiveAuthenticationChallenge:completionHandler:` (Basic/Digest seulement, le reste en `PerformDefaultHandling` pour ne pas casser TLS ; `NSURLCredentialPersistenceForSession`, jamais le trousseau ; annuler = `UseCredential` sans identifiant → page 401). Un `HttpAuthResponder` abandonné annule. App : `Msg::Auth` → `on_auth` (retire l'onglet de `awaiting` : pas de « injoignable » pendant la saisie) → `domain::http_auth::AuthPrompts` (invite seulement pour l'onglet affiché ou l'autre volet du split, une réponse sert tous les défis du même onglet/hôte/realm, `retry` si les identifiants viennent d'être refusés, oublié à `Msg::Loaded`) → palette mode `auth` (`#auth`, textes du serveur par `textContent`, avertissement en http, mot de passe effacé dès l'envoi). Fermer la palette ou l'ouvrir pour autre chose annule. Peek et `live_view` (Easel, veille) annulent toujours. Identifiants jamais stockés ni journalisés. Test : scénario « http auth » de `scripts	est-ui.ps1` (serveur `TcpListener` sur 127.0.0.1:18417, identifiants de test `scripts/fixtures/http-auth.json`, `eval` préfixés `authN:` lus par `AuthEval` et exclus de `$evals`). macOS vérifié par compilation croisée seulement.
- User-Agent macOS (0.8.29) : `domain::safari` (version de Safari = celle du moteur : macOS ≥ 26, sinon build de
  `WebKit.framework`, plancher 18.5), posée par `views::announce_browser` dans `wry::APPLICATION_NAME`
  (`applicationNameForUserAgent` de chaque configuration : fenêtres ouvertes par les pages comprises). Jamais de version
  figée ni d'UA sans nom : Google les dit « non pris en charge ». Plus d'exception pour `docs.google.com`. Windows non concerné.

## Mots de passe (0.7.19)
- Domaine `passwords.rs` : `Passwords { entries, never }`, `Entry` versionnée pour la synchro future (id aléatoire 128 bits, `created`/`modified` en ms, `device`, pierre tombale `deleted` aux secrets effacés, purge `TOMBSTONE_TTL_MS` = 90 j), `merge` (dernière modification par id, id d'appareil en départage, idempotent), `origin_of` (origine exacte schéma+hôte+port, https ou localhost seulement : ni sous-domaine, ni http, ni autre port), `offer`, `import`, `generate` (sans biais, sans caractères ambigus), `http_auth_candidates(origin, realm)` (proposé par l'invite d'authentification HTTP : noms seulement à la palette, `auth_saved`). `Drop`/`Debug` protègent les secrets (`zeroize`).
- Infrastructure `wallet.rs` : `passwords.bin` (JSON : en-tête + `nonce` + `data` en hexadécimal), XChaCha20-Poly1305 avec l'en-tête en AAD ; clé de données enveloppée par le trousseau (`keyring` « BlackHole/passwords-key », Windows Credential Manager, macOS Keychain ; aucun trousseau ailleurs) ou Argon2id (m=64 Mio, t=3, p=1, sel 16 octets) d'un mot de passe maître. `open_at_start` (thread) : crée le fichier, y migre `vault.bin`, supprime ce dernier une fois relu. `Vault`/`vault.bin` restent pour `calendars.bin` et `devices.bin`. MSRV : `base64ct` épinglé en 1.7.x (1.8 exige Rust 1.85).
- App `passwords.rs` : `Passwords` (état, fil de fond, offres, presse-papiers), `PasswordAction` (commande `{"cmd":"passwords","action":{"op":…}}`, pages d'interface seulement). Remplissage : `autofill_query` → menu `{names, locked, generate, token}` ; `autofill_fill`/`autofill_unlock`/`autofill_generate` exigent le jeton à usage unique et l'origine relue sur le moteur. Un script de page peut poster sur `__bhipc` : ne jamais servir un secret sans ce jeton. Enregistrement : `login_submitted` → toast à deux boutons (`Offer { tab, choice }`), 2 min d'attente. Copie : `arboard` (`SetExtWindows::exclude_from_history/cloud`, `SetExtApple::exclude_from_history`), effacée après 30 s si inchangée. Mot de passe maître : Argon2 dans un thread (`Done::Protected`), verrouillage après `Settings::password_lock_minutes` (5/15/60/0 = à la fermeture). Fenêtre privée : lecture seule (pas d'offre ni de modification).
- Test : `test-ui.ps1`, `eval` « pwN: » lus par `PwEval`, hors indices ; mots de passe de test générés par le script.

## Menus et vocabulaire
- Apparence : `docs/design-system/` décrit les tokens (couleurs par teinte d’espace, typo, espacements, rayons, ombres, mouvement, icônes), les composants et les règles visuelles de `ui/`. Pour une nouvelle valeur, prendre le token retenu (voir `unification.md`) plutôt qu’une valeur en dur.
- Raccourcis affichés : `bhKeys('mod+shift+e')` → « ⌘⇧E » sur macOS, « Ctrl+Shift+E » ailleurs ; `bhKeyParts` pour des `<kbd>` (`ui/keys.js`, injecté par `views::ui_view`, `panel_view`). Jamais de `⌘`/`Ctrl` en dur. Les actions de palette passent `keys` (spécification) plutôt que `hint`.
- Icônes et tokens : `ui/icons.js` (table SVG partagée, `bhIcon(nom, 'xs'|'sm'|'md'|'lg')` = 12/16/20/24 px, trait 1,5 px non mis à l'échelle, `<i data-icon>` hydraté au chargement) et `ui/theme.js` (variables `--bh-*` générées depuis le tokens.json du design system, sous `:where()`), injectés par `ui_view` et `panel_view`. Pas d'emoji ni de glyphe de police comme icône (sauf emoji d'espace/dossier choisis par l'utilisateur ; boutons de fenêtre Windows en 10 px). Les tests lisent `svg.dataset.i` (nom de l'icône), pas le texte. Scénario macOS : `scripts/test-settings.sh`.
- Menus (`cmd: 'menu'`, rendus par `palette.html`) : `icon` est un nom de la table de `ui/icons.js` (SVG en traits) ; `confirm: '…'` arme l'élément au premier clic. Actions de palette : `confirm` demande une seconde Entrée.
- Vocabulaire : « quick access » (tuiles épinglées), « put to sleep » (`close_instance`, l'onglet garde sa place ; c'est aussi le bouton « − » d'un onglet de dossier), « close » (`close`, archive), « delete » (conteneur + contenu, avec confirmation). Casse de phrase.
- Accès rapide : « Close tab » et la commande `close` (gardée côté Rust) ne retirent jamais une tuile, ils la mettent en veille ; seul « Remove from quick access » la retire. Clic molette (`auxclick`) : veille pour un onglet d'accès rapide ou de dossier, `close` sinon. Pas de bouton son sur les tuiles (il masquait l'icône).
- « Lock URL » a été retiré (domaine, IPC, interface) ; un ancien champ `locked` de `workspace.json` est ignoré au chargement (test dans `infrastructure`).
- PiP : la page envoie `pip_left { paused }`. Sur macOS (WebKit), fermer la fenêtre flottante met la vidéo en pause (rien ne change) et « retour à l'onglet » la laisse jouer : `pip_left` affiche alors son onglet (`domain::pip::back_to_tab`), seulement pour une vidéo que le shell a mise en PiP (`pip.tab`, `pip_on_blur`, bouton PiP de la carte/palette via `pip_toggled`) et pas pour une sortie que le shell vient de provoquer (`pip.shell_exits`, 1,5 s). WebKit repasse la vidéo en `inline` dès le clic mais n'envoie `webkitpresentationmodechanged` qu'après l'animation de la fenêtre : la sonde (500 ms) le voit d'abord et oublie `pip.tab` ; `pip.faded` garde l'onglet `pip::LEFT_EVENT_GRACE` (3 s) pour que ce `pip_left` tardif compte encore (cas « late » de `test-pip.sh`). Sous WebView2 c'est pareil pour une vidéo fichier ou réseau (vérifié avec de vrais clics : la pause arrive avant `leavepictureinpicture`) ; seul un flux en direct (`srcObject`, champ `stream` de `pip_left`) n'est pas mis en pause à la fermeture, et rien ne change alors d'onglet. Barre latérale masquée : un clic molette (`open_in_background`) la fait sortir `LINK_REVEAL`. Adresse saisie dans un onglet (`navigate`) : `Running::pending` la montre aussitôt dans la barre avec la ligne de progression (dans la page une fois qu'elle a commencé, 0.8.25), jusqu'au début de la page, à l'échec, à l'invite d'authentification ou au téléchargement (`LOADING_MAX` au plus). Menu du clic droit de la barre latérale (`createMenu`) : « New black hole tab » (action `burner_tab`). Test : fin de `test-ui.ps1` (`ad1`, `ad2`, `bm1`, `bm2`). Le ✕ de la carte ferme aussi la fenêtre (`MEDIA_CLOSE_JS`) et la carte reste fermée au moins 2 s (`DISMISS_GRACE`). `paused` est une pause vue pendant le PiP (`__bhPaused` dans `media_devices.js`), pas `video.paused` à la sortie : WebKit met en pause une vidéo muette (tuiles d'un appel Meet) qui revient dans un onglet masqué, et cet événement `pause` n'arrive qu'après `webkitpresentationmodechanged`. Test : `scripts/test-pip.sh` (cible devtools `eval tabN`, page `call.html` = flux muet en direct).
- Fenêtre Easel (Réglages dans la carte de page depuis 0.8.18, `body.embedded` sans barre) : `panel_window_attributes` (Windows : sans bordure + ombre + icône ; macOS : barre de titre transparente, 84 px réservés aux boutons système). La page dessine la barre du haut (38 px, `--tint`/`--glow` de la couleur de l'espace, boutons réduire/agrandir/fermer, glisser = déplacer, double-clic = agrandir) via `UiCommand::PanelWindow { window, action }` ; l'état « agrandi » suit `WindowEvent::Resized`. Dans l'Easel embarqué en onglet, cette barre est masquée.
- Suppression d'un espace ou d'un dossier : onglets archivés (`archive_tab` cherche dans tous les espaces) puis `drop_view`. Renommer un dossier : `UiCommand::EditFolder` → `bh.editFolderId`.
- Annuler une suppression : instantané domaine (`snapshot_space` / `snapshot_folder` → `Removed`) gardé 15 s dans `Running::undo`, notice `Undo` avec bouton (`notify`, voir « Notices »), `UiCommand::UndoRemoval` → `undo_removal` (retire aussi les entrées d'archive). Les routes « Air Traffic » d'un espace supprimé ne reviennent pas. « Close other tabs » : `snapshot_tabs` → `Removed::Tabs`, même Annuler (bureau et Android), sans confirmation. Bibliothèque et Easel : `app/trash.rs` (`Trashed` : surlignage `Highlights::take`, veille `Watches::take`, onglet archivé, carte `Easels::take_item`, Easel `Easels::take`), `UiCommand::UndoTrash`, un seul élément gardé `UNDO_FOR`. Éditeur de mot de passe des Réglages : ouvert jusqu'à `bh.passwordSaved` (`null` = enregistré, sinon `{field, text}` affiché sous le champ) ; l'Annuler d'une suppression est aussi dans `#pwstatus` (champ `undo` de l'état).
- Menu d'onglet : `reload_tab`, `duplicate_tab` (`Workspace::duplicate_tab` : nouvel onglet libre sous l'original, en tête des onglets libres pour une tuile ou un onglet de dossier, trou noir si l'original l'est ; jamais `open`, qui réutilise l'onglet de même adresse), `copy` (copie via la palette), `move_tab_to_space` (la vue est libérée : nouveau profil de cookies), `close_other_tabs` (onglets libres seulement, archivés). Menu d'espace : `set_space_color` (`#rrggbb`, pastilles `color` dans les menus).
- Palette : actions `key:<raccourci>` (rejouées par `shortcut`), `open_history`. `UiCommand::OpenLibrary { section }` : une section connue (`LIBRARY_SECTIONS`) passe par `library_jump`, que la Bibliothèque applique une fois ; la carte de téléchargement de la barre latérale ouvre `downloads`. À l'ouverture aucune ligne n'est sélectionnée (`sel = -1`) : Entrée passe par `submit` (adresse intacte en mode `url` → `reload`, sinon `navigate`/`open` du texte) ; la première ligne est sélectionnée dès qu'on tape, ↓/↑ partent du bord.
- Sonde média : une vidéo coupée par le site compte si elle joue, ne boucle pas et couvre ≥ 25 % de la fenêtre (Twitch démarre muet) ; `MEDIA_MUTE_JS` rend alors le son à ce qui joue.

## Liens, boucliers et capture en direct (0.7.18)
- Liens dans les pages (`ui/page_guard.js`, cadre principal, capture avant la page, `composedPath` pour traverser les shadow roots) : clic molette ou Ctrl/⌘+clic → `PageEvent::OpenLink` → `open_in_background` (l'onglet s'ouvre en haut de la liste et charge, la page affichée reste, Air Traffic respecté sans changer d'espace) ; clic droit → `PageEvent::LinkMenu` → `link_menu` (menu à la position de la page, via `view_origin` + `show_menu`). Les entrées du menu sont des `UiCommand` (`open_link` avec `target` tab/front/window/private, `open_peek`, `copy`) : une page ne peut ni ouvrir de fenêtre BlackHole ni choisir un Easel.
- Popups de connexion (`window.open` des pages, `with_new_window_req_handler` de `build_tab`) : règle pure `domain::popup::new_window(raw, sized)` → `Popup` (`about:blank`/vide, taille ou position demandées, ou page de connexion connue en https : `accounts.google.com`, `login.microsoftonline.com`, `appleid.apple.com`…, `github.com/login/oauth/`…), `Tab` (lien `target=_blank`, `window.open(url)` simple : onglet en arrière-plan sans opener, comme avant) ou `Deny` (`javascript:`, `data:`, `blob:`, `file:`). `Popup` → `NewWindowResponse::Allow` : le moteur ouvre la popup sur la configuration de l'onglet (même `Store`/profil), `window.opener` est gardé (GSI : sans lui « Relay function on opener iframe not found » et la connexion ne revient jamais). macOS (vendor/wry `wry_web_view_ui_delegate.rs`) : `NSWindow` créée par wry, `UIDelegate` de la popup = celui de l'onglet pour `webViewDidClose:` (`window.close()`), fenêtres fermées gardées dans `closed_windows` et libérées plus tard (jamais dans `windowWillClose:`), popup fermée avec son onglet, aucun `unwrap()`. La popup partage le `WKUserContentController` (scripts injectés) de l'onglet : `WryWebViewDelegate::owner` ignore les messages IPC d'une autre vue (sinon la page de connexion passerait pour l'onglet : remplissage, enregistrement de mot de passe). Windows : popup par défaut de WebView2 (sans nos scripts ni IPC). Les navigations de la popup ne passent pas par `PrivacyPolicy` (pas de délégué de navigation). Scripts injectés dans les pages (`autofill.js`, `find.js`…) : jamais `innerHTML` ni chaîne HTML (Trusted Types `require-trusted-types-for 'script'` de accounts.google.com : `TypeError: This assignment requires a TrustedHTML` et script interrompu) ; nœuds créés un à un, styles par `adoptedStyleSheets` (repli `<style>` + `textContent`). Test : `scripts/test-popup.sh` (serveur 47990 : popup avec `opener`, `postMessage`, `window.close()` ; page sous Trusted Types sans violation). Les scripts lancés par le harnais (`evaluateJavaScript`) comptent comme un geste utilisateur dans WKWebView.
- Popups d'extension (Windows) : `open_ext_popup` sous le bouton (`ExtAction`), ajustée au contenu (`POPUP_MIN` / `POPUP_MAX`), coins arrondis (`round_ext_popup`, à refaire à chaque redimensionnement). Une fenêtre qu'une extension demande depuis un onglet (déverrouillage Bitwarden) repart en `Msg::OpenTab`, jamais `OpenTabFrom` : `is_extension_popout` en fait une popup sous le bouton de l'extension (`ExtAnchors` envoyé par `bar.html`, `ext_anchor`), pas un onglet. Nom du bouton : `__MSG_clé__` lu dans `_locales/<default_locale>` (`win::localized`). Test : `scripts\test-ext.ps1`.
- Recherche sur la sélection (0.7.19) : clic droit sur du texte sélectionné (point du clic dans les rectangles de la sélection) → `PageEvent::LinkMenu { url, text }` (`url` vide hors lien, `text` tronqué à 1000 caractères par la page) → `link_menu`. `domain::selection` : `clean_selection` (500 caractères max, espaces normalisés, caractères de contrôle et invisibles bidi/largeur nulle retirés), `selection_action` (→ `Open` si `omnibox::web_address` lit une adresse http(s) plausible — guillemets/ponctuation autour retirés, ni courriel, ni schéma autre que http(s) avant `://`, TLD alphabétique ≥ 2, IPv4 écrite en entier — sinon `Search` avec le moteur des Réglages, encodage `application/x-www-form-urlencoded`), `excerpt` (libellé ~30 caractères, inséré par `textContent`). Entrées : « Search for “…” » ou « Open address “…” » (`open_link` `target: "front"`, nouvel onglet affiché) et « Copy » ; un élément de menu peut porter `background` (Ctrl/⌘+clic ou clic molette dans `palette.html`) → `target: "tab"` (`open_in_background`). Champs de saisie (`input`, `textarea`, `contenteditable`) et iframes gardent le menu du moteur (copier/coller, orthographe) ; Maj+clic droit aussi. macOS : `preventDefault` sur `contextmenu` masque le menu natif de WKWebView, comme pour le menu des liens (non vérifié sur macOS).
- Boucliers façon Brave : `PrivacyPolicy::with_ads(trackers, ads)`, interrupteurs `set_blocking` / `set_ad_blocking` et `Shields { hide_ads, video_ads, cookie_banners }` partagés par les clones (atomiques). `ui/shields.js` reçoit la configuration à la création de la vue (`views::shields_script`) : feuille construite (CSP) avec des sélecteurs génériques connus (AdSense/GPT, Taboola, Outbrain, rendus de pub YouTube ; CMP OneTrust, Cookiebot, Quantcast, Didomi, Usercentrics…), défilement rendu si un bandeau était là, saut des pubs YouTube (`.ad-showing` → fin de la pub + bouton Skip, sans toucher au son). Les vues de réserve sont recréées après un changement. Tests : domaine (listes et clones), application (persistance), infrastructure (listes intégrées sans domaine de site).
- Blocage des requêtes par plateforme. macOS : `PrivacyPolicy::active_lists()` → `macos-webext/rules.rs` (`rules_json` : une règle `url-filter` par hôte, sous-domaines compris, `load-type: third-party` pour laisser la page ouverte elle-même au gestionnaire de navigation et à ses exceptions ; `compile_hosts` au démarrage via `views::prepare_blocking`, `block_hosts` ajoute la liste au `WKUserContentController` de la vue, tout de suite ou à la fin de la compilation). Une vue garde les listes de sa création. Décompte : vendor/wry implémente la méthode privée et facultative `_webView:contentRuleListWithIdentifier:performedAction:forURL:` (`blockedLoad`) → `WebViewBuilderExtDarwin::with_content_blocked_handler` → `PageEvent::Blocked`. Windows : `win::block_hosts(view, hôtes, décision)` pose deux filtres `AddWebResourceRequestedFilter` par hôte listé (`*://hôte/*`, `*://*.hôte/*`, toutes listes confondues : `PrivacyPolicy::listed_domains`) ; seules ces requêtes appellent le gestionnaire, qui lit l'adresse de la page (`Source` de l'émetteur, jamais la vue : pas de cycle), écarte le même site (`domain::is_third_party`) et demande `errors::with_exceptions(policy.evaluate(..))` : interrupteurs et exceptions de l'utilisateur valent tout de suite ; refus = réponse 403 créée par l'environnement, hôte compté par `PageEvent::Blocked`. `views::guard_script` donne une liste vide à `page_guard.js` sur les deux systèmes : le script ne remplace plus `fetch`, XHR, `setAttribute` ni les setters `src`, et n'observe plus le DOM (coût sur chaque appel de Docs, YouTube, Meet) ; son code de blocage ne sert plus qu'aux cibles sans moteur natif. Test macOS : `scripts/test-blocking.sh` ; côté Windows, vérifié par Clippy en compilation croisée seulement (depuis macOS : `cargo clippy --workspace --exclude blackhole-android --all-targets --target x86_64-pc-windows-msvc` avec `CC_x86_64_pc_windows_msvc`/`AR_x86_64_pc_windows_msvc` pointant vers un faux compilateur qui crée des fichiers vides), comportement non testé.
- Zoom de page : par site (`settings::site_key`, `www.` ignoré), en pourcentage (`ZOOM_STEPS`, `next_zoom` dans le domaine, 100 jamais stocké, au plus `MAX_SITE_ZOOMS` sites) dans `Settings::site_zoom` (`settings.json`, `site_zoom`, niveaux invalides écartés à la lecture), enregistré par `BrowserService::set_site_zoom`. App : `zoom_of(tab, host)`, `zoom_active` réapplique à tous les onglets de la fenêtre, `apply_zoom` à chaque changement d'adresse (`page_url`). Onglet trou noir : `Running::zoom`, en mémoire seulement.
- Sonde média (`service_media`) : un onglet n'est interrogé que s'il l'a signalé depuis moins de `MEDIA_HINT_FOR` (5 s : `PageEvent::Media` envoyé au plus une fois par seconde par `media_devices.js` sur `play`/`pause`/`volumechange`…, `getUserMedia`, `RTCPeerConnection` ; `Msg::Loaded`, `PageEvent::Location`, `media_ctl`), si sa dernière sonde joue, est en PiP ou en appel, ou s'il est `pip.tab`. `media_devices.js` ne tourne pas sur les éditeurs Google (docs, sheets…) : ils ne sont jamais sondés. `pip_on_blur` et le PiP au changement d'onglet ignorent un onglet en appel (`in_call` : caméra ou micro ouverts). La croix de la carte lecteur d'un appel (caméra, micro ou connexion `call`, champ `call` de `media_json`) quitte l'appel : `views::CALL_HANG_UP_JS` → `__bhRtc.hangUp()` clique le bouton « quitter » de la page (Meet `jsname="CQylAd"`, Teams, Jitsi, ou `aria-label` « Leave call »/« Quitter l’appel »…), puis, si l'appel tient encore 1,5 s après, arrête les pistes et ferme les `RTCPeerConnection` ; titre « Leave the call » (`sidebar.leave_call`).
- Renderer mort : `with_on_web_content_process_terminate_handler` (macOS) ou `win::on_renderer_gone` (Windows, `ProcessFailed` de type `RENDER_PROCESS_EXITED`) → `Msg::Crashed` → `on_crashed` : onglet à l'écran rechargé une fois, toast `toast.page_crashed` s'il retombe en moins de `CRASH_LOOP` (30 s) ; onglet caché : `drop_view`, rechargé à l'affichage. Non testé (pas de moyen sûr de tuer un seul renderer depuis le harnais) sur les deux systèmes.
- Coins arrondis (macOS) : la boucle ne réapplique `round_cards` que sur les cartes à l'écran, et `round_view` n'écrit le calque que s'il a changé. Le masque lui-même reste (le fond de la carte est un dégradé dessiné par la page de la barre : des caches d'angle ne le reproduiraient pas).
- Capture en direct : `live_picked` → menu « Add the live area to » (Easels de l'espace, puis New Easel) → `LiveCaptureTo { easel }` → `capture_live(…, easel)`.
- Barres de défilement : `ui/theme.js` (feuille construite, pouce translucide, couleur de l'espace au survol), injecté dans `ui_view`, `panel_view`.
- `test-easel.ps1` : un `eval` qui renvoie `__skip…` est exclu des indices des `Check`.
- Meet et Google Agenda (sans OAuth) : l'utilisateur colle l'« adresse secrète au format iCal » de chaque agenda (Réglages > Calendar). `infrastructure::calendar` : `is_valid_source` (https, `calendar.google.com`, `/calendar/ical/…​.ics`), `CalendarSources` (fichier chiffré `calendars.bin`, même clé que le coffre via `Vault::named` / `read_json` / `write_json`), `fetch` (timeout, pas de redirection, 16 Mo max), `parse(ics, from, to)` (lignes repliées, TZID via `chrono-tz`, heures flottantes = fuseau local, RRULE + EXDATE via `rrule`, occurrences déplacées/annulées via RECURRENCE-ID, journées entières et événements sans salle Meet ignorés). `domain::meetings` : `Meeting`, `find_meet_link`, `room_code`, `upcoming`, `next_soon`, `starting`. App `meet.rs` : `Calendar` (thread de synchro, statut, carte), `service_calendar` (appelé par `about_to_wait`), `open_meet`, `meeting_actions` (palette : New Google Meet, Join …), `calendar_json` (Réglages, sans les adresses), `handle_calendar`. `notify_action` affiche une notice avec bouton (Join, Undo ; voir « Notices »). Carte « prochaine réunion » : de `CARD_LEAD` (15 min) avant à `CARD_KEEP` (5 min) après le début (`meetings::next_soon`), puis la suivante ; son icône devient une croix au survol/focus (`UiCommand::MeetingDismiss { url, start }` → `Calendar::hidden`, en mémoire, cette occurrence seulement).
- Outils de développement : `with_devtools(true)` pour les vues d'onglet seulement (les pages d'interface restent en debug), feature `devtools` de wry dans `crates/app/Cargo.toml` ; raccourci `devtools` dans `shortcuts.js` (F12 / Ctrl+Maj+I / ⌘⌥I), action de palette `key:devtools`, entrée « Inspect page » du menu de l'onglet affiché.
- Rapport de confidentialité (bouclier de la barre latérale) : `UiCommand::PrivacyReport { x, y }` → `privacy_report` : `BlockLog::for_site(site)` (par site, 100 sites × 100 hôtes max), classement pub/traqueur via `PrivacyPolicy::evaluate`, note `privacy_grade(trackers, ads)` (A à E, un traqueur pèse deux pubs), menu ancré.
- Rangement automatique : domaine `tidy_suggestion(min)` (onglets libres groupés par `site_of_host`, sous-domaines et suffixes pays repliés), `tidy` (réutilise un dossier racine du même nom), `untidy`. App : `tidy_json` (carte dès 6 onglets libres, site à 3 onglets), commandes `Tidy` / `Untidy` / `DismissTidy`, action de palette `tidy`.
- Copilote de réunion : `Meeting.agenda` (description sans le bloc d'accès Google `-::~`) et `attendees` (CN ou adresse, ressources exclues) lus par `calendar::parse` ; `open_meet` d'une salle de l'agenda → toast « Take notes » (`UiCommand::MeetingNotes`, sans question si l'Easel existe déjà) → `open_meeting_notes` (Easel « Notes · titre · jour », note `notes_template`) affiché en split à droite de l'onglet Meet : `easel_beside: Option<(TabId, easel)>`, `easel_side()` (onglet affiché + Easel courant), `content_rects`/`place_panels`/`easel_origin` (vues live), volet droit de `splitbar` marqué `easel` ; fermer le volet droit retire l'Easel, le gauche l'ouvre en onglet ; fenêtre Easel si aucun onglet n'est dans la salle (salle reconnue par `meetings::in_room` : Meet réécrit l'adresse en `/meet/CODE` ; Join réutilise un onglet déjà dans la salle). Devtools : étapes `meeting URL TITRE` (réunion en cours sans agenda) et `state` (`[dev] state -> {…}`), cible `eval splitbar`. `calendar::parse(ics, owner, …)` écarte annulés et refusés par le propriétaire (Google garde dans l'export iCal une invitation supprimée, refusée).
- Veille de zone : `domain::watch` (`Watches::add/due/record/check_soon/mark_read`, première lecture = référence, `Reading::Changed { before, after }`), `infrastructure::store::WatchStore` (`watches.json`, URL http(s) seulement), `app/watching.rs` (`watch_area`, `service_watches` : vue cachée `views::live_view`, `read_js(selector)`), Library « Watched Areas », toast « Open » sur changement. `watches_json` envoie `checking` (« Check now » demandé ou vue cachée en lecture, jusqu'à la réponse) : la ligne dit « checking… », puis « checked just now » avec un éclat (`.fresh`) ; « il y a » avance sur place (`.ago`, 15 s). Le bouton Easel (et la carte de l'accueil) ouvre « Add to » : Easels de l'espace (courant coché, `easelCurrent` / `now.easel`) et « New Easel » → `watch_to_easel { id, easel | new }` (`pick_easel`). Library > Easels & Notes et le menu d'un Easel de la barre latérale ont « New Easel » (action `new_easel`, ouvert en onglet).
- Comparateur : `views::COMPARE_JS` (`ui/compare.js`), `app/compare.rs` (`compare_start`, `service_compare`, attente 1,5 s), palette `mode: 'compare'` (`showCompare` / `drawCompare`, 5 colonnes cochées par défaut, prix le plus bas et meilleure note en vert), entrée « Compare with the other tabs » du menu d'onglet.
- Continuer sur l'autre appareil (réseau local, sans serveur ni compte) : `domain::handoff` (`format_code` / `parse_code` : 12 caractères Crockford = 60 bits, `can_continue` http(s), `has_position` / `resume_url` : `t=` pour YouTube et les VOD Twitch, `is_fresh` 10 min, `Seen` anti-rejeu). `infrastructure::handoff` : `spawn(Config)` → `Handle` (`Request` / `Event`), thread unique, socket UDP ouverte seulement s'il y a un appareil appairé ou un appairage en cours (`PORT` 47807, sinon port éphémère si une autre fenêtre le tient : elle envoie mais ne reçoit pas), paquets `BH1` + type + nonce + ChaCha20-Poly1305 (en-tête authentifié), enveloppe `{from, at, t, …}` (`hello`, `tab`, `ack`, `unpair`, `offer`, `accept`). Appairage : clé du code = SHA-256 étiré 100 000 fois, chaque côté envoie une moitié aléatoire, clé de paire = SHA-256(clé du code, moitiés) stockée dans `devices.bin` (`Vault::named`). Découverte par `hello` diffusé (255.255.255.255) puis unicast ; un onglet est renvoyé jusqu'à l'`ack` (6 essais). Test bout à bout en boucle locale dans `infrastructure`. App `handoff.rs` : `Nearby` (pas en fenêtre privée ; `Config::loopback` sous la feature `devtools`, pour éviter l'invite du pare-feu), nom d'appareil par plateforme (`COMPUTERNAME` / nom d'hôte), position lue par `POSITION_JS`, commandes `device_show_code`, `device_join`, `device_cancel_pairing`, `device_forget`, `continue_on`, état `devices` (menu d'onglet) et Réglages > Devices (jamais les clés), action de palette `continue:<id>`. macOS : `NSLocalNetworkUsageDescription` (Info.plist de `package-macos.sh`). L'échange réel Windows ↔ Mac n'a pas été testé.
- Réseau : `ureq` avec `native-certs` (magasin de certificats du système) ; les téléchargements du catalogue et des mises à jour gardent leur vérification SHA-256.

## Liens propres, split pilote, trou noir, surlignages, Recall, Wormhole

Spécifications : `docs/specs/0.8.6-features.md`. Scénarios Windows : `scripts\test-features.ps1`
(contrôles nommés `cl1:`, `bh2:`… lus par leur nom, 17 captures `f01`–`f13`, dont `f04a` le split en train de s’ouvrir, `f13a` une tuile montée sans rotation (0.8.16), `f13b` le réglage Flat / 3D, `f13c` l'accueil en Flat ; réseau requis ;
le Wormhole y est absent, couvert par le test en boucle locale de `handoff`). macOS : à écrire.

- Liens propres : `domain/clean_link.rs`, bouclier `clean_links` (`Shields`, bit 8), appliqué par
  `PrivacyPolicy::evaluate` avant le passage en https ; la cible d'une redirection est rejugée
  (jamais un contournement des listes). `Msg::LinkCleaned` compte pour le rapport de confidentialité.
- Split pilote : `domain/pilot.rs`, `app/pilot.rs` (`Running::pilot`, session). La page ne fait
  que signaler un clic (`PageEvent::PilotLink`, drapeau `__bhPilot` posé par le shell) : c'est le
  shell qui vérifie le pilote et évalue l'adresse. Reposer le drapeau à chaque chargement.
- Style de l'interface, Flat ou 3D (0.8.11) : `Settings::depth` (vrai par défaut), commande
  `set_depth`, Réglages > Appearance (`#style`), champ `depth` de l'état des pages, qui posent
  `body.flat`. `ui/depth.js` (injecté avec `theme.js` par `ui_view` et `panel_view`) : une page
  déclare ce qui réagit au pointeur (`bhDepth('.fav', 13)` ; depuis 0.8.16 rien ne tourne, la carte monte seulement, voir 0.8.16), le script pose la classe
  `bh-lean` et `--rx` / `--ry` / `--mx` / `--my`, la page écrit la transformation. Penchent :
  tuiles et lignes de l'accueil, tuiles d'accès rapide, tuiles et cartes de la Bibliothèque,
  notes et clips de l'Easel (jamais une carte en direct : sa vue native ne tourne pas, ni une
  carte en cours de saisie, de glisser ou de choix de couleur). La palette et les menus
  s'ouvrent en perspective (`palette-in`, `menuin` ; variantes `-flat`). Rien ne penche en
  `body.flat`, `noanim`, `still`, `dragging`, sous `prefers-reduced-motion` ni au doigt. Android
  non concerné. Le 3D se voit aussi au repos : `depth.js` donne les ombres du relief
  (`--bh-relief-panel` / `-raised` / `-key` / `-key-down` / `-sunk`, `--bh-sheen`, `--bh-sheen-soft`),
  chaque page les pose sous `body:not(.flat)` (sections et cartes détachées de la page, tuiles
  éclairées d'en haut, champs et choix segmentés en creux, barre du haut comprise ;
  l'ombre d'une carte penchée fuit le pointeur). Le relief n'est pas un mouvement : il reste
  avec les animations coupées. Une surface dont le `box-shadow` dit un état (téléchargement
  terminé, réunion en cours) ne reçoit que `--bh-sheen-soft`. Le relief est bas par choix
  (arêtes d'un pixel, ombres courtes et légères, reflet faible ; tuiles penchées de 12° au plus) :
  il doit se lire comme de la profondeur, jamais comme un décor. Ne pas le remonter.
  Barre latérale : onglets et accès rapide restent discrets (on les regarde toute la journée,
  et il peut y en avoir beaucoup) : une tuile d'accès rapide, l'onglet affiché, la ligne survolée
  et « New tab » n'ont que `--bh-relief-row` (deux arêtes fines, sans ombre portée ni reflet) ;
  un onglet au repos n'a pas de bande, les lignes ne penchent pas, une tuile penche de 12° au
  plus. Boutons du pied détachés et enfoncés à la pression, séparateurs en rainure. Toute règle
  `body:not(.flat) .tab…` doit laisser gagner `.tab.split` (trait d'accent) et `.tab.gliding`.
  Test : `scripts\test-features.ps1` (`dp1`–`dp12`).
- Mouvement des volets (0.8.11) : `app/src/shell/pane_motion.rs` (pur, testé). Un split qui apparaît
  (`split_shown`, appelé par `sync` : paire d'onglets nouvelle, pas un simple changement de focus)
  fait glisser le volet droit depuis la droite ; un lien pilote (`pilot_sent`) fait monter le
  volet de destination. Les vues ne sont que déplacées (`shifted` dans `layout`), jamais
  redimensionnées ; la boucle les cadence avec la barre latérale (`tick_motion`, `FRAME`). La
  barre de split joue sa part quand `opened` ou `sent.n` de son état changent (`play`, classes
  retirées à la fin : les volets sont redessinés à chaque rendu). Rien sans le réglage
  `animations`. Devtools : `state` donne `paneMotion`, `splitOpened`, `pilotSent`. L'Easel à
  côté d'un onglet ne bouge pas.
- Carte en direct de l'Easel (0.8.11) : `Item::viewport` = largeur de la page au moment du
  choix (`vw` de `live_pick`, non fiable : `Easels::set_viewport` l'écarte hors 200–8000) ;
  `apply_slot` zoome la vue (`easel::live_zoom` = largeur de l'emplacement / `viewport`, borné
  0,25–3) pour que la page garde sa mise en page et que la zone remplisse la carte.
  `easel::BAR_HEIGHT` (54) doit rester égal à la barre + le pied de `.card.live` dans
  `easel.html`, et `.slot` sans marge latérale. Cartes d'avant : `viewport` 0, zoom 1.
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

## Raccourcis, recherche et glisser-déposer
- Windows : les vues d'onglet désactivent les raccourcis navigateur de WebView2 (`with_browser_accelerator_keys(false)` dans `views::build_tab`), sinon Ctrl+F/G/P, F5, Alt+←/→ n'atteignent jamais la page. `ui/shortcuts.js` les reprend (F5 → `reload`, Ctrl+F5 → `r`, Alt+←/→ → `[`/`]`).
- Ctrl/⌘+F/G sont traités après la page (écouteur en phase bulle) et seulement si elle ne les a pas pris (`defaultPrevented`) : Google Docs garde sa propre recherche.
- Barre de recherche (`ui/find.js`) : surlignage par l'API CSS Custom Highlight (`::highlight(bh-find)`), sans toucher la sélection ni le focus ; repli `window.find` sinon. Recherche sur le texte visible mis bout à bout (table nœud/décalage par caractère, espaces ramenés à un, limite de bloc = espace) : les `Range` peuvent commencer et finir dans deux nœuds ; 80 ms d'attente à la frappe.
- Onglets : Mod+1…8 / Mod+9 → `shortcut` `tab_N` → `Workspace::nth_tab` ; Ctrl+Maj+Tab et Mod+Maj+[ / ] (`e.code` BracketLeft/Right) → `prev_tab` / `next_tab`. macOS : sous-menu natif Fenêtre (`install_menu`, `set_as_windows_menu_for_nsapp`).
- Page « Trop de redirections » (bureau) : bouton « Effacer les cookies de ce site et réessayer » (`clearCookies` dans la config, `PageEvent::ErrorClearCookies` → `error_clear_cookies` : `cookies()` / `delete_cookie` de wry sur le `Store` de l'onglet, filtrés par `error_page::cookie_of_site`, puis `error_retry`). Erreurs réseau de la mise à jour : messages `msg.update_*` (`updater::reach_error`), rendus dans la langue de l'interface.
- Une vue masquée mais pleine taille capte encore les glisser-déposer de la page (macOS) : la palette, la Bibliothèque et le panneau Easel sont parqués en 1×1 quand ils sont masqués (`palette_rect`, `place_panels`).
- Animation de la sidebar : pendant le mouvement, la vue de page garde sa largeur finale et ne fait que glisser (`sliding`) ; redimensionner une WebView à chaque image la fait se remettre en page et saccader. La boucle passe en `ControlFlow::Poll` cadencé par `FRAME` (8 ms, `thread::sleep` précis) : le minuteur système derrière `WaitUntil` (15,6 ms sous Windows) rend les images irrégulières.
- Cartes lecteur et téléchargement (`renderMedia`, `renderDownloads` dans `ui/index.html`) : mises à jour en place (signature / nœuds indexés par id), la carte lecteur reste sur le même onglet tant qu'il est listé. Styles sur les tokens (entrée `card-in`, téléchargement terminé en vert avec un éclat, `DOWNLOAD_LINGER` 8 s). Une carte terminée ou échouée a un bouton de fermeture (`UiCommand::DismissDownload`, refusé pour un téléchargement en cours) ; elle sort par `dl-out` (`dlLeave`), comme une carte dont le délai est écoulé. Un onglet ouvert pour un fichier seul (`Msg::DownloadStarted { tab }`, encore en `splash`) sort de `awaiting` (pas de « Site injoignable ») et se ferme à la fin du téléchargement (`downloads.only_tabs` ; le fermer avant annulerait le téléchargement). Test : `scripts/test-library.sh` (HOME jetable : le fichier ne va pas dans le vrai `~/Downloads`). Côté Rust, `media::Linger` garde le dernier état avec média 5 s (pubs, lecteur recréé) ; la sonde `MEDIA_PROBE_JS` compte une vidéo en mise en mémoire tampon comme en lecture.

## Logo et splash de l'accueil

- `ui/home.html` et `crates/android/ui/mobile.html` portent le même logo et le même splash (à garder en phase). Logo : calme au repos (anneau en 9 s), réaction au survol de `.brand` (bureau) ou au toucher (`.gulp`, 1,3 s, Android) : disque gonflé, second anneau rapide (en pause tant qu'il est invisible), étoiles avalées ; le retour au repos a ses propres transitions, plus lentes et sans rebond, et les étoiles entrent par `mote-in` après le splash.
- Splash : étoiles qui tombent, éclair, onde de choc, naissance du trou, nom lettre par lettre, puis `.land` : le fond s'efface et `.core` vole jusqu'au logo de la page (`--lx`, `--ly`, `--ls` mesurés par le script), qui prend le relais quand `body.splashing` tombe. Android sans accueil à l'écran : `.out` (fondu). Seuls `transform` et `opacity` sont animés ; pas de splash quand les animations sont coupées.
- Maquettes et variantes écartées : `docs/logo/` (`proposition.md`, `maquettes.html`, `maquettes-lentille.html`).

## Android (version allégée, `docs/android.md`)
- Architecture : coque Kotlin (`MainActivity` : vues d'onglets `TabView` sous une vue d'interface transparente `mobile.html` servie par `WebViewAssetLoader` ; `Native` : JNI) + cœur Rust `crates/android` (`core.rs` : `BrowserService` du bureau, onglets privés `PRIVATE_BASE`, vues vivantes `live.rs` `MAX_LIVE` = 4, pages d'erreur, `Blocker` partagé pour `shouldInterceptRequest`). wry n'est pas utilisé sur Android (une WebView par Activity, tao au lieu de winit, pas de gestionnaires BlackHole) ; `vendor/wry` et `crates/app` restent inchangés.
- Protocole (`bridge.rs`) : `MobileCommand` (interface, listener `bh` limité à `https://appassets.androidplatform.net`, cadre principal), `PageEvent` (listener `bhPage` des onglets, toutes origines, non fiable, seulement `error_menu`, accepté si l'onglet affiché montre notre page d'erreur), `NativeEvent` (WebViewClient, cycle de vie), `Effect` (open/show/load/html/nav/drop/ui/ssl/cancel/clear_profile/download/external/exit/desktop_mode/web_prefs/permission/update_*). Messages gardés par la coque : `surface` (zone tactile déclarée par l'interface, thème des barres système, animations), `thumbs`, `peek`/`peek_end` (glisser la barre), `copy`, `share`, `open_downloads`, `find`/`find_next`/`find_clear`, `print`, `add_to_home`, `default_browser`.
- Rendu de `mobile.html` : un message `state` arrive à chaque événement de page ; chaque partie compare la signature de ce qu'elle affiche (`changed(part, deps)`) et les listes (onglets, grille, suggestions, historique, appareils, téléchargements, pastilles) passent par `sync(box, items, key, make, update, anim)` : nœud gardé tant que sa clé existe (ses gestionnaires lisent `node.__it`), animation d'entrée par la classe `.in` (`enter`), une seule fois. Jamais de `replaceChildren` sur une liste tirée de l'état. Icônes : `putTile` garde la tuile inchangée (`iconStamp`). Vignettes : seule la carte de l'onglet est touchée (`drawShot`), JPEG encodé hors du thread principal (`TabView.snapshot` / `encode`). `surface` n'est envoyé que s'il change. `NativeEvent::Blocked { tab }` → message `blocked` (deux nombres) au lieu de tout l'état. `insets` : `bottom` (barres système) et `keyboard` (hauteur) séparés ; `--bb` porte la barre (clavier d'une page ou de la recherche dans la page), le clavier de nos champs la recouvre et la page n'est pas redimensionnée (`kbOurs`).
- Langue (Android) : section `[mobile]` de `crates/domain/i18n/{en,fr}.toml` ; `NativeEvent::Locale { tags }` (langues du système, envoyées après `Native.init`), `MobileCommand::SetLanguage`, message `i18n` (`lang`, textes `mobile.*`) envoyé à `Ready` et à chaque changement ; `core::text` / `text_f` / `text_n` et `Core::t` / `tf` / `tn` ; page : `T`, `Tn`, `applyTexts` (`data-i18n`, `data-i18n-ph`, `data-i18n-aria`), les signatures de rendu sont vidées au changement de langue ; coque : `toastKey`. Sans catalogue (aperçu dans un navigateur) le balisage garde son anglais. Test : `every_text_asked_for_is_in_the_catalog`.
- Renderer mort : `TabView.onRenderProcessGone` → `NativeEvent::Crashed` → `Core::crashed` (vue détruite ; onglet affiché rouvert une fois, accueil et toast s'il retombe en moins de `CRASH_LOOP` = 30 s ; une popup se ferme). La vue d'interface partage ce renderer : son `WebViewClient` renvoie `true` et `rebuildShell` en recrée une. Test réel : `adb shell am start … --ez bh_crash true` (debug, charge `chrome://crash`).
- Statuts sans page : `onReceivedHttpError` du cadre principal (401, 403, 5xx) garde `(adresse, statut)` jusqu'au `onPageFinished` de cette adresse (le WebView rappelle `onPageStarted` pour sa propre page d'erreur), puis sonde la page (`PROBE`, celle du bureau) → `NativeEvent::HttpStatus { probe }` → `is_engine_error_page` ou `is_bare_error_body` → `Core::error`.
- Authentification HTTP : `onReceivedHttpAuthRequest` → `NativeEvent::Auth { id, tab, host, realm }` → `auth.rs` (`AuthPrompts` du domaine, correspondance id du domaine ↔ id de la coque) → message `auth` pour l'onglet affiché (à l'arrivée du défi ou quand l'onglet est montré) → `MobileCommand::AuthAnswer { password: Secret }` / `AuthCancel` → `Effect::HttpAuth`. `auth_gone` ferme la question d'un onglet fermé. Interface : `ask()` (champs vidés dès la lecture).
- Popups : `setSupportMultipleWindows(true)`, `onCreateWindow` (geste requis) → `NativeEvent::NewWindow { tab, sized }` → `Effect::Popup` (la coque crée la vue et la remet au `WebViewTransport` un tour plus tard) ; `popup_first_address` juge la première adresse (`Navigating` ou `Started`) avec `domain::popup::new_window` : `Popup` reste, `Tab` → popup détruite et `Effect::Load` dans l'ouvrant, `Deny` → détruite. `onCloseWindow` → `NativeEvent::ClosedWindow`. `show()` et `home()` détruisent les popups qui ne sont pas à l'écran. État : `popup: true`.
- Menus et dossiers : `organize.rs` (`RenameTab`, `MoveTabToSpace` : la vue part, nouveau profil ; `MoveTabToFolder`, `CloseOtherTabs`, `RenameSpace`, `DeleteSpace`, `UndoRemoval`, `AddFolder`, `RenameFolder`, `ToggleFolder`, `DeleteFolder`, `Tidy`, `Untidy`, `DismissTidy`). État : `folders`, `tabs[].folder` / `renamed`, `tidy`. Interface : `hold()` (appui long), `tabMenu`, `folderPick`, `folderMenu`, `spaceMenu`, `drawerRows`, entrée de menu `confirm: true` (second toucher), toast à bouton Undo (`action: "undo_removal" | "untidy"`). Un menu s'ouvre par-dessus le tiroir ou la grille sans les fermer.
- Mises à jour Android (`crates/android/src/update.rs`) : mêmes sources que le bureau (redirection `releases/latest`, `SHA256SUMS.txt`, jamais l'API REST). Le cœur décide (adresses, tag plus récent, APK `BlackHole-android.apk` ou `-debug.apk` selon la clé de l'app via `updater::wants_asset("android"|"android-debug")`, comparaison SHA-256) ; la coque fait le réseau (`HttpURLConnection`, github.com en https seulement) et l'installation (`PackageInstaller`, confirmation système, `REQUEST_INSTALL_PACKAGES`). Vérification quotidienne sur `NativeEvent::Resume` (`MobilePrefs::auto_update`, `update_checked`), manuelle dans Réglages > Updates.
- Favicons (`icons.rs`) : `onReceivedIcon` → PNG 64 px en `data:` → `NativeEvent::Favicon`, validés (PNG base64, 24 Ko), gardés par site dans `icons.json` (300 au plus), jamais pour le privé, effacés avec l'historique ; l'interface les reçoit par `icons`/`icon`. Autorisations de site (`permissions.rs`) : caméra, micro, position, contenu protégé ; seule `MobileCommand::PermissionAnswer` accorde (fermer la question refuse), mémoire de session hors privé, onglet non affiché refusé. Autres : bureau/mobile par onglet (`DesktopSite`), taille du texte et tirer pour actualiser (`WebPrefs`), plein écran vidéo, envoi de fichiers, texte partagé (`NativeEvent::Shared`), menu d'image.
- Domaine : `Failure::Android(code)` (`WebViewClient.ERROR_*`) et `Failure::AndroidSsl(primary)` dans `error_page::classify`.
- Profils WebView (`androidx.webkit` `MULTI_PROFILE`) : `Default` (espace 1), `space-N`, `private` (vidé puis supprimé avec le dernier onglet privé). Sans cette fonction : profil unique, signalé dans les Réglages.
- `keyring` compile sur Android mais sans magasin persistant : ne pas y porter `Vault`/`Wallet` sans clé enveloppée par l'Android Keystore. `rustls-native-certs` ne trouve aucun certificat sur Android : pas de `ureq` côté Android sans `webpki-roots`.
- Build : `cd android; .\gradlew.bat assembleDebug` (tâche `cargoNdk` → `app/build/rustJniLibs`, arm64-v8a + x86_64 ; `-PrustRelease`). Outils : JDK 17, SDK 35, NDK 27.3.13750724, `cargo-ndk`, cibles `aarch64-linux-android`/`x86_64-linux-android`. Antivirus inspectant le HTTPS : `GRADLE_OPTS`/`SDKMANAGER_OPTS` = `-Djavax.net.ssl.trustStoreType=Windows-ROOT`. Aucune clé de release dans le dépôt : `release.yml` (job `android`) signe avec les secrets `ANDROID_KEYSTORE_BASE64`/`ANDROID_KEYSTORE_PASSWORD`/`ANDROID_KEY_ALIAS`/`ANDROID_KEY_PASSWORD` (variables `BH_ANDROID_*` lues par `app/build.gradle.kts`), sinon publie `BlackHole-android-debug.apk` avec un avertissement. `versionName`/`versionCode` dérivés de la version de `Cargo.toml`.
- Tests : `cargo test -p blackhole-android` (hôte), `cargo ndk -t arm64-v8a -P 29 clippy -p blackhole-android --all-targets`, `scripts\test-android.ps1 [-Boot]` (AVD `bh_test`, extras debug `bh_cmd64`/`bh_eval64` en base64, résultats `bheval:nom=` dans logcat, contrôles nommés, captures `%TEMP%h_android_shots`). Inspecter les captures, dont l'état intermédiaire du tiroir et le mode animations coupées.

## Synchronisation des onglets entre appareils (0.8)
- Domaine `sync.rs` : `Ledger { items, spaces }` (onglet = espace par nom insensible à la casse + adresse sans fragment ; `modified` en ms, `device`, `deleted`), `observe(live, device, now)` (titre seul = pas un changement, pierres tombales purgées après `TOMBSTONE_DAYS`, `MAX_ITEMS`), `merge(remote) -> Merge { ledger, plan, remote_behind }` (plus récent gagne, l'id d'appareil départage ; espaces d'abord). `Workspace::sync_view` (hors dossiers) et `apply_sync(plan) -> Vec<TabId>` (supprimés : leurs vues partent). `BrowserService::apply_sync`.
- Infra `sync.rs` : `SyncEngine::open/observe/snapshot(reply, pull)/parse/preview/accept`, `decide(mode, received, merge)` → `Nothing | SendBack | Ask | Apply` (`reply` : réponse à une fusion acceptée ou à « Sync now », appliquée sans question ; `pull` : « Sync now », réponse obligatoire). Données reçues validées (http(s), couleurs `#rrggbb`, textes bornés).
- `handoff.rs` : `Device.sync` (`ask`/`auto`/`off`, `#[serde(default)]`), `Request::SetSync`, `Request::Snapshot` → `Message::Part { snap, id, part, parts, data }` (6 Ko, 100 au plus, `Ack` par fragment, sorties `quiet`), `Event::Snapshot { device, name, json }`. `DeviceStore::with_vault_key`, `Config::lan_with` (Android), `Vault::named_with_key` public.
- Desktop (`app/src/features/handoff.rs`) : un appareil qui passe en ligne reçoit le registre ; observation toutes les 2 s, envoi aux appareils `auto` 3 s après un changement ; `Ask` → toast à trois boutons (`sync_answer`). Réglages > Devices : Ask/Always/Off et « Sync now » (`device_sync`, `device_sync_now`). Build `devtools` : ports `BH_HANDOFF_PORT`/`BH_HANDOFF_PEER_PORT`.
- Android (`crates/android/src/devices.rs`) : mêmes règles ; clé de `devices.bin` enveloppée par l'Android Keystore (`KeyVault.kt`), `MulticastLock`, `NativeEvent::Tick`. Interface : Réglages > Devices, « Continue on … » dans la feuille, proposition `sync_offer`.
- Tests : domaine, infra (fragments sur la boucle locale), cœur Android (deux cœurs appairés), `scripts\test-android.ps1` (section Devices, code, proposition), `scripts\test-sync.ps1` (Windows ↔ émulateur réels).

## Langues de l'interface (i18n)
- Catalogues `crates/domain/i18n/{en,fr}.toml` (TOML plat : `[section]` + `clé = "texte"`, `include_str!`) ; `domain/i18n.rs` : `LANGUAGES` (code, nom dans sa langue), `resolve(choix, locales_système)` (choix, puis première locale système connue, puis `en`), `catalog(lang)` (analysé une fois), `get` (repli anglais puis clé), `format` (`{nom}`), `plural` (`.one`/`.other`, `plural_form` : fr 0–1 singulier), `merged`, `with_prefix("error.")`, `message(clé, params)` + `Catalog::render` (texte d'une couche sans langue : statuts `handoff`, erreurs `calendar`, `wallet`, `import`, `easel` → rendus par l'app au moment de l'affichage, donc dans la langue courante). Tests : parité exacte des clés et des paramètres, paires de pluriels, textes non vides.
- `Settings::language` (`#[serde(default)]` = `"system"`, valeur inconnue → `"system"`), `BrowserService::set_language`, `UiCommand::SetLanguage { language }`. Langue système : `sys-locale` (macOS `CFLocale`, Windows `GetUserPreferredUILanguages`), repli `LANG`.
- App `i18n.rs` : langue courante (`apply`), `t` / `tf` / `tn` / `render`, `page_json()` (catalogue fusionné, mis en cache par langue, `<` échappé), `error_strings()`. `toast` et `notify` rendent les messages. `Running::language_changed` : `views::send_language` à chrome, flyout, hotzone, bar, home, palette, peek, splitbar, Bibliothèque, Easel (panneau et fenêtre), Réglages ; titres des fenêtres ; menu natif macOS reconstruit (`install_menu`) ; `ui_state`. Test `every_used_key_is_in_the_catalog` : toute clé littérale (`t("…")`, `bhT('…')`, `data-i18n*`, `T('…')`, `message("…")`) existe.
- Pages : `ui/i18n.js` (injecté par `ui_view`/`panel_view`, `__BH_I18N__`) : `bhT(clé, params)`, `bhTn(clé, n, params)`, `bhI18n.set/apply/lang`, attributs `data-i18n`, `data-i18n-title`, `data-i18n-placeholder`, `data-i18n-aria` (textContent / setAttribute seulement). Le texte anglais reste dans le balisage pour la lisibilité. Les pages gardent leur dernier état et le redessinent sur `bhlang` (barre latérale : signatures des cartes remises à zéro). Bibliothèque (`ui/library.html`) : construite en DOM (`el`, `row`, `textContent`), sur les tokens du design system comme les Réglages (rail, en-tête à badge, cartes, filtre segmenté, états vides, compteurs du rail), `body.noanim` suit `animations` de son état. Dates : `toLocaleDateString(bhI18n.lang)`.
- Page d'erreur : `views::error_page` ajoute `i18n` (clés `error.*`) à la config ; Android passe la langue de son interface (`core::error_page_in`). Scripts des pages web : `views::page_script(js, clés)` remplace `__BH_TEXT__` (find, autofill, sélecteur de zone) ; un changement de langue vaut pour les onglets créés ensuite.
- Ajouter une chaîne / une langue : voir `AGENTS.md`. Garder les noms propres (Easel, Boost, Air Traffic, Shields, Meet). Le changelog reste en anglais. Typographie française : espaces avant « : ; ? ! » et guillemets « », apostrophe ’.
- Tests UI : `set_language` en tête de chaque script (`test-errors.sh` en français, les autres en anglais, `test-ui.ps1`/`test-easel.ps1` en anglais) ; `scripts/test-i18n.sh` (macOS) bascule anglais → français → anglais et lit les textes (Réglages, barre latérale, barre du haut, Easel, Bibliothèque, page d'erreur). Écran verrouillé : les captures CoreGraphics sont noires, les `eval` restent valables.

## 0.8.14 : onglet ancré, traqueurs appris, gestionnaire de mots de passe optionnel

- Onglet ancré : `Tab::anchored` (`workspace.json` : `anchored`), `Workspace::toggle_anchor` /
  `anchored_tabs` (refusé pour un trou noir), commande `tab_anchor`, entrée du menu d'onglet, ancre
  `.anch` sur la ligne. Jamais mis en veille (`discard_idle_tabs` le traite comme occupé, plafond
  `max_live_tabs` compris) ; réveillé en arrière-plan une fois, `ANCHORS_DELAY` après le lancement
  (`wake_anchored`). Mis en veille à la main, il reste endormi jusqu'à ce qu'on l'affiche.
- Gestionnaire de mots de passe : `Settings::password_manager` (vrai par défaut), `PasswordAction::Enabled`,
  Réglages > Passwords (`#pwmanager`). Coupé : `autofill_query`, `login_submitted` et les identifiants
  proposés à l'authentification HTTP ne font rien ; les mots de passe enregistrés restent.
- Traqueurs appris (façon Privacy Badger) : règles pures `domain/trackers.rs` (`looks_like_tracking`,
  `base_domain`, `Trackers::observe` : appris après `SITES_TO_LEARN` sites distincts ; jamais
  `ESSENTIAL`, un site visité soi-même, un hôte déjà listé ou autorisé ; tout borné), fichier
  `trackers.json` (`TrackerStore`, `write_later`). `ui/tracker_watch.js` (cadre principal) ne fait que
  lire les `PerformanceResourceTiming` (aucune fonction de la page remplacée) et envoie
  `PageEvent::Requests` ; `app/src/features/trackers.rs` relit l'adresse sur le moteur. `PrivacyPolicy::set_learned`
  (partagé par les clones) : appris bloqués avec l'interrupteur des traqueurs, hôtes autorisés jamais
  bloqués, listes comprises. `active_lists` nomme une liste raccourcie ou apprise d'après son contenu
  (recompilée macOS) ; vues de réserve recréées à chaque changement ; les vues déjà ouvertes gardent
  leurs listes natives (WebView2 décide en direct pour les hôtes déjà filtrés). Jamais en fenêtre privée
  (lecture seule du profil principal) ni en trou noir. Réglages > Privacy (`#learning`, `#learnedlist`,
  `#allowedlist`), rapport du bouclier : un hôte cliqué est autorisé (`tracker_allow`).
- Compteur du bouclier par espace : `BlockLog::record_in` / `total_in` ; la barre latérale montre
  l'espace affiché, le rapport et les Réglages gardent le total de la session.
- Copier depuis le menu d'une sélection : `selection::copy_text` (lignes gardées, `MAX_COPY` 200 000
  caractères), la page envoie autant ; la recherche garde `clean_selection` (500, une ligne).
- Recherche dans la page : index du texte gardé tant que la barre est ouverte (invalidé par un
  `MutationObserver`), tableaux typés, visibilité et bloc demandés une fois par parent, recherche
  incrémentale quand le mot s'allonge.
- `depth.js` : une carte garde le pointeur tant qu'il est dans son rectangle d'avant l'inclinaison, et
  reste à plat près de ses bords (sinon elle oscillait sous un pointeur immobile). Les tuiles d'accès
  rapide ne penchent plus : elles montent d'un pixel au survol (relief inchangé).
- Easel : `live_embedded` ; `sync` → `drop_stray_live` libère les vues en direct de la fenêtre
  principale dès que l'Easel n'y est plus affiché (elles restaient sur la page).
- Accueil : ciel étoilé du site (`#sky`, canvas dessiné une fois, parallaxe au défilement et au
  pointeur, immobile sans animations). Barre du haut : `body.zoomed` réserve la place du badge de zoom
  (`--zw` mesuré). Carte de page : élévation renforcée en style 3D (`#cardshadow`).
- Test macOS : `bash scripts/test-0814.sh` (serveur local 47981, page de 30 000 lignes, étape `state` :
  `anchoredAlive`, `live`, `learned`, `allowed`). Windows : code commun, non testé
  (`test-features.ps1` `dp*` peut attendre l'inclinaison des tuiles d'accès rapide).

## 0.8.15 : extensions Windows, page blanche, bouclier Android

- Popups d'extension (Windows) : WebView2 fait de chaque vue un onglet dans sa propre fenêtre, la
  popup comprise ; `tabs.query({active, currentWindow|lastFocusedWindow})` y répondait la popup
  elle-même (`chrome-extension://…`), d'où « pas un site Web » dans uBlock Origin Lite et Ghostery.
  `ui/ext_tabs.js` (injecté par `views::extension_popup`, adresse de la page devant passée par
  `show_ext_popup`, rien sur l'accueil) répond par l'onglet de cette page (même adresse, sinon la page
  web vue en dernier, `lastFocusedWindow`/`lastAccessed`). Harnais : cible `eval extpopup` ;
  test `scripts	est-ext.ps1` (popup uBOL sur example.com).
- Extension d'exemple `cookie-banner-hider` (`app/src/features/extensions.rs`) : sélecteurs en
  `:is(…):not(html, body)` (glosbe.com met `cookie-consent` sur `<body>` : page blanche) ;
  `refresh_sample` remplace au lancement une copie installée laissée telle quelle (`OLD_SAMPLE_CSS`).
- Android : le compteur du bouclier est une capsule à côté de l'icône, dans la hauteur du champ
  d'adresse (la pastille en coin était coupée par `#omni`, `overflow: hidden`) ; jeton `--on-ok` pour
  son texte en thème clair. Contrôle `shieldfit` de `test-android.ps1`.

## 0.8.16 : Linux (aperçu), sélecteur de couleur, inspecteur ancré, onglets d'extension

- Barre latérale sans scintillement (`index.html`) : `tabNode` garde la ligne d'un onglet d'un rendu à
  l'autre (`rowCache`, clé : l'onglet, s'il est actif, son icône, son média) ; une ligne qui change garde
  son `.dot` (icône) si l'hôte et l'icône sont les mêmes. Cache vidé sur un rendu forcé (`s === prev`,
  `listsStale` : langue, glisser). Les cartes d'espace ne sont recréées que si les espaces changent
  (`spacesSig`). L'icône n'a plus `decoding = 'async'` (une image neuve restait vide une image).
- Rechercher dans la page (`find.js`) : les deux `Highlight` (`bh-find`, `bh-find-cur`) restent inscrits
  dans `CSS.highlights` et sont vidés puis remplis sur place (`paint`). WebKit ne repeint pas les plages
  d'un surlignage retiré du registre ou remplacé : les anciens résultats restaient peints.
- Linux (aperçu, x86_64, X11 ou XWayland) : wry sur WebKitGTK 4.1. `main` force `GDK_BACKEND=x11`, appelle
  `gtk::init()` et crée la boucle winit en X11 (`with_x11`) ; `about_to_wait` vide la file GTK à chaque tour
  (`gtk::main_iteration_do`) et revient au plus tard après `GTK_PUMP` (8 ms). Trousseau : keyring
  `sync-secret-service` (sans lui, magasin en mémoire et nouvelle clé du coffre à chaque lancement).
  Pas d'extensions (stub `ExtensionHost`), pas de capture dans le harnais. Mise à jour par l'app (x86_64)
  selon l'installation (`updater::linux_install`) : paquet Debian (`/usr/bin/blackhole`, clé `linux-deb`
  → `BlackHole-linux-amd64.deb`, installé après la fermeture par `pkexec apt-get install`, à défaut
  `pkexec dpkg -i`, puis relancé ; refusé, l'ancienne version se relance ; il faut `pkexec`) ou archive
  décompressée dans un dossier inscriptible (`blackhole` à côté de `blackhole.desktop`, clé `linux` →
  `BlackHole-linux-x86_64.tar.gz`, fichiers copiés à côté puis renommés par-dessus, binaire en dernier,
  puis relance). Un binaire copié ailleurs ne se met pas à jour (`supported` faux, lien vers la version).
  Paquets : `scripts/package-linux.sh VERSION [BINAIRE]` → `dist/BlackHole-linux-amd64.deb` (dépend de
  `libwebkit2gtk-4.1-0`, `libgtk-3-0`, `libxkbcommon-x11-0`, `libdbus-1-3`) et `BlackHole-linux-x86_64.tar.gz`.
  CI : test + Clippy `-D warnings` sur `ubuntu-22.04`, job `linux` (paquets joints à la release).
  Vérification locale depuis macOS : conteneur Docker (Debian, `libwebkit2gtk-4.1-dev`…), Xvfb.
  **Limite connue (vue sous Xvfb) :** les vues créées à leur taille finale (barre du haut) se peignent,
  mais une vue déplacée ou redimensionnée après sa création (accueil, onglets, barre latérale qui glisse)
  reste noire, avec des `Gdk-CRITICAL _gdk_frame_clock_freeze` (fenêtres X étrangères de wry). À vérifier
  sur un vrai bureau Linux avant de présenter l'aperçu comme utilisable.
- Inspecteur web ancré (macOS) : WebKit range l'inspecteur dans la vue parente de la page en supposant
  qu'elle la remplit ; avec la vue de contenu de la fenêtre comme parent, la page couvrait toute la
  fenêtre (barre du haut et barre latérale perdues, fenêtre noire à la fermeture). vendor/wry place
  chaque vue enfant dans un `WryWebViewHost` de sa taille (`host`, déplacé, masqué et retiré à sa place,
  `outer()`) ; `round_view` / `raise_view` de `macos-webext` agissent sur cet hôte (`placed_view`).
  Harnais : étape `inspect show|attach|detach|close` (`macos_webext::inspector`).
- Onglets demandés par une extension (macOS) : le délégué du contrôleur implémente
  `openNewTabUsingConfiguration` / `openNewWindowUsingConfiguration` (sans eux WebKit refusait :
  « Ouvrir dans un nouvel onglet » de Bitwarden, clé de sécurité) ; adresses relevées par
  `ExtensionHost::take_opened` dans `poll_webext`, ouvertes en onglets (jamais en popup), popover fermé.
  Windows : `take_opened` vide (WebView2 passe par le gestionnaire de nouvelle fenêtre des vues).
- Couleur d'un espace : élément de menu `picker` (`palette.html`, `colorPicker`) : 8 pastilles
  (appliquent et ferment), pavé teinte × saturation (flèches au clavier, Maj = grand pas), piste de
  luminosité (15–85 %), valeur hexadécimale. La couleur part pendant le choix (60 ms au plus,
  `set_space_color`), le menu reste ouvert. Textes `color.hex`, `color.hue_saturation`, `color.lightness`.
- Style 3D : plus aucune inclinaison vers le pointeur ; `depth.js` garde son API (`bhDepth`, `bh-lean`)
  mais pose `--rx`/`--ry` à 0 et `--mx`/`--my` à 50 % : la carte survolée ne fait que monter
  (`translateZ`, ombre plus longue). Reflets radiaux qui suivaient le pointeur retirés.
- Test macOS : `bash scripts/test-0816.sh` (serveur local 47982 : inspecteur ancré dans la carte, fermé,
  détaché ; sélecteur de couleur : pastilles, hex, pavé au clavier, menu ouvert, pastille qui ferme).
  Windows : code commun (sélecteur, 3D) non testé ; `test-features.ps1` `dp1` attend une tuile montée
  sans rotation (`--ry` = 0, capture `f13a-home-tile-raised`).

## 0.8.17 : flyout et menus, DMG glisser-déposer, tuiles, accueil, profils d'import, passkeys

- Barre latérale masquée : un menu ouvert depuis le flyout (`UiCommand::Menu`, `origin` « sidebar »,
  surtout le sélecteur de couleur d'un espace) garde le flyout (`Sidebar::hold_for_menu`) : la palette le
  couvre, donc son `mouseleave` (`peek false`) est ignoré jusqu'à `close_palette`. Ensuite, le prochain
  mouvement sur la page le replie (la bande `hotzone` reste large, contrairement à `reveal`).
- DMG macOS : `scripts/make-dmg.sh [APP] [SORTIE]` (appelé par `release.yml` et `package-macos.sh --dmg`) :
  l'app à côté d'un lien `/Applications`, fond fléché clair (`scripts/make_dmg_background.py`, stdlib, 1x +
  Retina via `tiffutil`), mise en page Finder par AppleScript (une minute au plus ; sinon avertissement,
  image construite quand même avec le lien). Icône du volume si `SetFile` est là.
- Tuiles d'accès rapide : `Settings::quick_access_size` (`QUICK_ACCESS_SIZES` : small 5 par ligne, medium 4
  et 48 px, large 3), commande `set_quick_access_size`, classes `qa-small` / `qa-large` de `index.html`,
  Réglages > General seulement (plus dans les menus depuis 0.8.25).
- Accueil : sections `HOME_SECTIONS` (`now`, `favorites`, `watches`, `downloads`, `recent`) masquables
  (`Settings::hidden_home_sections`, `set_home_section`, bouton « Customize the start page », Réglages >
  General). Zones surveillées (`home_watches_json`, dernière valeur, les plus récemment changées) et
  téléchargements présents sur le disque (`home_downloads_json`), `HOME_LIST` au plus ; une section vide
  reste cachée.
- Import : noms de profils donnés par l'utilisateur, `Local State` (`profile.info_cache.<dossier>.name`)
  pour Chromium, `profiles.ini` (`Name=`/`Path=`) pour Firefox ; repli sur le nom du dossier.
- Passkeys (macOS) : sans l'entitlement `com.apple.developer.web-browser.public-key-credential` (signature
  Developer ID et accord d'Apple), WebKit refuse WebAuthn d'emblée (`NotAllowedError` en ~10 ms, rien à
  l'écran), répond `false` à `isUserVerifyingPlatformAuthenticatorAvailable` après plusieurs secondes et
  annonce à tort un authentificateur dans `getClientCapabilities`. `ui/passkeys.js` (toutes les frames,
  macOS seulement) répond tout de suite « pas d'authentificateur de plateforme » et enveloppe
  `CredentialsContainer.prototype.get/create` (pas l'instance : WebKit remplace `navigator.credentials`
  après le début du document) ; un refus rapide envoie `PageEvent::PasskeyRefused`, notice une fois par
  site (`domain::passkeys::Refusals`, 10 min). Les extensions (Bitwarden : son script de page attend cette
  réponse pour se substituer) passent avant et se replient sur ces fonctions. 1Password et Dashlane : leurs
  extensions Safari fournies par leurs apps (Réglages > Extensions). Windows : rien d'injecté, WebAuthn de WebView2 non vérifié.
- Test macOS : `bash scripts/test-0817.sh` (serveur local 47983 : tuiles, sections de l'accueil, flyout
  gardé derrière le sélecteur, passkeys et notice ; pointeur hors de la fenêtre). Windows : code commun
  (tuiles, accueil, import) non testé.

## 0.8.18 : recherche, rechargement forcé, échecs fantômes, accueil réorganisable, jump list, Réglages en onglet

- Recherche de la barre de commandes : règles pures `domain/suggest.rs`, classement fait par le shell à
  chaque frappe (`UiCommand::Suggest { query, seq }` → `bh.suggested`, `app/src/features/suggesting.rs`) : la
  palette ne classe plus rien, elle dessine (avant la première lettre : onglets, accès rapide, commandes).
  Candidats construits à l'ouverture (`suggest_prepare`) : onglets de tous les espaces (`OtherTab`, nom de
  l'espace, `Activate` change d'espace), accès rapide, favoris, `pool` de l'historique, commandes.
  - `key` (même page quelle que soit l'écriture), `is_noise` (connexion, OAuth/SSO, `callback`, `logout`,
    redirections, `?code=&state=`, segments français `connexion`…), `pool` (une ligne par page, page
    d'accueil d'un site vu par `ROOT_FROM_PAGES` pages ; page de résultats d'un moteur → recherche
    précédente, `search_terms`, `RowKind::Search`, deux au plus).
  - Visites : `HistoryEntry::recent` (10 dernières, date + `VisitKind` : `Typed` pour `open`/`navigate`
    de l'interface, `Bookmark` si `via: "bookmark"`, `Link` sinon, `Redirect` pour une page quittée en
    `BOUNCE_SECS` sans demande : elle passe son type à la suivante) et `spaces` ; `history.json` :
    `visits`, `recent` `[(s, kind)]`, `spaces` (absents = anciens fichiers). `BrowserService::expect_visit`,
    `Running::next_visit`. `frecency` = moyenne (âge × poids du type) × (1 + 1,5 ln visites).
  - `match_quality` : `fold` (casse, accents, œ/æ/ß), chaque mot doit correspondre, une faute permise
    à partir de 4 lettres (`typo_of`, distance OSA ≤ 1 en début de mot, score plus bas).
  - `Adaptive` (choix appris façon Firefox, `suggest.json`, `ChoiceStore`, 500 au plus, demi-vie 30 j,
    jamais en fenêtre privée ni en trou noir) : `UiCommand::SuggestChosen { input, key }` à chaque ligne
    prise ; oublié avec l'historique (`suggest_forget`).
  - `rank_rows` (score = qualité × 100 + habitude × 45 + choix × 70 + type + 10 pour l'espace affiché ;
    une commande nommée par la saisie d'abord ; une page trouvée par plusieurs voies est offerte sous sa
    forme la plus utile, onglet d'abord), `pick` (8 lignes, 3 commandes gardées, 2 recherches),
    `completion` (un mot de 2 lettres ou plus qui commence l'adresse d'un site habituel : texte gris dans
    le champ, `#ghost`, Tab ou → le prend, la première ligne l'ouvre ; jamais après un effacement).
  - Jeu d'évaluation : `domain/src/suggest_eval.rs` (historique réaliste, requête → résultat attendu) ;
    tout réglage de poids s'y mesure, un cas s'y ajoute quand une requête répond mal.
  - Titres : une nouvelle page du même site reprend le titre de l'onglet tant qu'elle n'a pas le sien
    (`History::name_if_bare`), le moteur ne signalant pas un titre inchangé. Android : `suggestions`
    passe par `rank_rows`/`pick` (sans choix appris). « Recent activity » de l'accueil : sans bruit ni
    doublon (`home_recent_json`).
- Synchro de l'historique (0.8.18), à part de celle des onglets : un interrupteur (`Settings::history_sync`,
  vrai par défaut, `SetHistorySync`, Réglages > Devices carte `#histcard`, Android `data-set="history_sync"`),
  aucune question, en arrière-plan. `infrastructure/history_sync.rs` : messages `"v": 2` sur le transport des
  registres (`Request::Snapshot`, ignorés par un appareil plus ancien) : `history_pull { since }` (quelques
  octets) et `history { until, more, pages, forgotten, cleared_at, choices }`. Incrémental : chaque entrée
  porte `HistoryEntry::changed` (horloge de l'appareil : visite, fusion qui change quelque chose) ;
  `History::changes_since` rend ce qui a changé après le repère, plus ancien d'abord, `BATCH` (200) pages
  (jamais coupé au milieu d'une seconde) ; `Marks` (`history-sync.json`) garde le repère atteint par
  appareil. Rien de changé : réponse vide. Une page porte ce qui la classe (visites datées et typées,
  titre, espaces **par nom**), les pages oubliées et l'effacement voyagent (`History::forget` / `clear_at`,
  `history-erased.json` à part pour qu'une version plus ancienne relise `history.json`, oubli gardé 30 j),
  et les choix appris de la barre de commandes (`Adaptive::merge`, le plus récent gagne ; Android n'en a
  pas). `History::merge` idempotente et symétrique (index par adresse) ; écrite au prochain `flush`, pas
  sur le fil d'interface. Cadence : `HISTORY_FIRST` (20 s) après le lancement, puis `HISTORY_EVERY`
  (5 min), quelques secondes après qu'un appareil se connecte, et « Sync now » (`HistorySyncNow`). Jamais
  en fenêtre privée (pas de Nearby) ni pour un onglet trou noir (absent de l'historique). Tests :
  `history::tests`, `history_sync::tests`, test Android de bout en bout (`devices.rs`, deux cœurs appairés),
  contrôle `histsw` de `test-android.ps1` (non exécuté).
- Rechargement : Mod+R et F5 = `reload`, Mod+Maj+R / Ctrl+F5 / Maj+F5 / Maj+clic = `UiCommand::HardReload`
  (`views::hard_reload`), aussi dans le menu de page et la palette (`key:shift+r`). Tout rechargement de la
  page affichée appelle `spin_reload` → `bh.reloading()` de `bar.html` : seule l'icône (`#reload svg`)
  tourne, tour après tour tant que la page charge, arrêt sur une fin de tour (`animationiteration`).
- Échecs fantômes (« Site injoignable ») : tout `Msg::Failed` attend `ABORT_GRACE` (`aborted`, avec
  l'adresse du moteur au moment de l'échec) ; un `LoadStart`, un `Loaded` ou un téléchargement l'annule.
  `errors::settle_failure` : ignoré si l'adresse du moteur a changé entre-temps (redirection par script,
  nouvelle adresse) ; échec passager (`error_page::is_transient` : WebView2 0, 6–10, 12, 16 ; NSURL -1001,
  -1004, -1005, -1011, -1017) ou sans raison → `retry_quietly` (une fois par adresse et
  `RETRY_WINDOW_SECS`, `Retries` ; `reload` si le moteur est sur l'entrée en échec, sinon `load_url`) avant
  la page d'erreur. Boucle de redirection : `Msg::Hop` (chaque navigation du cadre principal, sauts
  compris) compte les sauts depuis la dernière page ; à `LOOP_HOPS` (6) ou plus, jamais de second essai
  et « Too many redirects » (WebView2 ne signale qu'un « connection aborted », souvent sans 3xx).
  `CONNECT_MAX` sans réponse : un second essai aussi. macOS : non testé.
- Accueil : `Settings::home_order` (`home_order()` garde chaque section une fois, une section absente
  reprend sa place par défaut ; ordre par défaut = vide), `UiCommand::SetHomeOrder`. `home.html` :
  sections dans `#sections`, déplacées par leur titre (pointeur, FLIP en `transform`, `body.dragging`)
  ou Alt+↑/↓ au clavier ; nœuds déplacés, jamais recréés (`placeSections`). Icône `grip`.
- Jump list Windows : `macos-webext/src/jumplist.rs` (`ICustomDestinationList`, tâches `IShellLinkW` vers
  l'exécutable, `--private` pour la fenêtre privée), fil `bh-jumplist` ; `window_tasks()` au lancement et
  au changement de langue. Rien sur les autres systèmes (menu du Dock macOS non fait).
- Réglages dans la carte de page (plus de fenêtre) : `settings_panel` (`LazyView`, `SETTINGS_HTML`,
  `views::ui_view`), `settings_shown` (dans `home_shown`, `active_view`, `ensure_panels`, `place_panels`,
  `sync`) ; tout `self.library = false` remet aussi `settings_shown = false`. Ouverture : `open_settings`
  (`settings_wanted`), fermeture : `close_settings` (Échap, `close_settings`, `panel_window` close, Mod+W).
  État `embedded` : `body.embedded` masque `#top`, fond et ciel de l'accueil. Barre du haut : `place`
  « settings ». Pastille `#applied` (« Changes applied ») 450 ms après la dernière commande qui change un
  réglage, 2 s à l'écran. Devtools : `shots` capture la fenêtre principale, `eval settings` le panneau.
- Test Windows : `scripts\test-0818.ps1` (serveur local 47984, contrôles nommés `sr*` dont accents, faute, complétion, choix appris, onglet d'un autre espace, `rl*`, `se*`, `ho*`,
  `rt*`, jump list lue dans `CustomDestinations`). macOS : compilation croisée seulement.

## 0.8.19 : market d'extensions signé

- Source unique : `market/market.json` (racine du dépôt). Chaque entrée : `id` (nom de fichier,
  `domain::market::valid_id`), `name`, `description` `{en, fr}` (plus de clés `extdesc.*`),
  `homepage`, `category`, `version`, `platforms` (`macos` / `windows` → `url` de release GitHub,
  `sha256`, `size`), `permissions` (manifest : `permissions` + `host_permissions`), `source` (dépôt,
  `tag_prefix`, motif d'asset par plateforme ; lu par l'outil, ignoré par l'app). L'app l'embarque
  (`market::builtin`, `include_str!`) comme plancher hors ligne ; `CATALOG` / `CatalogEntry` n'existent plus.
- Règles pures `domain/market.rs` : `platform_key` (pas d'hôte d'extensions = pas d'entrée : Linux,
  Android), `trusted_package_url` (seulement `https://github.com/<o>/<r>/releases/download/<tag>/<fichier>`),
  `valid_sha256`, `compare_versions` / `is_upgrade` (jamais de retour en arrière : une liste plus ancienne
  ne propose rien), `accept` (schéma, `seq` jamais en recul, `expires`), `asset_matches`, `added_permissions`.
- `infrastructure/market.rs` : `Index` / `Entry` / `Package` (serde), `validate`, signature Ed25519
  (`ed25519-dalek` épinglé en 2.1.1 pour la MSRV 1.80, `verify_strict`) sur les octets exacts de
  `market.json`, clés publiques `PUBLIC_KEYS` (deux possibles pendant une rotation). `fetch` lit
  `github.com/<repo>/releases/download/market/market.json` + `.sig` (pas d'API REST, pas de quota),
  `accept_download` refuse signature fausse, `seq` 0, index plus ancien ou expiré, puis écrit
  `<données>/market/` (JSON puis signature, `.part` + renommage). `current` = cache valide sinon `builtin`.
  `catalog.rs` ne garde que l'installation : `install(data, &Entry)` (paquet de l'OS courant, URL de release
  GitHub, 64 Mo, SHA-256), `store(data, id, version, sha, bytes)`, `needs_update` (via `is_upgrade`).
- App : `Running::market` (lu au lancement depuis le profil des extensions), `refresh_market(asked)` sur un
  fil, `poll_market` (dans `poll_webext`) : `FIRST_MARKET_CHECK` 20 s puis `MARKET_CHECK_EVERY` 24 h,
  `MARKET_RETRY` 1 h après un échec ; automatique seulement si `updates_enabled()` (pas en build `cargo`,
  `devtools` ni privé). « Check for updates » (`CheckCatalogUpdates`) télécharge d'abord la liste et compte
  les mises à jour à l'arrivée (`market.fetching = Some(true)`). État Réglages : `market` (`signed`,
  `signedAt` = `seq`, `checking`, `error`) ; `settings.html` : `#catsrc` sous la note du catalogue, bouton
  qui tourne pendant la recherche, état vide `settings.market_none` quand l'OS n'a aucune entrée.
- Outil `crates/market-tool` (binaire `bh-market`, jamais embarqué) : `keygen`, `check`, `review`, `audit [--write]`
  (télécharge chaque paquet épinglé, vérifie le hash, remplit `size` et `permissions`), `update --summary`
  (releases amont par l'API avec `GITHUB_TOKEN`, première release publiée au bon `tag_prefix`, digest
  `sha256:` de GitHub recoupé, manifest lu, refus d'une version non plus récente), `sign` (`seq` = maintenant,
  `expires` = +30 j, clé `MARKET_SIGNING_KEY` en hex, refusée si absente de `PUBLIC_KEYS`), `verify`.
- Workflow `.github/workflows/market.yml` : `track` (quotidien ou manuel) ouvre une PR `market/update` avec
  versions, URLs, hashes et raisons de revue (⚠️) : rien n'atteint les utilisateurs avant la fusion ; `merge`
  (après `track`) la fusionne seul si elle ne touche que `market/market.json`, que son dernier commit a 48 h
  et que `bh-market review main.json pr.json` ne trouve rien (`review_reasons` : mêmes extensions, mêmes
  noms, sources et plateformes, version plus récente, paquets du dépôt suivi, aucune permission ajoutée) ;
  sinon elle attend un humain (fermer la PR arrête tout). Une fusion par le jeton du workflow ne relance
  aucun workflow : `publish` vient après `merge` dans le même run et extrait `main` à jour ;
  `publish` (push de `market/market.json` sur `main`, quotidien, manuel sur `main`) signe et téléverse dans
  la pré-release `market` (jamais « latest » : l'updater de l'app l'ignore). Secret `MARKET_SIGNING_KEY`
  absent : avertissement, rien de publié, l'app garde sa liste embarquée. Autoriser Actions à créer des PR
  (Settings > Actions > General).
- Tests : `domain::market::tests`, `infrastructure::market::tests` (signature, rejeu, expiration, entrées
  dangereuses, liste embarquée valide pour macOS et Windows), `catalog::tests`, `bh-market` (manifest,
  choix de release et d'asset, signature d'une liste tamponnée). Réseau : `bh-market audit`.

## 0.8.20 : écran Market, accord aux nouvelles permissions, Réglages plus larges (ajout depuis GitHub : 0.8.21)

- Permissions lisibles, `domain/market.rs` : `permission_lines` (une ligne par sorte, `PermissionLine { key,
  sensitive, items }`, sensibles d'abord ; `<all_urls>` et tout motif d'hôte `*` → `all_sites`, qui couvre
  les sites précis `some_sites` (hôtes dans `items`), noms inconnus → `other`), `added_permission_lines(old,
  new)` (ce qu'une mise à jour ajoute : même sens écrit autrement = rien). Textes `settings.perm_<key>`
  (test `every_kind_of_permission_has_its_text`).
- Paquet installé : `market::manifest_info` (zip, manifest à la racine ou le moins profond ; aussi utilisé
  par `bh-market`), `catalog::installed_permissions` (`webext/<id>.permissions.json` écrit par `store`, lu
  dans le zip une fois pour une installation plus ancienne, effacé par `remove`).
- App : chaque entrée de `catalog_state` porte `repo`, `file`, `size`, `sha256`, `permissions` et `updateAdds`
  (`Running::update_adds` : lignes ajoutées par la mise à jour ; permissions installées inconnues = toutes) ;
  `market.platform` (`macos` / `windows`). `UpdateAllCatalog` laisse de côté les mises à jour avec
  `updateAdds` (le nombre laissé va dans `catalogChecked`) : elles passent par « Accepter et mettre à jour »
  dans leur fiche (même `install_catalog`).
- `settings.html` : carte `#market` (onglets `.mktabs` Toutes / Installées / Mises à jour avec compteurs,
  `#mksearch` sans accents ni casse sur nom, description, dépôt et catégorie, puce de l'OS `#mkos`, catégories
  `#mkcats`, grille `#catalog` de cartes `.xc` cliquables et focusables). Fiche `#mkdetail` à la place de la
  grille (`mk.open`, `mkOpen`) : retour, tags (Vérifiée, version ou `installée → proposée`, Nouvelles
  permissions), panneau d'accord, actions (installer, mettre à jour, interrupteur, retirer, `open_link` vers
  GitHub), « Pour cet appareil » (système, fichier, taille, SHA-256 copiable), permissions. Échap ferme la fiche
  avant les Réglages. État dans `mk` (non persisté) ; rendu `put`/`morph` comme le reste de la page.
- Réglages : `section` jusqu'à 1100 px (680 avant), `.note` limitée à 110 caractères.
- Ajouter depuis GitHub (0.8.21, étape 3 du market, confiance au premier usage) : règles `domain::market` `github_repo`
  (`owner/repo` ou adresse ; propriétaire = lettres, chiffres, tirets), `sideload_id` (`gh-<owner>-<repo>`,
  jamais un id du market), `asset_rank` (Windows : Chromium puis générique, jamais Firefox ni Safari ; macOS :
  Chromium, générique, puis Firefox), `asset_pattern` (version du nom → `*`), `display_name`.
  `infrastructure/github_ext.rs` : `latest_release` (API REST `releases/latest` : un clic de la personne, pas
  une veille ; 403/429 → `msg.github_rate_limited`), `parse_release` (seuls les fichiers
  `github.com/<owner>/<repo>/releases/download/…`), `prepare` / `check_package` (téléchargement par
  `catalog::fetch`, SHA-256, `digest` de GitHub comparé quand il existe, manifest lu), `install` (`catalog::store`
  puis épinglage dans `extensions-github.json` : fichier, motif, URL, SHA-256, permissions), `update_of` (release
  plus récente avec un fichier du même motif), `remove`. App `github_ext.rs` : `Sideload` (`pinned`, `flow`
  Idle → Looking → Choose → Preparing → Ready → Installing → Done / Failed, numéro de flux contre les réponses
  périmées, `updates`), commandes `GithubLookup`, `GithubPrepare { asset | update }`, `GithubInstall`,
  `GithubCancel` (interdites aux pages) ; « Check for updates » cherche aussi une release plus récente de chaque
  extension épinglée ; « Update all » ne les prend jamais. Chargées au lancement comme celles du market.
  Écran : bouton `#mkgh`, panneau `#mkgithub` à la place de la grille (saisie, fichier recommandé, aperçu
  « Non vérifiée par BlackHole » avec empreinte, conformité au digest GitHub et permissions, « Installer quand
  même »), cartes et fiches marquées « Non vérifiée », catégorie `github`. Une extension du market trouvée par
  son dépôt renvoie vers sa fiche. Test : `scripts\test-github-ext.ps1` (réseau, API GitHub anonyme).
- Tests : `domain::market::tests`, `catalog::tests::remembers_the_permissions_of_the_installed_package`,
  `scripts\test-ext.ps1` (carte du market, fiche d'une extension installée : permissions et hash). macOS :
  compilation croisée seulement.

## 0.8.22 : déplacement de fenêtre (Windows), extensions en privé, nouvelle fenêtre plus rapide

- Déplacer une fenêtre (barre du haut, barre de l'Easel) : `drag_window` de l'app, jamais `Window::drag_window`
  de winit sous Windows. winit 0.30 pose un drapeau `dragging` que seul `WM_EXITSIZEMOVE` efface : un clic
  rapide sur la barre (relâché avant que le shell traite le message) le laissait levé et plus aucun glisser ne
  marchait. `blackhole_macos_webext::drag_window` (win.rs) lance le déplacement natif (`WM_NCLBUTTONDOWN`,
  `HTCAPTION`) seulement si le bouton principal est encore enfoncé. Côté page (`bar.html`, `easel.html`), sous
  Windows le glisser ne part qu'après 4 px de mouvement bouton enfoncé : la boucle de déplacement avale le
  clic suivant, donc un clic ou le double-clic qui agrandit n'y entre jamais. macOS inchangé (winit).
  Test manuel : clic rapide sur la barre puis glisser, double-clic agrandir/restaurer.
- Fenêtre privée : son profil WebView2 est neuf, chaque extension y est réinstallée et ouvre sa page
  d'accueil (Dark Reader, Ghostery, Bitwarden). `domain::welcome::WelcomePages` : pendant `WELCOME_WINDOW`
  (15 s) après le chargement d'une extension, une page demandée sans onglet d'origine (`Msg::OpenTab`,
  `OpenTabFrom` de l'onglet 0 hors Peek, `take_opened`) est écartée ; jamais en fenêtre normale. Un lien
  d'une page affichée a toujours son onglet. macOS : stockage des extensions déjà gardé (identifiant stable),
  la règle ne gêne rien.
- Nouvelle fenêtre (`is_new_window` : `--new-window` passé par `open_window`, fenêtre privée, ou autre
  fenêtre vivante dans le registre `peers`) : pas de splash (champ `intro` de l'état de l'accueil).
  Démarrage : hotzone, flyout et palette (`Option<WebView>`) sont créés juste après les premières vues par
  `raise_due` (palette forcée par `open_palette` via `flush_raise(true)`) ; mesures `BH_TRACE=1` en debug
  (`startup N ms: …`). Contenu affiché vers 0,8 s pour une nouvelle fenêtre, 1,2 s en privé.

## 0.8.23 : onboarding d'accueil

- Visite d'accueil (`domain::onboarding`) : proposée **une seule fois**, au premier lancement d'un profil
  (`BrowserService::first_launch`, aucun réglage encore écrit), jamais en fenêtre privée ni en nouvelle
  fenêtre (`offered`). Quitter pendant la visite ne la ramène pas. Réglages > Général > Système, « Revoir »
  (`UiCommand::ReplayTour`) rejoue **les fonctionnalités seules** (`Mode::Tour`) ; la première fois
  (`Mode::Welcome`) ajoute les choix. Maquettes : `docs/onboarding/maquettes.html` (variante A retenue).
- Page : `ui/onboarding.html` (style, fenêtre et script, classes `ob-`), concaténée à `home.html` par
  `HOME_HTML` (`concat!`) : `home.html` s'arrête sans `</body></html>`, c'est `onboarding.html` qui ferme le
  document. `bh.render` de l'accueil appelle `bhOnboard.update(s.onboarding, s)` ; la fenêtre s'ouvre après
  l'atterrissage du splash (`window.bhSplashEnd`), voile `#ob-veil` sur l'accueil, carte 740 × 580 réduite
  aux petites fenêtres (`--ob-k`). Étapes : bienvenue (langue), espaces, barre de commandes, Shields (« Même la
  lumière n'en ressort pas »), « Et aussi », import, navigateur par défaut (pas sous Linux), extensions (s'il y
  a un hôte d'extensions), style, fin. « Passer la présentation » saute aux choix ; Échap ferme tout.
  Entrée / ← / → au clavier, focus gardé dans la fenêtre ; un clic pendant une transition est ignoré.
- Transition : le trou noir seul, noir pur (`.ob-warp .void`, `ob-swallow`) : naît avec un rebond, grandit
  jusqu'à couvrir la carte (0,7 s), l'étape est échangée dans le noir et la suivante sort (0,25 s). La fin
  couvre toute la page d'accueil (`#ob-warp-all`) puis l'accueil ressort (`main.ob-emerge`). Animations coupées
  ou `prefers-reduced-motion` : simple échange, aucun mouvement.
- État : `onboarding` de l'état de l'accueil (`onboarding_json` : `null` fermé ; `{mode: "tour"}` ; en
  `welcome` : `os`, `sources` d'import, `imports` (nombre d'imports terminés pendant la visite) et `importMsg`,
  `defaultBrowser {can, is, asked}`, `extensions` (`onboarding::starters` du market pour ce système : Dark Reader
  et SponsorBlock cochées, Bitwarden et uBlock Origin Lite non, avec `installed`, `installing`, `error`),
  `space`, `color`, `colors` (`SPACE_COLORS`), `engine(s)`, `recall`, `language`). Le rendu ne reconstruit que
  ce qui a changé dans l'étape affichée (signature par étape), les animations continuent.
- Commandes : `onboarding.open {on}` (la page montre ou cache sa fenêtre), `onboarding_done`, `replay_tour`,
  `set_default_browser` ; la visite réutilise `import_bookmarks`, `import_file` (`logins`), `install_catalog`,
  `set_space_color`, `set_depth`, `set_engine`, `set_recall`, `set_language`. Une extension installée depuis la
  visite n'ouvre pas sa page d'accueil par-dessus (`WelcomePages::tour_open`, posé par `onboarding.open` :
  tant que la visite est ouverte, quelle que soit la durée du téléchargement, puis `WELCOME_WINDOW` après).
  Import : un import par profil (coché ✓), un autre profil reste importable. Extensions : « Téléchargement… »
  et une barre par carte pendant l'installation, « Installation… » sur le bouton, puis « Installée » ; un
  échec se relance en recochant (l'état arrive par `poll_webext` → `ui_home`).
- Voile de la barre latérale et de la barre du haut : `bhDim(on)` de `theme.js` (voile fixe qui avale les clics),
  envoyé par `sync_dim` à `chrome`, `flyout` et `bar` tant que la fenêtre est ouverte et que l'accueil est devant
  (`start_page_front`) ; appelé à la fin de `ui_state`. Barre du haut : `html.bh-dimmed .win` garde les boutons
  de fenêtre Windows au-dessus du voile, et un glisser sur le voile déplace toujours la fenêtre.
- Navigateur par défaut (`blackhole-macos-url`) : `set_default_browser`, `can_be_default_browser`,
  `is_default_browser`. macOS : dialogue du système, choix lu par `URLForApplicationToOpenURL` (bundle `.app`
  seulement). Windows : l'installeur (`scripts/windows-installer.iss`, section `[Registry]`, HKCU) enregistre
  BlackHole comme navigateur (`RegisteredApplications`, `StartMenuInternet\BlackHole`, ProgId `BlackHoleURL` →
  `BlackHole.exe --open "%1"`) ; l'app ouvre `ms-settings:defaultapps?registeredAppUser=BlackHole` et lit
  `UrlAssociations\https\UserChoice\ProgId`. Le choix est relu au retour du focus (`WindowEvent::Focused`).
  Build `cargo` ou zip portable : non enregistré, la visite le dit (`default_cannot_win`). Un lien reçu ouvre
  une nouvelle fenêtre (`--open`, registre `peers`), pas un onglet de la fenêtre déjà ouverte. Linux : rien.
- Style par défaut d'un nouveau profil : plat (`Settings::default().depth = false`, 0.8.23). Un fichier de
  réglages plus ancien sans `depth` reste en 3D (`default_true` du DTO) : rien ne change pour les profils existants.
- Harnais : un build `devtools` ne montre pas la visite (profils neufs des scripts) sauf avec `BH_ONBOARDING=1`.
  Test Windows : `scripts\test-onboarding.ps1` (27 contrôles `obN:` : ouverture après le splash, voiles, trou à
  mi-transition, étapes, couleur appliquée, fin sans défilement, levée des voiles, rejeu depuis les Réglages,
  animations coupées, Échap, français ; Dark Reader et SponsorBlock installés : réseau requis). Captures prises à l'écran : la fenêtre
  doit rester devant. Tests : `onboarding::tests`, `welcome::tests::the_welcome_tour_drops_…`,
  `release_notes_are_shown_once_after_an_update` (`first_launch`). macOS : compilation croisée seulement.


## 0.8.24 : DNS chiffré, protection contre l'empreinte numérique

- Empreinte numérique : une protection des Shields (`Shields::fingerprinting`, vraie par défaut, bit 4,
  DTO `fingerprinting`, `set_shield "fingerprinting"`, Réglages › Confidentialité). Script
  `ui/fingerprint.js` injecté dans toutes les frames des vues de pages et des zones en direct
  (`views::fingerprint_script`, vide quand la protection est coupée ; comme les autres Shields, fixé à la
  création de la vue : s'applique aux pages ouvertes ensuite, réserves vidées). Graine = secret de la
  session (`session_secret`, `RandomState`) + site de l'onglet (`ancestorOrigins`, deux derniers
  libellés) : stable pendant la visite, différente d'un site à l'autre et à chaque lancement. Relectures
  canvas (`toDataURL`/`toBlob` sur une copie, `getImageData`) et WebGL `readPixels` (octets seulement) :
  un bit d'une valeur tous les 199 pixels ; WebGL `UNMASKED_*` génériques (`WebKit`, `WebKit WebGL`) ;
  audio `getChannelData`/`getFloatFrequencyData` : écart inaudible ; `hardwareConcurrency` arrondi
  (2/4/8) ; pas de `getBattery`. Fonctions remplacées par des `Proxy` (lues comme du code natif, même
  nom). Sites laissés tranquilles : `domain::fingerprint::EXEMPT_SITES` (Docs, Meet, Teams, Zoom,
  Figma…). Limites : pas les Web Workers, ni polices, taille d'écran ou user agent. Test macOS :
  `scripts/test-fingerprint.sh` (10 contrôles).
- DNS chiffré : `Settings::encrypted_dns` (faux par défaut) et `dns_resolver` (id de
  `domain::dns::RESOLVERS` — Quad9 par défaut, Mullvad, Cloudflare, AdGuard — ou adresse `https://`
  validée par `dns::doh_template`). Commandes `set_encrypted_dns`, `set_dns_resolver`,
  `install_dns_profile` (`handle_settings`, `features/dns.rs`). Windows : `views::set_encrypted_dns`
  au lancement → `dns::chromium_switch` (`--enable-features=DnsOverHttps:Fallback/true/Templates/…`,
  retour au DNS du réseau si le résolveur est injoignable), appliqué au redémarrage (`restart` dans
  l'état des Réglages). Le réglage n'a pas encore été vérifié dans WebView2 : le contrôler sur Windows
  (`edge://net-internals` n'existe pas dans WebView2 : comparer une page de test DNS, ex.
  `https://on.quad9.net`). macOS : WebKit suit le DNS du système → profil `dns::mobileconfig`
  (`com.apple.dnsSettings.managed`, identifiant fixe `app.blackhole.dns` : un nouveau remplace
  l'ancien) écrit par `infrastructure::write_dns_profile`, ouvert puis Réglages Système › Profils ;
  désactiver ouvre ce volet (l'utilisateur supprime le profil). Linux : le résolveur du système
  (systemd-resolved, DNSOverTLS) ; l'interface l'explique. Android : non traité.

## 0.8.25 : synchro des extensions, ajout par lien, copier l'image, progression dans la page

- Synchro des extensions (à côté des onglets et de l'historique) : `Settings::extension_sync` (vrai par défaut,
  DTO `default_true`, `SetExtensionSync`), demandée avec l'historique (`HISTORY_EVERY`, appareil qui revient,
  « Sync now » = `HistorySyncNow`). Messages `"v": 2` `extensions_pull` / `extensions` (`infrastructure/ext_sync.rs` :
  id vérifié contre sa source, 200 au plus) ; règle pure `domain::ext_sync::plan` : extensions du market présentes
  pour ce système installées sans question (`install_catalog`), GitHub et lien proposés par une notice (« Ajouter »
  / « Plus tard », `ExtSyncAnswer`, refus gardés pour la session) puis installés par `sync_install` (dernière
  release, meilleur fichier `asset_rank` ; même lien). Ajouts seulement : rien n'est retiré par une synchro.
  App : `features/ext_sync.rs` (`ExtSync`), aiguillage dans `handoff.rs` (`on_extensions` avant `on_ledger`).
  Réglages > Appareils : carte `#histcard` « Ce qui se synchronise » (onglets réglés par appareil, historique,
  extensions si l'hôte d'extensions existe, un « Sync now »).
- Ajouter une extension (Réglages > Extensions) : bouton déroulant `#mkadd` (`#mkaddmenu`, flèches, Échap) : depuis
  GitHub (inchangé) ou depuis un lien (`LinkPrepare`, `mk.ghMode = 'link'`). Règles `domain::market` :
  `direct_package_url` (https, sans identifiants, hôte avec un point ; page du Chrome Web Store ou d'Edge Add-ons →
  adresse de téléchargement du store, `store_package_url`), `direct_id` (`url-<hôte>-<FNV>`), `direct_file_name`,
  `crx_payload` (en-tête CRX2/CRX3 retiré, zip ou xpi tel quel). `catalog::fetch_direct` (https jusqu'à la dernière
  redirection, 64 Mo), `github_ext::check_direct` / `prepare_direct` ; épinglé dans `extensions-github.json` avec
  `direct: true` (catégorie `link`, aucune mise à jour cherchée : on réinstalle depuis le même lien).
- Copier : toute entrée `copy` des menus passe par le shell (`UiCommand::CopyText`, `arboard`), plus par
  `execCommand('copy')` (échouait quand la palette n'avait pas le focus système). « Copier l'image » (`CopyImage`,
  `features/clipboard.rs`) : image téléchargée sur un fil avec l'origine de la page en `Referer`
  (`infrastructure/image_clip.rs` : http(s), 30 Mo, 64 Mpx ; PNG, JPEG, GIF, WebP, BMP, ICO), pixels RGBA vers le
  presse-papiers (`arboard` `image-data`). `page_guard.js` envoie aussi l'image sous le pointeur quand elle est dans
  un lien. Mod+Maj+C (`shift+c`, page d'abord : Google Docs) copie l'adresse de la page (propre si Shields).
- « Enregistrer l'image » (`SaveImage`, même fichier) : même téléchargement, écrit dans le dossier Téléchargements par
  `image_clip::save` (format lu dans les octets, pas l'adresse : `extension_of`, SVG compris ; nom tiré de l'adresse
  par `domain::downloads::image_file_name`, nettoyé, jamais hors du dossier ; `create_new`, nom libre via
  `unique_path`). Le fichier devient un téléchargement terminé (`download_saved` : carte, Bibliothèque).
- Shields YouTube (`shields.js`, aussi dans les cadres : lecteurs intégrés, `youtube-nocookie.com`) : `adPlacements`,
  `playerAds`, `adSlots`, `adBreakHeartbeatParams` retirés de la réponse du lecteur (`JSON.parse`,
  `Response.prototype.json` enveloppés par `Proxy`, `ytInitialPlayerResponse` par un accesseur), sur youtube.com
  seulement ; une pub insérée côté serveur est encore sautée (`.ad-showing` / `.ad-interrupting`, bouton « Skip »).
- Barre de progression dans la page : `ui/progress.js` (cadre principal des onglets) dessine la ligne par
  `html::before` d'une feuille construite (aucun nœud ajouté : pas de souci d'hydratation React). La nouvelle page la
  lance au début du document ; le shell (`page_progress`) la teinte à `LoadStart` (`tint`, WebView2 : `ContentLoading`,
  après l'engagement) et la termine à `Loaded` / `Failed` (`done`). Avant (adresse saisie qui attend son serveur,
  `Running::pending`), la barre du haut garde une ligne fine juste au-dessus de la carte (champ `serverWait`) : un script
  évalué après `load_url` s'exécute déjà dans le document suivant sous WebView2.
- PiP d'un direct (Twitch) : `media_devices.js` répond `play` / `pause` de la Media Session tant que la vidéo flotte
  (durée infinie ou de plus de deux jours, ou `srcObject`), sinon Chromium n'affiche pas de bouton pause.
- Accès rapide : « Vue scindée » dans le menu d'une tuile ; la taille des tuiles n'est plus que dans Réglages >
  Général (`tileSizeItems` retiré des menus de tuile et de zone, clé `sidebar.tile_size` supprimée).
- Réglages : à chaque ouverture (`settings_opened`, champ `opened` de l'état), le trou noir de la visite
  (`#sx-warp`, `sx-swallow`) avale la page puis les Réglages en sortent (`sx-emerge`) ; rien en animations coupées
  ni sous `prefers-reduced-motion`.
- Tests : `ext_sync::tests` (domaine et infrastructure), `market::tests` (`direct_*`, `crx_payload`),
  `github_ext::tests::a_package_from_a_link_…`, `image_clip::tests`, réglage relu (`extension_sync`). Scénario
  Windows : `scripts\test-0825.ps1`. macOS : compilation croisée seulement ; synchro entre deux appareils, PiP
  Twitch et pubs YouTube non vérifiés en réel.

## 0.8.26 : reprise de la carte média, adresse des pages à une page, ouverture du split, Android

- Carte média (lecture / pause, `MEDIA_TOGGLE_JS`) : `media_devices.js` garde les gestionnaires `play` / `pause` de
  la Media Session que la page déclare (`setActionHandler` enveloppé, `window.__bhMs` non énumérable) ; le bouton les
  appelle d'abord, puis l'élément (`play()` 400 ms plus tard s'il est encore en pause). Un `play()` venu de dehors était
  remis en pause par un lecteur qui se croyait arrêté (YouTube). Mêmes médias que la sonde (audibles, ou grande vidéo
  hors boucle) ; ceux qu'il a mis en pause sont marqués `__bhHeld` et reprennent d'abord. Le PiP d'un direct remet les
  gestionnaires de la page en partant (`own.play`), au lieu de les effacer. Pendant qu'il flotte, `playbackState` suit
  la vidéo (`follow`, événements `play` / `pause`) puis reprend sa valeur d'avant : réglé une fois, il restait
  « playing » après une pause et le bouton de la fenêtre remettait en pause au lieu de relancer (Twitch).
- Adresse après `pushState` : `page_guard.js` envoie `location` avec `url: location.href` ; `page_moved` la garde si
  `domain::omnibox::same_document_url` l'accepte (http(s), même origine que l'adresse du moteur, sans identifiants),
  sinon l'adresse du moteur. Relue aussitôt, celle du moteur était encore l'ancienne (WebView2 et WebKit l'apprennent
  un peu après) : la barre restait sur youtube.com jusqu'au rechargement.
- Ouverture du split (`pane_motion`) : `OPEN` = 420 ms sur `cubic-bezier(.2,.85,.25,1)` (`ease_open`, résolu dans
  `bezier`), comme `pane-in` de `splitbar.html`, qui fait maintenant glisser le titre de toute sa largeur (+ 8 px) ;
  le volet attend `PAINT` (32 ms) hors de vue avant de bouger (sa page est peinte), la barre a le même délai.
- Android : Réglages par sections (`#s-root`, `.sec[data-sec]`, `showSection` / `openSettings(nom)`, retour système et
  flèche : la liste d'abord, puis la page) : Général, Apparence, Confidentialité, Appareils, Données, Mises à jour,
  chacune avec son résumé (`settingsSummary`). Style 3D / Plat du bureau (`Settings::depth`, `set_setting` `depth`,
  `body.flat`) ; relief `--relief-*` recopié de `ui/depth.js` (à garder en phase), plus doux en thème clair. Menu ⋮
  en groupes (`menu_page`, `menu_tab`) et pied `.quick.foot` (Historique, Téléchargements, Réglages). Choix à puces
  sous leur libellé (`.opt.stack`).
- Tests : `omnibox::tests::a_page_reports_only_an_address_of_its_own_origin`, `pane_motion::tests` (attente de peinture,
  courbe), `settings_survive_a_restart` (Android, `depth`). Scénario Android : `sections`, `section`, `flat`, `relief`,
  `sectionback`, `menugroups` (non lancé ici : pas d'appareil). Reprise vérifiée sur YouTube et Twitch dans Chromium (script de la
  carte) ; WebView2 réel, macOS (compilation croisée seulement) et animation du split non vérifiés en direct.

## Release, installeur et mise à jour
- Workflow `.github/workflows/release.yml` : tests (Windows + macOS + Linux), `.deb` et archive Linux (`scripts/package-linux.sh`), installeur Inno Setup + zip portable, binaire macOS universel (`scripts/package-macos.sh`), `SHA256SUMS.txt`. Publie sur push `main` quand `v<version de Cargo.toml>` n'a pas de tag, ou sur tag `vX.Y.Z` (doit correspondre à `Cargo.toml`).
- Version : `Cargo.toml` (+ `Cargo.lock`). Pour publier : incrémenter, committer, pousser.
- Installeur Windows local :
  ```
  cargo build --release -p blackhole
  iscc /DAppVersion=X.Y.Z /DExePath=..\target\release\blackhole.exe scripts\windows-installer.iss
  ```
  Sortie dans `dist\`. L'icône vient de `scripts/make_icon.py` (PNG macOS + `.ico` Windows).
- Mise à jour dans l'app (`updater.rs`) : bouton `#update` en bas de la sidebar (le flyout de survol est une 2e vue chrome, recréée avant la palette dans `raise_palette`) ; Windows lance l'installeur en silencieux, macOS remplace le `.app`. Désactivée pour les builds `cargo` (dossier `target`) et `devtools`. Recherche par la redirection `github.com/<repo>/releases/latest` puis `SHA256SUMS.txt` du tag (jamais l'API REST : 60 requêtes/h anonymes par IP, vite épuisées derrière un réseau d'entreprise) ; échec → nouvel essai 30 min plus tard. Réglages > Général > Updates : vérification manuelle (`check_app_update`, permise partout), installation seulement si `can_self_update()`, sinon lien vers la page de la version. Installer (barre latérale ou Réglages) fait décoller un vaisseau (`launchShip` / `bhLaunch`, icône `rocket`), absent en animations coupées. Test réseau : `cargo test -p blackhole-infrastructure updater -- --ignored`.

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
7. Tout nouveau texte visible passe par les catalogues (`en.toml` et `fr.toml`).

## 0.8.27 : privé / public, Shields dans les frames, Meet et Docs, rangement, téléchargements

- Pas de `--process-per-site` dans les arguments de WebView2 (`views::builder`) : toutes les pages d'un site partageaient
  un renderer, Meet tournait sur le même fil que Gmail, Agenda et Docs (partage d'écran flou ou pixelisé, Docs lent ;
  ajouté le 3/11, signalé dès le 4/11). L'argument `--disable-features=…` tient en un seul mot (une espace faisait de
  `msSmartScreenProtection` un argument à part). `media_devices.js` ne touche plus au partage d'écran (l'enveloppe de
  `RTCRtpSender.setParameters` forçait 3 Mb/s et la pleine résolution sur chaque couche, contre la régulation de Meet).
- Docs, Sheets, Slides, Drive, Apps Script : `page_guard.js` ne prend plus le clic droit en phase de capture (menu de
  l'éditeur, Copier/Coller avec mise en forme) ; le nôtre ne vient que si la page n'en montre aucun (phase bulle).
- Blocage natif Windows (`win::block_hosts`) : `ICoreWebView2_22::AddWebResourceRequestedFilterWithRequestSourceKinds`
  (`…_SOURCE_KINDS_ALL`), repli sur l'ancien filtre. Un traqueur dans une frame d'un autre site (processus à part)
  n'était ni refusé ni compté : le compteur du bouclier restait bas sous Windows.
- Privé / public : `peers::drop_target` (`Drop::Into` / `Refused` / `Nowhere`) et `Transfer::private` ; un onglet privé
  lâché sur une fenêtre normale (ou l'inverse) reste où il est (notice `toast.tab_other_privacy`), et `receive` jette un
  envoi de l'autre sorte. Extensions en privé : `Settings::private_extension_settings` (vrai par défaut, sous
  `extensions_in_private`, `SetPrivateExtensionSettings`, Réglages > Extensions `#extpriv`) ; avant le moteur,
  `infrastructure::seed_private_extensions` copie `Local/Sync/Managed Extension Settings`, `Extension State`,
  `Extension Rules`, `DNR Extension Rules` et l'IndexedDB `chrome-extension_*` du profil principal (jamais cookies,
  historique ni stockage des sites ; `LOCK` sauté). Rien ne revient. Windows seulement (macOS : stockage déjà gardé).
- Rangement : chaque groupe de `tidy_json` porte ses `tabs` ; puces cochables (`tidySkip`, `UiCommand::Tidy { skip }`,
  règle `domain::tidy_kept`), survol de la carte = onglets à ranger surlignés (`.tab.tidy-lit`, `--tg` par site), survol
  d'une puce = ce site seul.
- Téléchargements : `RevealDownload { id }` → `downloads::reveal_file` (macOS `open -R`, Windows `explorer /select,"…"`
  par `raw_arg`, Linux dossier) ; bouton dossier `.cl.fo` de la carte terminée, bouton de la notice « Show in folder »,
  ligne de la Bibliothèque. Visionneuse : `domain::downloads::viewer_for` / `viewable_file` (PDF, images, vidéo, son,
  texte ; jamais HTML/SVG/XML) ; `PrivacyPolicy::evaluate` et `resolve_input` acceptent ces `file:` ; Library/accueil
  ouvrent un tel fichier dans un onglet (Windows, Linux ; macOS : l'app du système, WKWebView exige `loadFileURL`).
- Notices : un clic sur une notice sans bouton la retire partout (`notice_done`) ; `#toast.show` reçoit les clics.
- Bibliothèque : section Espaces et `LibraryOpenTab` retirés (sept sections ; huit avec Shields en 0.8.28).
- Workflow Market : `track` ne touche plus une PR `market/update` qui a déjà la même liste (`cmp`) ; sinon
  `create-pull-request` la rebasait chaque jour sur `main` et son commit neuf relançait les 48 h de `merge`.
- Test Windows : `scripts\test-0827.ps1` (compteur du bouclier : page, frame du même site, frame d'un autre site, script ;
  notice cliquée ; bouton dossier ; Bibliothèque ; option des extensions en privé). Tests : `peers::tests`,
  `a_private_window_starts_with_the_extensions_settings_only`, `the_sites_unticked_on_the_card_stay_loose`,
  `the_engine_views_documents_pictures_and_media_never_pages`. macOS : compilation croisée seulement ; Meet et Docs non
  vérifiés en réel (compte requis).

## 0.8.28 : récapitulatif des Shields (Bibliothèque)

- Bibliothèque > Shields (huitième section, `LIBRARY_SECTIONS`, icône `shield`) : à gauche les sites de l'espace
  affiché puis les autres sites du journal (`BlockLog::sites`), et le total de la session (traqueurs, pubs, liens
  nettoyés, temps gagné) ; à droite la note dans son anneau, cinq tuiles (traqueurs, pubs, liens nettoyés de la
  session, bandeaux cookies et empreinte : interrupteurs des Shields), la liste des hôtes refusés (filtre Tous /
  Traqueurs / Pubs, propriétaire et nature, `appris`, `autorisé`, boutons `tracker_allow` / `tracker_block`), le
  « Pourquoi » de la note et les entreprises. Données : `Running::shields_json` (`features/privacy.rs`), dans l'état
  de la Bibliothèque (`shields`).
- Règles pures : `domain::shields_report` (`owner_of` : table courte domaine → entreprise et nature, sous-domaines
  compris ; `companies` : regroupement, un hôte inconnu sous son domaine de base ; `time_saved_secs` : estimation de
  `MS_PER_REQUEST` 50 ms par requête refusée, affichée « ≈ »). Un hôte compte parmi les pubs si la liste des pubs
  le dit ou si son propriétaire est une régie (`is_ad_host`, partagé avec le menu du bouclier).
- En direct : `on_blocked` lève `shields_dirty` ; `service_shields` (boucle) envoie `bh.shields(…)` à la Bibliothèque
  ouverte au plus toutes les `SHIELDS_EVERY` (400 ms) et redemande un réveil tant qu'un envoi attend. Autoriser ou
  rebloquer un hôte relance `ui_library`.
- Menu du bouclier : entrée « See the Shields report » → `open_library` `section: "shields"` ; la page choisit alors
  le site affiché (`jump`).
- `Trackers::block_again` retrouve l'entrée autorisée comme `allow` la garde (`www.` retiré) ou un domaine
  au-dessus : le rapport nomme l'hôte refusé, pas l'entrée.
- Test Windows : `scripts\test-0828.ps1` (section ouverte sur le site affiché, propriétaires, entreprises, tuiles,
  Autoriser puis Bloquer à nouveau, filtre Pubs, compteur en direct, pas de défilement horizontal). Tests :
  `shields_report::tests`, `site_tests`, `allowed_hosts_stay_allowed_until_blocked_again`. macOS : compilation
  croisée seulement.

## 0.8.29 : privé étanche, Google sur macOS, ⌘Z, adresse longue

- Rien de public ne sort d'une fenêtre privée : `open_window` passe par `peers::opens_private` (⌘N, « nouvelle fenêtre »,
  onglet lâché) ; palette et menu de lien n'offrent plus de fenêtre normale, libellés « onglet privé »
  (`state.private`). Un lien venu d'une autre app (`Msg::External`, macOS le donne à n'importe quel processus) part vers
  une fenêtre normale (`hand_external`, `peers::normal_window`). `live_view(private)` : session privée, plus le profil.
- macOS : `DataStore::Session` (un magasin en mémoire par processus) pour `Store::Private` ; avant, chaque onglet privé
  avait le sien. Trou noir : toujours un magasin par onglet.
- Traces du privé : les pages du shell (`shell_builder`) tournent sur la session en mémoire (macOS) : elles chargent les
  icônes des sites, qui finissaient dans le cache et avec les cookies du profil. `infrastructure/private.rs` : dossier
  jetable 0700, `prepare` (copie de `settings.json` seulement), `sweep` (dossiers des processus morts, à chaque
  lancement) ; `erase_private` dans `exiting` (⌘Q ne revient pas dans `main`). `ExtensionHost::new(ephemeral)`. Choix assumé :
  les extensions que l’utilisateur a autorisées en privé y gardent leurs réglages (macOS : même stockage).
- `outerWidth` / `outerHeight` / `screenX` / `screenY` valaient 0 (macOS) : `_webView:getWindowFrameWithCompletionHandler:`
  (délégué UI de wry) rend le cadre de la vue. Flou de Meet : le flux WebRTC en boucle locale est identique à une
  WKWebView nue (mêmes résolution, débit, QP) ; en mode économie d'énergie WebKit plafonne `requestAnimationFrame` à 30/s.
  Non vérifié dans un vrai appel.
- Appel masqué (macOS) : WebKit ralentit les scripts d'une page cachée (1 image/s envoyée pour une vidéo produite par
  script). `service_calls` (`tabs/media.rs`) appelle `run_while_hidden` pour les onglets en appel seulement (préférences
  privées, testées avant usage) et remet les valeurs de WebKit à la fin. 19 à 28 images/s mesurées. Chromium le fait seul.
- ⌘Z, ⇧⌘Z, ⌘A (macOS) : `performKeyEquivalent:` (vues enfants) les donne d'abord à la page ; si elle ne les prend pas,
  `page_first_keys.js` (toutes les frames) lance `undo` / `redo` / `selectAll`. WebKit ne renvoie pas l'événement au menu.
- Champ d'adresse (`bar.html`) : centré sur la fenêtre avec une largeur fixe, il recouvrait les boutons de part et
  d'autre d'une fenêtre étroite (demi-écran) ; `fit()` le borne (`--room`) entre eux. Le texte est coupé dans le champ.
- Test macOS : `bash scripts/test-0829.sh` (étape harnais `press z|Z|a`). Tests : `safari::tests`,
  `a_private_window_only_opens_private_ones_and_keeps_outside_links_out`.

## 0.8.30 : alias email

- Réglages > Email aliases (section entre Passwords et Extensions) : service (SimpleLogin, Addy.io, Firefox Relay), clé,
  serveur (auto-hébergé possible sauf Relay), options, liste des alias (copier, couper, supprimer). Pastille « alias » dans
  la liste des mots de passe (`Aliases::is_alias`). BlackHole ne relaie rien : rien ne part avant la connexion d'un service,
  puis seulement sur un clic.
- Règles `domain/aliases.rs` (`Provider::server` : https ou machine locale seulement ; `reaches` : seul le compte qui a fait
  un alias agit dessus ; `valid_email` / `valid_remote` : réponses du service contrôlées). API `infrastructure/aliases.rs`
  (bloquant, hors du thread UI, `redirects(0)` : la clé ne suit jamais une redirection). SimpleLogin n'a qu'une bascule :
  rebasculée si l'état rendu n'est pas celui voulu.
- Clé et alias dans `passwords.bin` (`Passwords::aliases`), jamais dans `Settings` ni dans une page. Une version plus ancienne
  qui réécrit le portefeuille les perd (champs inconnus). `Settings` : `alias_provider` (indice seulement), `alias_suggest`,
  `alias_share_site` (nom du site donné au service : faux par défaut).
- Page : `autofill.js` `isAliasField` (email d'inscription ou de newsletter ; pas avec `current-password`, ni un champ
  `username` seul), `autofill_query { login, email }`, `autofill_alias { index?, token }` sous le même jeton à usage unique.
  Un alias par site : s'il en existe un actif il est proposé, sinon « Create ». `alias(token, email)` écrit dans le champ
  demandé même si le focus a bougé ; `aliasGone` vide ce champ si l'alias est supprimé (Undo de la notice).
- App `features/aliases.rs` (`Aliases`, `AliasAction`, `{"cmd":"aliases","action":{"op":…}}`). Portefeuille verrouillé pendant
  la création : `unsaved`, écrit à l'ouverture. Fenêtre privée : lecture seule. Linux : pas de portefeuille, donc pas d'alias.
- Test Windows : `scripts\test-0830.ps1` (faux SimpleLogin local ; vraies frappes par le port DevTools du moteur).
  Tests : `aliases::tests` (domain, infrastructure), `the_alias_account_and_the_aliases_are_sealed_with_the_rest`. Non
  vérifié avec de vrais comptes (Relay : API non documentée pour des tiers). macOS : non lancé. Android : sans alias.
