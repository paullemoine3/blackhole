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
- Windows : toutes les vues d'un même dossier de données doivent avoir les mêmes réglages moteur (extensions activées, GPU, arguments) → toujours passer par `views::builder()`.
- Je ne peux pas compiler macOS sous Windows : rester prudent, utiliser `cfg`, et signaler que macOS n'a pas été testé.

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
cargo test --workspace
```
- Tests unitaires dans le crate concerné (domaine/application/infra en priorité). Tout nouveau comportement logique reçoit un test ciblé.
- **Scénarios UI Windows** (feature `devtools`, harnais `crates/app/src/devtools.rs`) :
  - `scripts\test-ui.ps1` : navigation, palette, paramètres, animations (sidebar, palette), bouton de mise à jour, maximize/restore. 17 captures dans `%TEMP%\bh_shots`.
  - `scripts\test-ext.ps1` : installe Dark Reader depuis le catalogue et vérifie l'injection (utiliser une page claire comme info.cern.ch ; example.com gère déjà le mode sombre).
  - Profil jetable `%TEMP%\bh_data` ; le script lit `%TEMP%\bh_script.txt`.
  - Protocole du script : `wait MS`, `shot NOM`, `shots NOM` (fenêtre des paramètres), `cmd JSON` (un `UiCommand`), `eval chrome|bar|home|palette|peek|settings|content JS`, `quit`. Les contrôles lisent les `eval` par indice : **ajouter un `eval` décale les indices suivants**, mettre à jour les `Check`.
  - Les captures sont prises à l'écran : la fenêtre doit être visible et non recouverte. Vérifier visuellement les images.
  - Si l'appli reste en vie après un timeout, arrêter le process par son id (`Stop-Process -Id`) avant de relancer.
- **macOS** : `scripts/test-webext.sh` (installe uBOL, vérifie le blocage actif/inactif, retrait) et le harnais `devtools` avec captures CoreGraphics (`/tmp/bh_script.txt` → `/tmp/bh_shots/`).
- Toute nouvelle fonctionnalité d'UI : ajouter un scénario (et un `Check`) dans le script de la plateforme concernée, avec capture ; toute animation : tester l'état intermédiaire et le mode animations désactivées.

## Extensions
- macOS : `WKWebExtensionController`. Windows : `ICoreWebView2Profile7::AddBrowserExtension` (zips Chromium dépaquetés dans `<données>/webext/unpacked/<id>`, `catalog::remove` supprime aussi ce dossier).
- Bitwarden : build Firefox sur macOS, build Chromium (`chromium`/`windows` dans `CatalogEntry`) sur Windows.
- Ajouter une extension au catalogue : URL de la release officielle, télécharger puis calculer le SHA-256, ajouter une entrée avec test (`catalog.rs` vérifie longueurs de hash et https).
- Limites WebView2 : pas de badge, pas d'onglet actif exposé aux extensions ; le popup est une vue `chrome-extension://<id>/<popup>` sur le même profil.

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
