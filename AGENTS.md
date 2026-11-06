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

## Livraison et entretien

- Préserver les changements utilisateur déjà présents ; limiter les modifications
  au périmètre demandé. Ne pas committer ni publier sans demande.
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

- Page « Site injoignable » (`ui/unreachable.html`) : affichée via `PageLoadEvent::Failed` (vendor/wry, WebView2 non testé) ou après `CONNECT_MAX` (20 s) sans démarrage de chargement ; test macOS `scripts/test-unreachable.sh`.
- User-Agent macOS : `views::user_agent_for` choisit l'UA à chaque navigation principale via `wry::USER_AGENT_RESOLVER` (UA Safari forcé pour Meet, UA WebKit par défaut pour `docs.google.com` : le token Safari rend Docs flou). Windows non concerné.
