# BlackHole Android — état des lieux et proposition (étape 1)

Document de travail à valider avant tout code. Les maquettes sont dans
[`maquettes.html`](maquettes.html), à ouvrir dans un navigateur. Elles sont animées et
proposent deux bascules : « animations réduites » et thème clair.

## 1. Faisabilité technique

### 1.1 wry 0.57 sur Android (`vendor/wry/src/android`)

Constats tirés du code vendoré :

- **Une seule WebView par Activity.** `WEBVIEW_ATTRIBUTES` est indexé par `ActivityId`, et
  `WryActivity.setWebView` remplace le contenu de l'Activity. BlackHole superpose plusieurs vues
  natives : la barre, la palette, les onglets, les vues de réserve. Ce modèle ne s'y prête pas.
  Avec wry, chaque onglet serait une Activity distincte, avec un écran noir à chaque
  changement et sans animation possible entre deux onglets.
- **Il passe par `tao`, pas par `winit`.** `InnerWebView::new` attend un `RawWindowHandle::AndroidNdk`
  dont le pointeur est en réalité l'objet Java `WindowManager` fourni par tao
  (`tao::android_binding!` + `wry::android_setup` + `wry::android_binding!`, macros
  `tao-macros`). winit 0.30 passe par `android-activity` (NativeActivity/GameActivity) et
  fournit un vrai `ANativeWindow*`. wry le prendrait pour un `JObject` : erreur
  `UnsupportedWindowHandle`, ou plantage JNI. Il faudrait donc un deuxième runtime de fenêtrage,
  tao, en plus de winit.
- **Les ajouts BlackHole n'existent pas côté Android.** Les handlers ajoutés au vendoring n'ont
  pas d'implémentation Android : `with_load_failure_handler`, `with_server_trust_handler`,
  `with_http_auth_handler`, téléchargements, nouvelles fenêtres, `USER_AGENT_RESOLVER`,
  profils de données (`Store`). Il faudrait les écrire dans le Kotlin de wry.
- **`crates/app` n'est pas réutilisable.** Son `main.rs` (≈ 5 600 lignes) suppose winit, plusieurs
  vues par fenêtre, `rfd`, `arboard`, `muda`, `sysinfo` et un découpage desktop (sidebar
  redimensionnée, hotzone, vues paresseuses). Le porter reviendrait à le réécrire tout en
  risquant de faire régresser Windows et macOS.

**Conclusion : ne pas utiliser wry sur Android.** Le plus simple et le plus sûr est une **coque
Android native en Kotlin**, qui gère directement les `android.webkit.WebView` (une par onglet
vivant, plus une vue d'interface). Elle s'appuie sur un **cœur Rust** (`cdylib`, appelé par JNI)
qui réutilise tels quels `domain`, `application` et une partie d'`infrastructure`.
`vendor/wry` reste intact.

### 1.2 Compilation `aarch64-linux-android` des crates internes

| Dépendance | Crate | Verdict Android | Action |
|---|---|---|---|
| `url`, `zeroize`, `thiserror`, `serde(_json)` | domain/app./infra | OK | — |
| `rusqlite` (bundled) | infra | OK : SQLite est compilé par le clang du NDK | `cargo-ndk` fournit `CC`/`AR` |
| `ring` (via rustls/ureq) | infra | OK avec le clang du NDK | idem |
| `chacha20poly1305`, `argon2`, `sha2` | infra | OK (pur Rust) | — |
| `chrono` (clock), `chrono-tz`, `rrule`, `csv` | infra | OK (`android-tzdata` est déjà prévu par chrono) | — |
| `keyring` 3 (`apple-native`, `windows-native`) | infra | **Compile, mais aucun magasin persistant sur Android** (magasin factice) : la clé du coffre serait perdue à chaque lancement | Fournir la clé depuis l'**Android Keystore** (côté Kotlin : clé AES non exportable qui enveloppe une clé de 32 octets). Constructeur `Vault` à clé explicite sous `cfg(target_os = "android")` ; `keyring` passe en dépendance non-Android |
| `ureq` + `rustls-native-certs` 0.7 | infra | Compile, mais `load_native_certs` ne trouve **aucun certificat** sur Android (`/system/etc/security/cacerts` n'est pas lu) | Aucun appel réseau `ureq` dans le périmètre proposé : les téléchargements passent par la WebView et le `DownloadManager`. À traiter plus tard (catalogue, agenda), par exemple avec `webpki-roots` sous `cfg(android)` |
| `dirs` | infra | `data_dir()` / `download_dir()` renvoient `None` → repli sur le dossier temporaire | Les stores prennent déjà un `&Path` (`Store::new(data)`) : le chemin vient de Kotlin (`context.filesDir`). `infrastructure::data_dir()` n'est pas appelé sur Android |
| `import.rs` (import Chrome/Firefox), `updater.rs`, `catalog.rs` | infra | Compilent, sans objet sur Android | Non appelés |
| `wry`, `winit`, `rfd`, `arboard`, `sysinfo`, `muda`, `softbuffer`, `xcap` | app | Hors sujet | Le crate Android ne dépend **pas** de `crates/app` |
| `macos-webext`, `macos-url` | — | Hors sujet | Non dépendus |

Seuls changements nécessaires dans les crates partagés : un `cfg(target_os = "android")` pour
la clé du coffre, et `keyring` déplacé en dépendance `cfg(not(target_os = "android"))`.
Windows et macOS ne changent pas.

Ce tableau vient de la lecture du code. Il sera confirmé par un vrai
`cargo ndk -t arm64-v8a build`, dès que le NDK sera installé.

### 1.3 `unsafe`

Les points d'entrée JNI ont besoin de `#[no_mangle] extern "system"`, que la lint
`unsafe_code` signale. Comme pour `macos-webext`, il faudra autoriser `unsafe_code` dans le nouveau crate
seulement, et le confiner au module `jni` (sous `cfg(target_os = "android")`). La logique du
crate reste testable sur l'hôte, sans `unsafe`.

## 2. Outils

Installé : Rust 1.99.1 (cibles `x86_64-pc-windows-msvc`, `aarch64-apple-darwin`,
`x86_64-apple-darwin`), un **JRE 8** Oracle (insuffisant : AGP 8 exige un JDK 17) et `~/.android/adbkey`
(clé adb). **Absents** : JDK 17, Android SDK, NDK, Gradle, adb, émulateur, `cargo-ndk`,
`cargo-mobile2`. L'hyperviseur est présent (`HypervisorPresent = True`). La plateforme
d'hyperviseur Windows (WHPX), qu'utilise l'émulateur, n'a pas pu être vérifiée sans élévation.

**Rien n'a été installé.** Liste proposée. L'emplacement suggéré est `E:\Android`, car E: a 578 Go
libres contre 242 Go sur C:.

| Outil | Rôle | Téléchargement | Sur disque |
|---|---|---|---|
| JDK 17 (Microsoft OpenJDK ou Temurin, zip) | Gradle/AGP | ~190 Mo | ~320 Mo |
| Android `cmdline-tools` (sdkmanager) | installation du SDK | ~150 Mo | ~300 Mo |
| `platform-tools` (adb) | install/captures | ~15 Mo | ~40 Mo |
| `platforms;android-35` | compilation | ~65 Mo | ~150 Mo |
| `build-tools;35.0.0` | aapt2, apksigner | ~60 Mo | ~150 Mo |
| `ndk;27.2.12479018` (r27c) | clang pour Rust/SQLite/ring | ~700 Mo | ~2,6 Go |
| Gradle 8.x (wrapper) + caches AGP/Kotlin/androidx | build de l'APK | ~130 Mo + ~600 Mo | ~1,5 Go |
| `rustup target add aarch64-linux-android x86_64-linux-android` | std des cibles (arm64 pour les téléphones, x86_64 pour l'émulateur) | ~2 × 60 Mo | ~2 × 200 Mo |
| `cargo install cargo-ndk` | compilation Rust → `jniLibs` | quelques Mo | ~10 Mo |
| **Sous-total build** | | **≈ 2 Go** | **≈ 5,5 Go** |
| `emulator` | émulateur | ~350 Mo | ~1 Go |
| `system-images;android-35;google_apis;x86_64` | image Android 15 | ~1,5 Go | ~4 Go |
| AVD (données, 1 appareil type Pixel) | | — | ~3–6 Go |
| **Sous-total émulateur** | | **≈ 1,9 Go** | **≈ 8–11 Go** |

Option sans émulateur : un téléphone Android réel en débogage USB, avec adb seulement.
`cargo-mobile2` n'est pas utile : il génère un projet tao+wry, écarté au § 1.1.

## 3. Périmètre « de base »

**Inclus (v1)** :

1. **Onglets** : ouvrir, fermer avec Undo, réordonner. Le modèle façon Arc (`domain/workspace.rs`)
   est réutilisé tel quel : nouvel onglet en haut, successeur à la fermeture, accès rapide mis en veille.
   Le plafond d'onglets vivants (`overflow_to_release`) est conservé, avec une valeur plus basse sur mobile (4).
2. **Barre d'adresse et recherche** : `domain::omnibox`, moteur des Réglages, suggestions tirées de
   l'historique et de l'accès rapide.
3. **Favoris / accès rapide** : tuiles épinglées, sur l'accueil et dans le tiroir.
4. **Historique** : `HistoryStore` (SQLite) ; recherche et effacement.
5. **Onglet privé** : profil WebView séparé et éphémère (voir § 5.3), aucune écriture d'historique.
6. **Shields** : traqueurs et pubs (`PrivacyPolicy::with_ads`, mêmes listes),
   `shields.js` (masquer les pubs, sauter les pubs YouTube, bandeaux cookies).
7. **Rapport de confidentialité** : `BlockLog::for_site`, `privacy_grade`.
8. **Téléchargements** : `DownloadListener` → `DownloadManager` Android (dossier
   `Download/BlackHole`), carte de progression dans le tiroir, https seulement.
9. **Réglages essentiels** : moteur, thème, couleur d'espace, animations, Shields, effacement des
   données, dossier de téléchargement, à propos.
10. **Thème et couleurs d'espace** : espaces (`Space`) avec leur couleur. Cookies isolés par espace
    si le profil WebView le permet (§ 5.3).
11. **Pages d'erreur sci-fi** : `error.html` et `domain::error_page`, avec une nouvelle variante
    `Failure::Android(code)` testée (codes `WebViewClient.ERROR_*` et `SslError`).
12. **Retour Android, zones sûres, sombre/clair, animations réduites.**

**Exclus ou à plus tard** :

- **Exclus de la v1** : Easel, extensions, outils de développement, PiP natif, split view,
  Peek, calendrier et Meet, comparateur, veille de zone, rangement automatique (tidy),
  import depuis d'autres navigateurs, mises à jour intégrées (le Play Store ou l'APK s'en chargent).
- **Portefeuille de mots de passe et autofill** : exclus de la v1. Il demande un audit de sécurité
  propre à Android (pont `autofill.js`, Keystore, `FLAG_SECURE`).
- **Authentification HTTP** : candidat v1.1 (`onReceivedHttpAuthRequest` → invite de la palette),
  les règles sont déjà dans `domain/http_auth.rs`.
- **« Continuer sur l'autre appareil »** : candidat v1.2. Le protocole (`domain/handoff.rs`,
  `infrastructure/handoff.rs`, UDP 47807, ChaCha20-Poly1305) est pur Rust et se compile pour Android.
  Il faut en plus `CHANGE_WIFI_MULTICAST_STATE`, un `MulticastLock` et l'écran d'appairage.

## 4. UI mobile

Les maquettes sont dans `maquettes.html`, en 8 écrans : accueil, navigation, grille d'onglets, tiroir,
palette, rapport Shields, page d'erreur, réglages.

- **Barre d'adresse en bas**, à portée du pouce : ☰ (tiroir), champ d'adresse avec bouclier et
  progression, compteur d'onglets, ⋮ (palette). Elle se masque au défilement et revient au
  moindre geste vers le haut.
- **Glisser la barre horizontalement** passe à l'onglet voisin. Les deux WebViews suivent le doigt
  (`translationX` natif) et des points indiquent la position.
- **Grille d'onglets** à deux colonnes, avec les vignettes réelles (`drawToBitmap` réduit, envoyé en
  `data:` à l'interface). Balayer pour fermer (Undo 15 s), appui long pour réordonner ou épingler,
  bascule « Private ». Les cartes entrent avec le `pop` du bureau.
- **Tiroir = sidebar** : il s'ouvre depuis le bord gauche ou par ☰ et suit le doigt, puis termine en
  ressort. Il glisse à **largeur fixe**, sans redimensionnement par image, comme la règle desktop.
  Contenu : espaces, accès rapide, onglets, Library (historique, téléchargements, réglages),
  carte de téléchargement.
- **Palette = feuille du bas** : saisie d'adresse, suggestions et actions sur une seule surface, avec
  les animations `spring`, `rowin` et `bloom` de `palette.html`. Glisser la poignée vers le bas la ferme.
- **Pages d'erreur** : même gabarit, mise en page verticale sous 600 px de large.
- **Retour Android** (`OnBackPressedDispatcher`, retour prédictif Android 14+), dans cet ordre :
  fermer la feuille, le tiroir ou la grille ; sinon page précédente ; sinon fermer l'onglet s'il a été
  ouvert par un autre onglet ; sinon passer l'app en arrière-plan.
- **Zones sûres** : affichage bord à bord (`WindowInsetsCompat`), encoches et barre de gestes
  transmises à l'interface en variables CSS (`--safe-top`, `--safe-bottom`).
- **Sombre/clair** : `prefers-color-scheme` suit le système ; réglage Thème (Système, Sombre, Clair).
- **Animations réduites** : le réglage système « Supprimer les animations »
  (`ANIMATOR_DURATION_SCALE = 0`) et le réglage `animations` de l'app donnent `body.still`, comme
  sur le bureau. Les animations natives (tiroir, glissement d'onglet) sont alors instantanées.

## 5. Architecture proposée

```
crates/android/          cdylib + rlib, « blackhole-android »
  src/lib.rs             cœur mobile pur (testé sur l'hôte) : état, commandes, réponses
  src/bridge.rs          protocole JSON : MobileCommand (interface) / PageEvent (pages)
  src/jni.rs             #[cfg(target_os = "android")] points d'entrée JNI (seul code unsafe)
  ui/mobile.html         coque d'interface : barre, tiroir, grille, palette (un seul document)
  ui/home.html           accueil mobile
  ui/settings.html       réglages mobiles
android/                 projet Gradle (Kotlin, AGP 8, minSdk 29, targetSdk 35)
  app/src/main/java/…/   MainActivity, TabHost, ShellView, Bridge, Downloads, KeyStore
  gradlew, build.gradle.kts, settings.gradle.kts
scripts/android-build.ps1   cargo ndk → jniLibs, puis gradlew assembleDebug
scripts/test-android.ps1    adb : install, scénario, captures
docs/android.md
```

### 5.1 Répartition des rôles

- **Rust (cœur)** : `Workspace`, onglets, espaces, accès rapide, `HistoryStore`, `Settings`,
  `PrivacyPolicy` et `BlockLog`, `omnibox`, `error_page`, `selection`. Persistance par
  `write_later` et `flush_writes` dans `filesDir`. API JNI minimale, en JSON :
  `init(dataDir, vaultKey)`, `command(json) -> json`, `should_block(url, top) -> bool` (appelé depuis
  `shouldInterceptRequest`, sur un thread réseau), `page_event(tab, json)`.
- **Kotlin (coque)** : cycle de vie, WebViews d'onglets (plafond de vues vivantes), gestes natifs
  (tiroir, glissement d'onglet), retour, zones sûres, téléchargements, Keystore, vignettes.
- **Interface (HTML/JS)** : une WebView d'interface **transparente au-dessus** des onglets. Quand rien
  n'est ouvert, elle ne capte que les zones qu'elle déclare : la barre, puis le tiroir ou la feuille
  quand ils sont ouverts. Kotlin laisse passer les autres touchers à l'onglet. On garde ainsi **un seul
  renderer d'interface** et des animations CSS fluides, sans redimensionner de vue native à chaque image.

### 5.2 Sécurité, mêmes règles que le desktop

- **Interface → shell** : `WebViewCompat.addWebMessageListener` limité à l'origine de l'interface
  (`https://appassets.androidplatform.net` via `WebViewAssetLoader`), jamais `addJavascriptInterface`.
  Les messages sont désérialisés en `MobileCommand`, une énumération fermée, l'équivalent de `UiCommand`.
- **Pages → shell** : un autre listener, sur les WebViews d'onglets seulement. Il n'accepte que
  `PageEvent` (lien, sélection, sonde d'erreur), considérés **non fiables**. Une page ne peut jamais
  émettre une `MobileCommand`. Le contournement d'une page d'erreur suit la même règle qu'au bureau :
  le bouton n'envoie que `ErrorMenu`, et seule l'entrée du menu de l'interface accorde l'exception
  (mémoire seulement, refus HSTS).
- **Navigation** : onglets en http(s) seulement (`shouldOverrideUrlLoading` refuse `intent:`,
  `file:`, `content:`, `javascript:`). Les liens `intent:`/`market:` ne s'ouvrent qu'après
  confirmation. La vue d'interface ne navigue jamais hors de ses assets.
- **Interface** : CSP stricte, données externes en `textContent` seulement. Aucune télémétrie.
- **Secrets** : la clé du coffre est enveloppée par l'Android Keystore et n'est jamais écrite en clair.
  `allowBackup=false` et règles d'extraction de données (pas de sauvegarde cloud des données de navigation).
- **WebView** : `allowFileAccess=false`, `allowContentAccess=false`, mixed content bloqué, Safe Browsing actif.
- **APK debug** signé par la clé de debug générée par Gradle. Aucune clé de release dans le dépôt
  (`*.jks`/`*.keystore` ajoutés au `.gitignore`).

### 5.3 Espaces et mode privé

Le WebView Android partage un seul jeu de cookies par processus. L'API **Profile** d'androidx.webkit
(`WebViewFeature.MULTI_PROFILE`, WebView ≥ 110, androidx.webkit ≥ 1.9) donne des profils séparés :
un par espace, plus un profil privé créé au lancement et supprimé à la fermeture des onglets privés.
C'est l'équivalent de `Store`. Si cette fonction est absente, ce qui est rare en 2026, on se replie
ainsi : un seul profil pour tous les espaces, et pour le mode privé, cookies et stockage effacés à la
fermeture du dernier onglet privé, avec un avertissement dans l'interface.

### 5.4 Pages dédiées ou variante CSS ? **Pages mobiles dédiées, avec le gabarit d'erreur partagé.**

Justification :

- Les pages desktop (`bar.html`, `index.html` sidebar, `palette.html`, `hotzone.html`) sont
  découpées par **vue native** et dialoguent par `UiCommand` avec un shell winit. Sur mobile, la barre,
  le tiroir, la grille et la palette doivent vivre dans **un seul document**, pour les animations
  croisées et un seul renderer. Une variante CSS ne suffirait pas : la structure DOM et le protocole
  diffèrent.
- Les ajouter en `@media` dans les pages desktop les alourdirait de code sans objet, et chaque
  retouche mobile pourrait faire régresser Windows ou macOS. Ce risque est inacceptable.
- **L'identité visuelle est préservée par le partage.** Les jetons (couleurs, dégradés), le disque
  d'accrétion, les étoiles et les `@keyframes` (`spring`, `rowin`, `bloom`, `pop`, `rise`, `swirl`)
  sont copiés depuis les pages desktop dans `crates/android/ui/`, avec une note de maintenance dans
  les deux fichiers de consignes.
- **Exception : `error.html` est partagé**, inclus par `include_str!("../../app/ui/error.html")`.
  C'est un gabarit autonome, chargé dans l'onglet sur les deux plateformes. Il suffit d'y ajouter un
  bloc `@media (max-width: 600px)` neutre au-dessus de 600 px, que le harnais desktop vérifie.
  `views::error_page` sera déplacé dans une petite fonction pure partagée, par exemple
  `domain::error_page::render`, si cela reste simple.

## 6. Plan de l'étape 2 (après validation)

1. Installer les outils validés au § 2.
2. Adapter les crates partagés : `cfg(android)` pour la clé du coffre et `keyring`, `Failure::Android`
   avec ses tests. Vérifier clippy Windows, `--features devtools`, la compilation croisée
   `aarch64-apple-darwin` et `cargo test --workspace --locked`.
3. Créer `crates/android` : cœur, protocole et tests unitaires sur l'hôte. Puis `cargo ndk build`
   pour arm64-v8a et x86_64.
4. Projet Gradle Kotlin et APK debug.
5. Pages mobiles avec leurs animations, puis captures de chaque écran dans l'émulateur, inspectées
   (état intermédiaire des animations et mode réduit compris).
6. Script `scripts/test-android.ps1` : adb install, scénario par intents ou messages de debug,
   `adb exec-out screencap`, contrôles.
7. Rédiger `docs/android.md` et mettre à jour `AGENTS.md` et `.github/copilot-instructions.md`.
8. Proposer un job CI Android pour `release.yml`, sans l'ajouter : `ubuntu-latest`,
   `android-actions/setup-android`, NDK r27c, `cargo ndk` et `gradlew assembleDebug`, puis APK en
   artefact.

## 7. Questions à trancher

1. Installer les outils (≈ 5,5 Go), et l'émulateur (≈ 8–11 Go de plus) ? Où : `E:\Android` ?
   Ou plutôt un téléphone en USB ?
2. Architecture « coque Kotlin + cœur Rust par JNI, sans wry Android » : OK ? Elle introduit du
   Kotlin dans le dépôt.
3. Périmètre v1 du § 3 : OK ? Faut-il y ajouter l'authentification HTTP ou « Continuer sur l'autre
   appareil » ?
4. minSdk 29 (Android 10, ~95 % du parc) : OK ?
5. Identifiant d'application : `app.blackhole.browser` ? Une autre préférence ?
