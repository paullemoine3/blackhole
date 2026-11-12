# Animations du logo BlackHole — proposition

Document de travail à valider avant tout code, sur le modèle de `docs/android/proposition.md`.
Deux pages de maquettes animées, à ouvrir dans un navigateur :

- [`maquettes.html`](maquettes.html) — série 1, autour du logo actuel (disque rond). En réserve.
- [`maquettes-lentille.html`](maquettes-lentille.html) — série 2, inspirée de la visualisation
  d'un vrai trou noir (disque vu par la tranche, face arrière courbée par la gravité), dans nos
  couleurs.

Chaque page propose les bascules « Animations réduites », « Thème clair », « Format téléphone »
(splashs) et « Ralenti × 0,25 » pour voir les états intermédiaires. Rien n'est intégré dans
l'app : `crates/app/ui/home.html` et `crates/android/ui/mobile.html` sont intacts.

## Règles communes à toutes les variantes

- **Animations réduites.** Logo : image fixe (état de repos de la variante). Splash : pas de
  splash, comme aujourd'hui (`if (s.animations) splash()` sur le bureau, `body.still #splash`
  sur Android). `body.noanim` / `body.still` et `prefers-reduced-motion` sont respectés.
- **Coût.** Seuls `transform` et `opacity` sont animés ; dégradés et masques sont peints une
  fois. Le splash actuel anime `letter-spacing` (une mise en page par image pendant 1,2 s) :
  aucune variante ne le fait.
- **Les deux pages restent en phase.** Toute variante retenue se pose dans `home.html` et
  `mobile.html` en même temps, avec le même balisage du logo (aujourd'hui `.logo` + `::after`
  sur le bureau, `.hole` > `.disc` + `.ring` sur Android).
- **Tokens.** Couleurs `accretion-hot`, `accretion-warm`, `accretion-ring`, `accent` ; ressorts
  `ease-glide`, `ease-pop`, `ease-press` ; `duration-splash` et `duration-breathe` à mettre à
  jour selon le choix. La série 2 demande un token de plus, `accretion-deep` (orange assombri).
- **Tests à l'intégration.** Un contrôle et une capture par plateforme (`scripts\test-ui.ps1`,
  `scripts\test-android.ps1`), état intermédiaire et mode animations coupées compris.

## Série 1 — autour du logo actuel

| | Variante | En bref | Calques animés | Changement |
|---|---|---|---|---|
| A0 | Actuel | Le disque respire, un anneau tourne | 2 | — |
| A1 | Disque incliné | La matière coule sur une ellipse, derrière puis devant l'ombre | 3 | Balisage du logo refait (4 enfants) |
| A2 | Comètes en orbite | Trois comètes sur trois orbites | 4 | 3 enfants ajoutés, logo 1,6 fois plus large |
| A3 | Tourbillon | Deux nappes de bras en sens contraires | 3 | Balisage refait, dessin du logo changé |
| A4 | Réactif | Calme au repos, avale ses étoiles au survol ou au toucher | 1 au repos, 9 en réaction | S'ajoute à une autre variante |

| | Splash | Durée | En bref | Changement |
|---|---|---|---|---|
| B0 | Actuel | 2,6 s bureau, 1,7 s Android | Disque, anneau, étoiles en spirale, nom qui se resserre | — |
| B1 | Effondrement | 1,95 s | Étoiles vers un point, éclair, naissance du trou, nom lettre par lettre | 2 enfants, nom découpé en lettres |
| B2 | Allumage et atterrissage | 1,75 s | L'anneau s'allume, puis le logo vole jusqu'à sa place sur la page | Le plus de travail : position mesurée par script, repli si Android s'ouvre sur un onglet |
| B3 | Horizon | 1,5 s | L'ombre grandit jusqu'à avaler la fenêtre | 1 enfant, plus d'étoiles |

## Série 2 — logo « lentille »

La forme : une bande devant l'ombre, une arche au-dessus (face arrière du disque, courbée),
une jupe qui raccorde l'arche à la bande, un arc en dessous, un anneau de photons. Chaque
partie est un cercle masqué ; seule la texture intérieure tourne.

| | Variante | En bref | Calques animés | Changement |
|---|---|---|---|---|
| A5 | Matière qui coule | Le plus proche du gif : stries qui glissent, côté gauche plus lumineux | 5 | Bloc à 7 enfants, boîte du logo agrandie, teinte de l'onglet privé à refaire |
| A6 | Version marque | Aplats or → violet, un reflet parcourt l'arche et la bande | 3 | Même bloc + 2 reflets ; le violet suit `--accent` |
| A7 | Bascule | A5 dont le point de vue oscille entre la tranche et le dessus | 9 | A5 + 4 images clés ; à mesurer sur Android |

| | Splash | Durée | En bref | Changement |
|---|---|---|---|---|
| B4 | Lever du disque | 1,95 s | La bande se trace, la face arrière se lève au-dessus | Logo lentille dans `.core`, étoiles retirées |
| B5 | Bascule | 2,1 s | Vu de dessus (logo actuel) puis par la tranche : l'arche se forme | Comme B4 |
| B6 | Approche | 1,8 s | Le trou arrive de loin, puis son ombre prend la fenêtre | Logo lentille + disque noir, étoiles vers l'extérieur |

Points d'attention de la série 2 :

- La forme est plus large que haute et son ombre plus petite : le logo doit passer d'environ
  46 px à 64 px sur le bureau et de 84 px à 120 px sur Android pour garder la même présence.
- Sous 60 px, les stries de A5 se perdent ; A6 reste net. Un mélange est possible : A6 sur la
  page d'accueil, texture de A5 dans le splash (c'est ce que montrent les maquettes B4 à B6).
- `color-mix()` et les masques CSS sont requis (WebView2, WKWebView 16.2+, WebView Android
  récents : à confirmer sur le parc Android visé, `minSdk` 29).
- Changer la forme du logo touche aussi l'icône de l'app et `docs/title.svg` si l'on veut une
  marque cohérente : hors de cette proposition.

## Constaté en passant, non corrigé

- `home.html` applique `animation: emerge` à `main` pendant le splash, mais `@keyframes emerge`
  n'existe pas dans la page : la page d'accueil n'a pas d'entrée animée sur le bureau.
- Le dégradé du logo diffère entre le bureau (sans violet, noir jusqu'à 42 %) et Android (avec
  violet, noir jusqu'à 34 %).

## Décision

Retenu et intégré dans `home.html` et `mobile.html` : logo **A4** (réactif), splash **B1**
(effondrement) avec la sortie « atterrissage » de **B2**. Sur Android, si l'app s'ouvre sur un
onglet, le splash s'efface simplement (pas de logo où atterrir). Les autres variantes restent
en réserve dans les maquettes.

## Questions d'origine

1. Série 1 ou série 2 (garder le disque rond, ou passer à la forme lentille) ?
2. Une variante de logo (A…) et une de splash (B…).
3. Durée visée du splash : la même sur le bureau et Android, ou plus courte sur Android comme
   aujourd'hui ?
