# Consignes pour les agents — BlackHole

Ces consignes s'appliquent à tout le dépôt. Répondre en français, de façon concise.
Ce fichier est l'index de référence (règles courtes) ; `.github/copilot-instructions.md`
détaille les mêmes sujets (protocole des scripts UI, mémoire, Easel, release). En cas de
doute, suivre le plus strict. Voir aussi `README.md` (commandes) et `docs/macos.md`.
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
  avant la fenêtre qui l'héberge.
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

- macOS : `bash scripts/test-webext.sh`, `bash scripts/test-easel.sh`.
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
- Icônes de menu : noms (`icon: 'trash'`) dessinés en SVG par `palette.html` (`ICONS`), pas d'emoji.
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
  menu : nouvel onglet, nouvelle fenêtre, fenêtre privée, Peek, copier). Maj+clic droit ou une
  sélection gardent le menu du moteur. Une page ne peut pas ouvrir de fenêtre elle-même : seule
  la commande d'interface `open_link` a un `target`.
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
- Copilote de réunion : `Meeting::notes_template`, `agenda_of` ; `meet.rs` crée ou rouvre l'Easel
  « Notes · titre · jour » et ouvre la fenêtre Easel (réglage `meeting_notes`).
- Veille de zone : `domain/watch.rs`, `WatchStore` (`watches.json`), `app/watching.rs` (une vue
  cachée à la fois, lecture à 8 s, abandon à 14 s, toutes les 30 min), section Library « Watched
  Areas ». Démarrée depuis le menu du sélecteur de zone (`WatchArea`).
- Comparateur : `ui/compare.js` (JSON-LD Product, Open Graph, tableau de caractéristiques) lancé
  dans chaque onglet chargé, `app/compare.rs`, palette en mode `compare` (données de page
  insérées par `textContent` uniquement).
- `ureq` utilise les certificats du système (`native-certs`) : nécessaire derrière un antivirus
  qui inspecte le HTTPS ; les téléchargements gardent leur vérification SHA-256.
- `test-easel.ps1` : un `eval` qui ne fait que piloter l'interface renvoie `__skip…` et n'est pas
  compté dans les indices des `Check`.

## Pages d'erreur et User-Agent

- Page « Site injoignable » (`ui/unreachable.html`) : affichée via `PageLoadEvent::Failed`
  (vendor/wry, WebView2 non testé) ou après `CONNECT_MAX` (20 s) sans démarrage de
  chargement ; test macOS `scripts/test-unreachable.sh`.
- User-Agent macOS : `views::user_agent_for` choisit l'UA à chaque navigation principale
  via `wry::USER_AGENT_RESOLVER` (UA Safari forcé pour Meet, UA WebKit par défaut pour
  `docs.google.com` : le token Safari rend Docs flou). Windows non concerné.

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
  L'installeur Windows utilise `scripts/windows-installer.iss` avec Inno Setup.
- Terminer par un résumé des changements, des vérifications effectuées et des
  limites, notamment pour l'autre système d'exploitation.
