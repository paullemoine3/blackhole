# Spécificités macOS

macOS reste une plateforme de premier rang, en parallèle de Windows.

- **Extensions natives (15.4+)** via `WKWebExtensionController` (crate `macos-webext`). uBlock Origin Lite est la version « Lite » (MV3, `declarativeNetRequest`) ; sa page d'options n'est pas exposée.
- **Popups d'extensions** : le bouton (icône + badge) de la barre du haut ouvre le popup natif (`WKWebExtensionAction.popupPopover`). L'onglet actif est exposé aux extensions (`WKWebExtensionTab` / `Window`).
- **Extensions des apps installées** : Paramètres → Extensions → « Trouvées dans vos apps » liste les extensions Safari de `/Applications` et `~/Applications` (`*.app/Contents/PlugIns/*.appex`), activables via `app_extensions.json`.
- **Compatibilité Bitwarden / Dark Reader** :
  - API `privacy` émulée (`privacy_shim.js`) : WebKit ne l'a pas, et sans elle le réglage « gestionnaire de mots de passe par défaut » de Bitwarden se décoche seul.
  - Extensions chargées avec le token d'agent `Firefox/128.0` (requis par Bitwarden, build Firefox).
  - Cadres d'extension (menu d'autofill, barre de notification) autorisés dans les pages.
  - Extensions à page d'arrière-plan persistante (Bitwarden) servies sous `safari-web-extension://<id>` ; les extensions à service worker ne reçoivent pas ce schéma (sinon Dark Reader reste sur son style de secours).
- **Navigateur par défaut** : `scripts/package-macos.sh [--install]` produit `dist/BlackHole.app` (http/https) et l'installe dans `/Applications` ; ensuite, action « Make BlackHole the default browser ».
- **Gestes trackpad** : deux doigts pour page précédente/suivante et changement d'espace sur la barre latérale.
- **Rail de 84 px** : couvre les boutons système fermer/réduire/agrandir.
- **Vidéo** : WKWebView décode déjà sur GPU. Annoncer `Safari/605.1.15` dans le user-agent rendait YouTube très lent : écarté.
- **Mise à jour** : remplacement de `BlackHole.app` puis relance ; désactivée hors bundle `.app`. Bundle signé ad hoc (pas de notarisation).
- **Autoremplissage natif** : au focus d'un champ identifiant/mot de passe d'une page https (ou localhost), BlackHole propose les identifiants du coffre. Désactivé tant que l'extension Bitwarden est active ; le mot de passe n'est injecté qu'après un clic.
- **Tests** : `scripts/test-webext.sh` (installe uBOL, vérifie le blocage actif/inactif, retrait) et harnais `devtools` avec captures CoreGraphics (`/tmp/bh_script.txt` → `/tmp/bh_shots/`).
