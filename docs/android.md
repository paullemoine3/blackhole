# BlackHole pour Android

Version allégée du navigateur, à l'identité visuelle du bureau : disque d'accrétion, palette
en ressort, pages d'erreur sci-fi. Elle reprend les règles du bureau (onglets façon Arc,
espaces, accès rapide, historique, Shields, pages d'erreur) en réutilisant tels quels
`crates/domain`, `crates/application` et `crates/infrastructure`.

## Architecture

```
android/                 projet Gradle (Kotlin, AGP 8.7, minSdk 29, targetSdk 35)
  app/src/main/java/app/blackhole/browser/
    MainActivity.kt      coque : vues d'onglets sous une vue d'interface transparente, effets,
                         retour, zones sûres, téléchargements, profils WebView
    TabView.kt           WebView d'un onglet (Shields, navigations filtrées, erreurs, TLS)
    Native.kt            JNI vers le cœur Rust
crates/android/          « blackhole-android » (cdylib + rlib)
  src/core.rs            état : BrowserService du bureau, onglets privés, vues vivantes, erreurs
  src/bridge.rs          protocole JSON : MobileCommand / PageEvent / NativeEvent / Effect
  src/live.rs            vues vivantes (au plus MAX_LIVE = 4, LRU)
  src/jni.rs             points d'entrée JNI (Android seulement, seul module à `#[no_mangle]`)
  ui/mobile.html         l'interface : barre du bas, tiroir, grille d'onglets, palette, réglages
  ui/page.js             script de début de page des onglets (pont `__bhipc` de la page d'erreur)
```

- **Pas de wry sur Android.** wry 0.57 n'y gère qu'une WebView par Activity, il passe par tao
  (et non winit), et les gestionnaires ajoutés par BlackHole (échecs de chargement, certificats,
  authentification HTTP) n'ont pas d'implémentation Android. La coque Kotlin pilote directement
  les `android.webkit.WebView`. `vendor/wry` et `crates/app` ne sont pas touchés.
- **Une seule vue d'interface** (`mobile.html`, servie par `WebViewAssetLoader` depuis
  `https://appassets.androidplatform.net/assets/`) au-dessus des onglets : un seul renderer pour
  l'interface, animations CSS. Elle déclare sa surface (`surface`) : barre du bas seulement, ou
  tout l'écran pendant que l'accueil, le tiroir, la grille ou une feuille sont ouverts. Les autres
  touchers vont à l'onglet (`ShellView.dispatchTouchEvent`).
- **Pages partagées avec le bureau** : `crates/app/ui/error.html` (gabarit des pages d'erreur,
  inclus par `include_str!`) et `crates/app/ui/shields.js`. Les pages d'interface mobiles sont
  dédiées : la structure (une seule page pour barre, tiroir, grille et palette) et le protocole
  diffèrent du bureau, et des variantes CSS dans les pages du bureau risqueraient de les faire
  régresser. Jetons de couleur et `@keyframes` (`spring`, `rowin`, `bloom`, `pop`, `rise`, `swirl`,
  splash de `home.html`) sont repris : **toucher l'un sans l'autre fait diverger les deux apps**.

### Protocole et sécurité

- Interface → shell : `WebViewCompat.addWebMessageListener(shell, "bh", {origine des assets})`,
  cadre principal seulement ; messages `MobileCommand` (énumération fermée, l'équivalent de
  `UiCommand`). Quelques messages restent dans la coque (`surface`, `thumbs`, `peek`,
  `peek_end`, `copy`, `share`, `open_downloads`). Jamais `addJavascriptInterface`.
- Pages → shell : listener `bhPage` sur les onglets, ouvert à toutes les origines (les pages
  sont **non fiables**) et lu au cadre principal seulement ; seul `PageEvent::ErrorMenu` existe.
  Il n'ouvre le menu de la coque que si l'onglet affiché montre notre page d'erreur ; seule
  l'entrée du menu (`MobileCommand::ErrorProceed`) accorde l'exception (mémoire seulement,
  refus HSTS via `Bypasses`).
- Navigations : `shouldOverrideUrlLoading` (cadre principal) → `NativeEvent::Navigating` →
  le cœur autorise, réécrit (https automatique, paramètres de pistage) ou annule. Schémas
  `intent:`, `market:`, `mailto:`, `tel:`, `sms:`, `geo:` : confirmation dans l'interface, puis
  `Intent` limité aux activités `BROWSABLE` sans composant ni sélecteur. `file:`, `content:`,
  `javascript:` sont refusés.
- Shields : `shouldInterceptRequest` → `Native.shouldBlock` (thread réseau, `Blocker` partagé,
  décompte `BlockLog` par site pour le rapport) ; `shields.js` injecté au début de chaque page
  (`addDocumentStartJavaScript`).
- Certificats : `onReceivedSslError` → `proceed()` uniquement pour un hôte accepté par le menu.
- WebView : `allowFileAccess`/`allowContentAccess` faux, contenu mixte interdit, Safe Browsing,
  `setSupportMultipleWindows(false)` (un `target=_blank` s'ouvre dans l'onglet).
- Données : `allowBackup=false`, règles d'extraction excluant tout ; profil dans
  `filesDir/profile` (mêmes fichiers JSON que le bureau). Aucun secret : le portefeuille et le
  coffre (`keyring`, sans magasin persistant sur Android) ne sont pas utilisés.
- Espaces et privé : un profil WebView (`androidx.webkit` `MULTI_PROFILE`) par espace
  (`Default` pour le premier, `space-N`), `private` pour les onglets privés, vidé puis supprimé
  à la fermeture du dernier. Sans `MULTI_PROFILE` (WebView < 110), un seul profil : les Réglages
  le signalent.
- Téléchargements : https seulement (règle du cœur), `DownloadManager` vers
  `Download/BlackHole/`, cookies du profil (jamais ceux du privé), carte de progression dans le
  tiroir.

## Fonctionnalités (v1)

Onglets (grille, balayer pour fermer, Undo 15 s, glisser la barre pour changer d'onglet),
barre d'adresse en bas et palette (recherche avec le moteur choisi, suggestions d'onglets,
d'accès rapide et d'historique), accès rapide (mis en veille, jamais retiré, par la fermeture),
historique (recherche, oubli, effacement en deux temps), onglets privés, Shields et rapport de
confidentialité (note A–E), téléchargements, réglages (moteur, thème Système/Sombre/Clair,
couleur d'espace, animations, Shields, effacement des données), espaces, pages d'erreur,
geste retour (ferme la surface ouverte, puis page précédente, puis accueil, puis quitte),
zones sûres (bord à bord), animations réduites (réglage système « Supprimer les animations »
ou réglage de l'app → `body.still`, animations natives de changement d'onglet coupées).

À plus tard : authentification HTTP, mots de passe, pages
d'erreur des statuts 401/403/5xx sans corps, masquage de la barre au défilement, ouverture du
tiroir par le bord de l'écran (en conflit avec le geste retour). Exclus : Easel, extensions,
outils de développement, PiP, split view, calendrier, comparateur, veille de zone.

## Appareils appairés : « Continuer sur » et synchro des onglets

Même service que le bureau (`infrastructure/handoff.rs` : UDP 47807 sur le réseau local,
ChaCha20-Poly1305, appairage par code) : un téléphone s'appaire avec un PC ou un Mac et inversement.
- **Clé** : `devices.bin` est scellé par une clé de 32 octets gardée chiffrée (AES-GCM) sous une clé
  de l'Android Keystore (`KeyVault.kt`, préférences privées exclues des sauvegardes) ; passée en
  hexadécimal à `Native.init`, jamais écrite en clair. `keyring` n'a pas de magasin sur Android.
- **Réseau** : `MulticastLock` pris pendant que l'app est au premier plan (sans lui le Wi-Fi filtre
  les diffusions), permission `CHANGE_WIFI_MULTICAST_STATE`. Le service n'écoute qu'avec un appareil
  appairé ou un appairage en cours, comme sur le bureau. `NativeEvent::Tick` (toutes les 300 ms au
  premier plan) lit ce que le réseau apporte.
- **Synchro** (`domain/sync.rs`, `infrastructure/sync.rs`, `crates/android/src/devices.rs`, côté bureau
  `app/src/handoff.rs`) : accès rapide, onglets ouverts et espaces (nom, couleur). Un registre
  (`sync.json`) garde par onglet (espace par nom + adresse) la date de sa dernière modification et
  l'appareil auteur ; une fermeture laisse une pierre tombale (30 jours). La fusion garde la
  modification la plus récente. Mode par appareil (`ask` par défaut, `auto`, `off`, dans `devices.bin`) :
  un appareil qui réapparaît reçoit le registre ; `ask` propose (Merge / Always / Later), `auto`
  fusionne et reçoit le registre 3 s après chaque changement local ; la réponse à une fusion acceptée
  s'applique sans seconde question. Le registre voyage en fragments de 6 Ko (100 au plus), acquittés.
  Jamais synchronisés : onglets privés, dossiers, historique, titres seuls.
- **Test** : `cargo test -p blackhole-android` (deux cœurs appairés en boucle locale) et
  `scripts\test-sync.ps1` (BlackHole Windows en build `devtools` ↔ émulateur : l'émulateur ne voit
  pas les diffusions du réseau, `adb emu redir add udp:47809:47807` et l'extra debug
  `bh_peer=10.0.2.2:47808` relient les deux ; le bureau lit `BH_HANDOFF_PORT`/`BH_HANDOFF_PEER_PORT`
  en build `devtools`). Échange réel sur un Wi-Fi entre un téléphone et un PC non testé ici.

## Construire

Outils (installés pour ce poste dans `E:\Android`) : JDK 17, Android SDK (`platform-tools`,
`platforms;android-35`, `build-tools;35.0.0`), NDK `27.3.13750724`, cibles Rust
`aarch64-linux-android` et `x86_64-linux-android`, `cargo install cargo-ndk`.

```powershell
$env:JAVA_HOME = 'E:\Android\jdk17'; $env:ANDROID_HOME = 'E:\Android\sdk'
# android\local.properties (non versionné) : sdk.dir=E\:/Android/sdk
cd android; .\gradlew.bat assembleDebug
```

`assembleDebug` lance d'abord `cargo ndk` (tâche `cargoNdk`, profil `dev` ; `-PrustRelease`
pour le profil release) vers `android/app/build/rustJniLibs`, puis empaquette
`android/app/build/outputs/apk/debug/app-debug.apk` (arm64-v8a et x86_64). Les pages de
`crates/android/ui` sont les assets de l'APK : pas de copie à faire.

Derrière un antivirus qui inspecte le HTTPS, Java ne reconnaît pas son certificat : ajouter
`$env:GRADLE_OPTS = '-Djavax.net.ssl.trustStoreType=Windows-ROOT'` et le même `-D…` à la
commande `gradlew` (et `SDKMANAGER_OPTS` pour `sdkmanager`).

Vérifier le cœur seul : `cargo test -p blackhole-android` (hôte) et
`cargo ndk -t arm64-v8a -P 29 clippy -p blackhole-android --all-targets` (cible Android).

**Signature** : en local, la clé de debug générée par Gradle (`~/.android/debug.keystore`).
Aucune clé de release dans le dépôt (`*.jks`, `*.keystore` ignorés) : voir « Release (CI) ».

## Installer et tester

```powershell
& E:\Android\sdk\platform-tools\adb.exe install -r android\app\build\outputs\apk\debug\app-debug.apk
scripts\test-android.ps1 -Boot      # démarre l'AVD bh_test si aucun appareil n'est branché
```

`test-android.ps1` réinstalle l'APK, efface les données de l'app, pilote l'interface par des
extras d'intent réservés aux builds debug (`bh_cmd64` : un message d'interface en base64,
`bh_eval64` : du JavaScript dans la page d'interface), lit les résultats `bheval:nom=valeur`
dans logcat (contrôles **nommés**, pas indexés) et range les captures dans
`%TEMP%\bh_android_shots`. Il couvre l'accueil, l'ouverture d'adresses, la palette et les
suggestions, le tiroir (état intermédiaire compris), la grille, le rapport Shields, le privé,
une page d'erreur, le refus d'un traqueur, les réglages (thème clair), le mode animations
coupées, l'historique, le retour système, Undo, et la sortie par le retour. Réseau requis.

AVD de test : `avdmanager create avd -n bh_test -k "system-images;android-35;google_apis;x86_64" -d pixel_7`
(`ANDROID_AVD_HOME=E:\Android\avd`), accélération WHPX.

Aperçu de l'interface sans appareil : ouvrir `crates/android/ui/mobile.html` dans un navigateur
(données de démonstration quand `window.bh` est absent).

## Release (CI)

Le job `android` de `.github/workflows/release.yml` (après `test`, comme Windows et macOS)
installe le NDK, passe clippy pour la cible Android puis construit l'APK avec le cœur Rust en
profil release (`-PrustRelease`). Lors d'une publication, `BlackHole-android.apk` rejoint les
fichiers de la release et `SHA256SUMS.txt`.

Signature : la clé de release n'est jamais dans le dépôt. Secrets GitHub à créer :
`ANDROID_KEYSTORE_BASE64` (le keystore en base64), `ANDROID_KEYSTORE_PASSWORD`,
`ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD`. Le job décode le keystore dans le dossier temporaire
du runner, le passe à Gradle par `BH_ANDROID_KEYSTORE` (+ `BH_ANDROID_KEYSTORE_PASSWORD`,
`BH_ANDROID_KEY_ALIAS`, `BH_ANDROID_KEY_PASSWORD`) et le supprime. Sans ces secrets, il publie
`BlackHole-android-debug.apk` (clé de debug de Gradle) avec un avertissement : un APK signé
autrement ne pourra pas le mettre à jour (désinstaller d'abord).

Créer une clé (à garder hors du dépôt) :

```bash
keytool -genkeypair -v -keystore blackhole-release.keystore -alias blackhole -keyalg RSA -keysize 4096 -validity 10000
base64 -w0 blackhole-release.keystore   # valeur de ANDROID_KEYSTORE_BASE64
```

La version de l'APK suit celle du workspace (`Cargo.toml`) : `versionName` identique,
`versionCode` = majeure × 10000 + mineure × 100 + correctif (0.8.0 → 800).
