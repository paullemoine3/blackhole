# Design system BlackHole

Référence visuelle de l’interface du shell (`crates/app/ui/`) : tokens, composants et règles d’usage. Version publiée, avec les aperçus rendus : [BlackHole Design System](https://claude.ai/artifact/QS8rSaxn5u22p1tojFzvHo) (artifact privé, à partager depuis son menu Share).

Ce dossier est de la documentation : rien n’est embarqué dans l’app. Les variables utilisées par le code (`--bh-*`) sont injectées par `ui/theme.js`.

BlackHole est un navigateur sombre : un cadre presque noir (`bg-shell`, #100f17) teinté par la couleur de l’**espace** actif, et des pages posées dessus comme des cartes. Ce système part de l’interface réelle du shell (`crates/app/ui/`), unifiée : chaque rôle a une seule valeur. Les anciens noms restent utilisables, marqués « Déprécié » ; `unification.md` donne la table de migration.

## Principes

- **Une seule couleur vive à la fois : celle de l’espace.** Tout ce qui est actif, sélectionné ou primaire prend `accent`, sur toutes les pages (accueil, palette et Bibliothèque comprises). Le reste est fait de blanc translucide sur `bg-shell`. Seules exceptions : la teinte des pages d’erreur, les couleurs de carte Easel et le rouge des actions destructrices.
- **Sombre uniquement.** Il n’y a pas de thème clair. Les « thèmes » de ce système sont les huit teintes d’espace : choisir un thème montre l’interface dans cet espace.
- **Hiérarchie par opacité.** Le texte est toujours `text-primary` (#eceaf5) ; les niveaux secondaires baissent son opacité (`opacity-muted`, `opacity-secondary`…) au lieu d’utiliser des gris.
- **Le trou noir est la marque.** Disque noir, accrétion orange (`accretion-hot`, `accretion-warm`), anneau doré et violet (`accretion-ring`, `space-violet`). Il apparaît dans le logo, le splash, l’ouverture de la palette et les pages d’erreur.
- **Ça bouge en ressort, et ça s’arrête sur demande.** Presque toutes les transitions ont un léger dépassement ; toutes sont coupées par le réglage « animations » et par `prefers-reduced-motion`.

## Couleur

### La teinte de l’espace
Le JS de chaque page pose ces variables depuis `state.color` (index.html:861, bar.html:136, settings.html:328, easel.html:252, palette.html:490) :

| Variable du code | Token | Calcul |
| --- | --- | --- |
| `--accent` | `accent` | la couleur de l’espace |
| `--glow` | `accent-glow` | rgba(couleur, .5) |
| `--glow2` | `accent-glow-soft` | rgba(couleur, .22) |
| `--tint1` / `--tint` | `tint-top` | mix(couleur, 30 %) vers `ink-850` |
| `--tint2` | `tint-bottom` | mix(couleur, 24 %) vers `ink-850` |
| `--sel` | `accent-selection` | rgba(couleur, .85) |

Les huit couleurs d’espace sont `space-violet`, `space-pink`, `space-green`, `space-amber`, `space-blue`, `space-red`, `space-teal`, `space-grey`. Un nouvel espace prend la suivante parmi les cinq premières (SPACE_COLORS, main.rs:121). Le changement d’espace fond toutes ces variables en `duration-tint` (.45s, enregistrées par `@property`).

- Utilisez `accent` pour : bouton primaire, ligne sélectionnée, focus d’un champ, interrupteur allumé, barre de progression, badge.
- Utilisez `accent-wash` pour l’élément actif d’une navigation, `accent-selection` pour une ligne sélectionnée ou un item de menu survolé.
- Peignez le fond du shell avec la recette de la barre latérale : `accent-glow` en haut, `accent-glow-soft` en bas à droite, dégradé `tint-top` → `tint-bottom`.

### Neutres
- Fonds : `bg-shell` (shell, = SHELL_BG de views.rs), `bg-page` (pages internes : accueil, erreurs, Bibliothèque ; plus sombre exprès), `bg-splash`, `bg-board` (Easel).
- Texte : `text-primary`, et `text-secondary` (60 %) quand l’opacité ne peut pas servir. Placeholder : `text-placeholder` (50 %).
- Voile sous une surface modale : `scrim` (palette, aperçu).
- Surfaces flottantes : `surface-panel` (palette), `surface-menu`, `surface-toast`.
- Remplissages : l’échelle `fill-sunken` (.02) → `fill-glint` (.22) en blanc translucide. Repos des contrôles `fill-control`, survol des rangées `fill-hover-strong`, sélection `fill-selected`.
- Bordures : `border-hairline` (.06) → `border-strong` (.14) ; champs `border-field`.

### États
- Destructeur : `danger-fill` pour l’état armé et le survol d’une action irréversible, partout (Easel compris) ; `danger-solid` pour un échec ; `danger-text` pour un libellé, `danger-text-soft` pour un libellé clair.
- Non sécurisé : `danger-text-soft` sur `insecure-fill`, bordure `insecure-border`.
- Succès : `success` pour un point ou un remplissage, `success-text` pour un texte ou un éclair.

### Contraste
- `text-primary` sur `bg-shell` : 16:1. Sur `tint-top`, il reste au-dessus de 8:1 dans tous les espaces.
- **`text-on-accent` (blanc) sur `accent` n’atteint 4,5:1 dans aucun espace** : violet 4,35, rouge 3,70, gris 3,39, rose 2,94, bleu 2,74, sarcelle 2,42, vert 1,90, ambre 1,83. Les boutons primaires et lignes sélectionnées sont donc peu lisibles en vert et en ambre. Correction possible, non appliquée (décision produit) : un `text-on-accent` par espace, #100f17 sur les teintes claires (vert, ambre, sarcelle, bleu, rose).

## Typographie

Police système, sans fichier embarqué : `-apple-system, "Segoe UI", system-ui, sans-serif` (`font-sans`) ; `ui-monospace, Menlo, Consolas, monospace` (`font-mono`) pour le code, les touches et les hôtes.

- Shell (barre latérale, barre du haut, Bibliothèque) : `body-shell` 13/1.4. Pages (Réglages, Easel, accueil, palette, erreurs) : `body-page` 14/1.45.
- Une seule pile, `font-sans`, partout.
- Titres de section : `section-label`, 11px en capitales, interlettrage .08em partout, `opacity-section`.
- Titres de page : `title-settings` 22px (Réglages, Bibliothèque), `title-error` 26px, `title-home` 30px ; tous en 600.
- Graisses : 400, 500 (titres de ligne), 600 (noms, boutons primaires, titres), 700 (initiales, badges).
- Chiffres alignés (`font-variant-numeric: tabular-nums`) pour les pourcentages, le zoom et les heures.

## Espacement, rayons, tailles

- Espacement au pas de 2px : 2 4 6 8 10 12 14 16 18 20 24 28 36 40 56 (`space-N`). `space-1` sert aux marges de 1px. Les valeurs impaires du code (3, 5, 7, 9, 22) sont hors échelle.
- Rayons : `radius-bar` 4 (barres) · `radius-chip` 6 (puces, kbd, petits boutons) · `radius-control` 8 (tout ce qui se clique : boutons, onglets, champs, options, items de menu) · `radius-card-sm` 10 (cartes du shell, toasts, omnibox) · `radius-card` 12 (cartes Easel, tuiles, puces d’espace) · `radius-panel` 14 (cartes de section, carte-page, menu) · `radius-dialog` 18 (palette) · `radius-pill` · `radius-round`. Un élément imbriqué prend le rayon du parent moins sa marge.
- Opacités : .35 glisser · .4 désactivé · .5 section · .6 atténué · .7 secondaire · .85 doux.
- Dimensions fixes : `size-topbar` 38px (barre du haut et en-tête de Réglages/Easel), `size-sidebar` 240px, `size-icon-button` 32px, `size-bar-button` 28px, `size-settings-nav` 210px (toute navigation latérale), `size-traffic-lights` 84px de retrait sur macOS.

## Élévation et lueurs

- La page web est une carte : `shadow-page-card` (rayon `radius-panel`, contour blanc .07) au-dessus du cadre teinté.
- Flottants : `shadow-palette` (avec un halo d’accent), `shadow-menu`, `shadow-ghost`.
- Les cartes de la barre latérale n’ont pas d’ombre portée, seulement un contour `shadow-ring`.
- Les lueurs prennent la couleur de l’espace : `shadow-pinned-active`, `shadow-space-active`, `shadow-switch-on`, `shadow-meet-live`, `shadow-omni-hover`.

## Calques

`z-under` (-1, ombre de la carte-page) · `z-wait` (9) · `z-splash` (10) · `z-ghost-out` (30) · `z-dropcard` (40) · `z-ghost` (50). Les vues natives (barre latérale, barre du haut, page, palette) sont des WebViews séparées : leur ordre est géré par Rust, pas par z-index.

## Mouvement

- Survol et couleur : `duration-hover` (.18s) `ease-standard`. Pression : `duration-fast` (.15s) `ease-press`, scale .975 (rangées), .94 (puces d’espace), .88 (boutons de la barre du haut).
- Apparitions : cartes en `dl-in` (.2s, +6px), toast en `ease-pop`, menu en `ease-pop`, palette et changement d’espace en `duration-panel` (.4s) + `ease-glide`. Trois ressorts seulement : `ease-press` (pression, soulèvement), `ease-pop` (apparition), `ease-glide` (entrée sans rebond).
- Teinte : `duration-tint` (.45s) sur `--accent`, `--glow`, `--glow2`, `--tint1`, `--tint2`.
- Ambiance : logo qui respire (`duration-breathe`), anneau en orbite (6s), splash (`duration-splash`).

**Couper les animations.** Le réglage « Black hole animations » envoie `animations: false` ; la page pose alors une classe qui annule toute animation et transition. Le système d’exploitation peut aussi demander `prefers-reduced-motion: reduce`. Toute nouvelle page interne doit gérer les deux :

```css
body.noanim *, body.noanim *::before, body.noanim *::after { animation: none !important; transition: none !important; }
@media (prefers-reduced-motion: reduce) { *, *::before, *::after { animation: none !important; transition: none !important; } }
```

Aujourd’hui la barre latérale, la barre du haut et l’accueil gèrent les deux ; les autres pages non (voir Incohérences).

## Iconographie

- Toutes les icônes du shell sont des SVG tirés d’une seule table, `ui/icons.js`, et s’insèrent avec `bhIcon(nom, taille)`. Pas de police d’icônes ni d’emoji pour les commandes. Les glyphes ⚙ 🗂 🛡 ⟳ ⌂ ← → ✕ ⋯ ◧ ⏸ ▶ 🔇 🔊 🎤 📹 ⧉ 📁 📷 🎨 ▸ ▾ sont remplacés par leur SVG.
- Dessin : `viewBox="0 0 16 16"`, `fill: none`, `stroke: currentColor`, trait `icon-stroke` (1.5px) non mis à l’échelle (`vector-effect: non-scaling-stroke`), extrémités et jointures rondes. La couleur vient toujours du texte parent.
- Échelle (famille `size`, variables CSS `--bh-<nom>`) :

| Token | Taille | Usage |
| --- | --- | --- |
| `icon-xs` | 12px | Indicateurs en ligne : cadenas, chevrons, petite croix |
| `icon-sm` | 16px | Par défaut : menus, boutons, barre du haut, navigation (Réglages, Easel, Bibliothèque), actions de rangée |
| `icon-md` | 20px | Icône de tête d’une section ou d’une carte, grande loupe de la palette |
| `icon-lg` | 24px | États vides, vignettes sans image |

- Une action de rangée (✕, ⋯, partage) offre au moins `size-hit-min` (24px) de zone de clic, même si l’icône fait `icon-xs`.
- Les boutons de fenêtre Windows gardent leurs glyphes système de 10px au trait de 1px, qui suivent la convention du système.
- Avant cette échelle, le code utilisait sept tailles (10, 11, 12, 15, 16, 17, 18px) et quatre traits (1.4, 1.5, 1.6, 1.8) pour des icônes de 16px.
- Les sites sont représentés par leur favicon, sinon par leur initiale blanche 700 sur une teinte calculée depuis l’hôte (`hsl(h, 60%, 50%)`, la même formule sur toutes les pages).
- Les espaces portent un emoji ou une initiale.
- Raccourcis : toujours via `bhKeys()` (keys.js) : « ⌘⇧E » sur macOS, « Ctrl+Shift+E » ailleurs.

## Contenu

- Le shell parle anglais, en phrases courtes et à l’impératif sur les actions (« Pin tab », « Click again to clear »). Les pages d’erreur parlent français.
- Les libellés disent ce qui se passe : « Continuer en http (non sécurisé) », pas « Ignorer ».
- Les actions destructrices ne demandent pas de confirmation par dialogue : elles s’arment au premier clic.

## Fichiers

- `tokens.json` : les tokens, au format de l’artifact Design System. `tokens.css` : les mêmes en variables CSS, sans préfixe, avec un bloc `[data-theme="<espace>"]` par teinte.
- `blackhole.tokens.json` : export au format W3C Design Tokens (DTCG). Les valeurs par espace sont dans `$extensions["blackhole.modes"]`.
- `components.css` : les composants rendus avec ces tokens (classes `bh-*`), utilisés par les aperçus de l’artifact.
- `components/<Composant>.md` : le guide de chaque composant.
- `unification.md` : ce qui a été fusionné, la table ancien → nouveau, et ce qui reste distinct exprès.
- `incoherences.md` : l’état du code avant l’unification, et ce qui reste à faire dans l’app.
