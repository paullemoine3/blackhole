# Unification

Ce que la relecture a fusionné, pour que chaque rôle ait une seule valeur. Aucun token n’a disparu : les anciens noms restent, marqués « Déprécié », et valent désormais la valeur retenue. Le code qui les utilise continue de fonctionner et prend la valeur unifiée.

## Ce qui reste distinct, exprès

- `bg-page` (#0c0b12) et `bg-shell` (#100f17) : la page est une carte plus sombre que le cadre teinté.
- Les couleurs de carte Easel (`easel-*`) : elles sont enregistrées dans les données (easel::COLORS) ; les changer demanderait une migration.
- Les cinq teintes des pages d’erreur (`error-*`) : elles portent le sens de l’erreur.
- Les notes claires de la Bibliothèque (`lib-note-*`) : elles imitent le papier.
- Les boutons de fenêtre Windows (`win-close`, glyphes 10px) : convention du système.
- `ease-grow` (puce d’espace qui s’élargit) et `ease-switch` (curseur) : mouvements propres à ces deux composants.

## Table de migration

| Famille | Ancien | Utiliser | Avant | Après | Effet |
| --- | --- | --- | --- | --- | --- |
| color | `ink-600` | `ink-550` | #2a2838 | #2b2a3a | Fantôme d’onglet : #2a2838 → #2b2a3a, écart invisible. ink-550 est gardé car c’est la couleur de carte Easel enregistrée. |
| color | `surface-toast-easel` | `surface-toast` | {ink-550} | {ink-700} | Toast d’Easel : #2b2a3a → #1b1a26, même toast que la barre latérale. |
| color | `danger-hover-easel` | `danger-fill` | {rose-600} | #ff5a5ad9 | Survol du bouton danger d’Easel : #c2455b → rgba(255,90,90,.85), comme toutes les actions destructrices. |
| color | `danger-deep` | `danger-solid` | {red-600} | {red-500} | Bouton de mise à jour en erreur : départ du dégradé #b4443c → #e5534b. |
| color | `media-off` | — | #ff5c5c66 | #ff5a5a66 | rgba(255,92,92,.4) → rgba(255,90,90,.4), même rouge que danger-fill. |
| color | `danger-text-soft` | — | #ffb3b3 | #ffb3bd | #ffb3b3 → #ffb3bd (fusion avec insecure-text). |
| color | `insecure-text` | `danger-text-soft` | #ffb3bd | #ffb3bd | Même valeur, nom unique. |
| color | `insecure-fill / insecure-border` | — | .18 / .45 sur le bouton risqué | .22 / .4 | Le bouton risqué des erreurs prend les valeurs de la puce « Not secure ». |
| color | `success-flash` | `success-text` | {green-200} | {green-300} | Éclair du compteur de bloqueurs : #8ff0b4 → #6ee7a8. |
| color | `text-placeholder` | — | #eceaf566 | #eceaf580 | Palette : .4 → .5, comme la barre d’adresse (meilleur contraste). |
| color | `text-placeholder-bar` | `text-placeholder` | #eceaf580 | #eceaf580 | Même valeur, nom unique. |
| color | `text-stats` | `text-secondary` | #ffffff8c | #eceaf599 | Statistiques de la barre latérale : blanc .55 → #eceaf5 .6. |
| color | `scrim` | — | #06050c75 | #06050c80 | Palette .46 et aperçu .55 → .5. |
| color | `scrim-peek` | `scrim` | #06050c8c | #06050c80 | Même voile pour la palette et l’aperçu. |
| color | `accretion-ring` | — | palette rgba(255,200,110,.85) | rgba(255,205,120,.85) | L’anneau de la palette prend l’or du logo. |
| color | `palette-glyph-fill` | `accent-glow-soft` | #7c5cff38 | #7c5cff38 (par espace) | Glyphes de la palette : violet fixe → accent de l’espace à 22 % ; le glyphe prend `accent`. |
| color | `violet-fixed-85` | `accent-selection` | #7c5cffd9 | #7c5cffd9 (par espace) | Bouton de la carte Nouveautés : suit l’espace. Sur les pages d’erreur, le bouton principal prend la teinte de la page (`error-*-accent` à 85 %). |
| color | `violet-fixed-wash` | `accent-glow-soft` | #7c5cff38 | #7c5cff38 (par espace) | Lueur de l’accueil et carte Nouveautés : suivent l’espace. |
| color | `violet-fixed-border` | `accent-border` | #7c5cff73 | #7c5cff73 (par espace) | Bordure de la carte Nouveautés : suit l’espace. |
| color | `lib-accent` | `accent` | #6a8cff | #7c5cff (par espace) | Bibliothèque : #6a8cff → accent de l’espace (le JS le passait déjà quand il était connu). |
| color | `lib-bg` | `bg-page` | #16171c | {ink-900} | Bibliothèque : #16171c → #0c0b12, comme l’accueil (page interne). |
| color | `lib-fg` | `text-primary` | #e8e9ee | {fg-100} | Bibliothèque : #e8e9ee → #eceaf5. |
| color | `lib-dim` | `text-secondary` | #8b8e99 | #eceaf599 | Bibliothèque : #8b8e99 → #eceaf5 à 60 %. |
| color | `lib-card` | `fill-raised` | #22242b | #ffffff17 | Bibliothèque : #22242b → blanc 9 % (tuiles, champ, colonnes). |
| shadow | `shadow-item-hover` | — | 0 10px 24px -16px #7c5cffb3 | 0 10px 24px -16px accent .7 | Suit l’espace au lieu du violet fixe. |
| shadow | `shadow-lib-lift` | `shadow-tile-lift` | 0 12px 26px -14px #000000 | 0 10px 22px -10px #000000b3 | Bibliothèque : même soulèvement que l’accueil. |
| shadow | `shadow-ring-update` | `shadow-ring` | 0 0 0 1px #ffffff24 | 0 0 0 1px #ffffff1a | Contour du bouton de mise à jour : blanc .14 → .1, comme les autres cartes de la barre latérale. |
| radius | `radius-hair` | `radius-bar` | 3px | 4px | Barres de progression 3 → 4px (invisible sur 3px de haut). |
| radius | `radius-key` | `radius-chip` | 5px | 6px | kbd, code, petits boutons d’onglet 5 → 6px. |
| radius | `radius-chip-button` | `radius-chip` | 7px | 6px | Boutons des toasts, segments, zoom, icônes de palette 7 → 6px (rayon intérieur d’un conteneur de 8px avec 2px de marge). |
| radius | `radius-row` | `radius-control` | 9px | 8px | Onglets, dossiers, boutons, champs, options, navigation 9 → 8px : un seul rayon pour tout ce qui se clique. |
| radius | `radius-space` | `radius-card` | 11px | 12px | Puces d’espace 11 → 12px. |
| radius | `radius-menu` | `radius-panel` | 13px | 14px | Menu contextuel 13 → 14px (padding 6px, items 8px). |
| radius | `radius-card-lg` | `radius-panel` | 16px | 14px | Cartes de l’accueil 16 → 14px, comme celles de Réglages. |
| spacing | `space-3` | `space-4` | 3px | 4px | Valeur impaire ramenée au pas de 2px dans les composants. |
| spacing | `space-5` | `space-6` | 5px | 6px | Valeur impaire ramenée au pas de 2px dans les composants. |
| spacing | `space-7` | `space-8` | 7px | 8px | Valeur impaire ramenée au pas de 2px dans les composants. |
| spacing | `space-9` | `space-10` | 9px | 10px | Valeur impaire ramenée au pas de 2px dans les composants. |
| spacing | `space-22` | `space-24` | 22px | 24px | Valeur impaire ramenée au pas de 2px dans les composants. |
| size | `size-easel-nav` | `size-settings-nav` | 220px | 210px | Navigation d’Easel 220 → 210px, comme Réglages et la Bibliothèque. |
| opacity | `opacity-disabled` | — | 0.25 | 0.4 | Barre du haut .25 → .4, comme Réglages. |
| opacity | `opacity-asleep` | `opacity-section` | 0.45 | 0.5 | Onglet endormi, en-tête de menu .45 → .5. |
| opacity | `opacity-muted` | — | 0.55 | 0.6 | .55 → .6. |
| opacity | `opacity-tertiary` | `opacity-muted` | 0.6 | 0.6 | Même valeur (.6). |
| opacity | `opacity-space-idle` | `opacity-muted` | 0.62 | 0.6 | Puce d’espace inactive .62 → .6. |
| opacity | `opacity-idle` | `opacity-secondary` | 0.75 | 0.7 | Boutons icône et navigation au repos .75 → .7. |
| opacity | `opacity-space-hover` | `opacity-soft` | 0.92 | 0.85 | Puce d’espace survolée .92 → .85. |
| duration | `duration-instant` | `duration-fast` | 0.12s | 0.15s | Bouton « … » de l’accueil .12 → .15s. |
| duration | `duration-press` | `duration-fast` | 0.16s | 0.15s | Enfoncement .16 → .15s. |
| duration | `duration-panel` | — | 0.38s | 0.4s | Palette .38 → .4s. |
| duration | `duration-switch-space` | `duration-panel` | 0.42s | 0.4s | Glissement entre espaces .42 → .4s. |
| duration | `duration-lit` | `duration-bloom` | 0.55s | 0.6s | Onglet qui s’allume .55 → .6s. |
| easing | `ease-pop` | — | cubic-bezier(.2,1.4,.4,1) | cubic-bezier(.2,1.3,.4,1) | Pop (.2,1.4,.4,1), toast (.2,1.3,.4,1) et menu (.2,1.2,.35,1) → une courbe. |
| easing | `ease-toast` | `ease-pop` | cubic-bezier(.2,1.3,.4,1) | cubic-bezier(.2,1.3,.4,1) | Même courbe. |
| easing | `ease-menu` | `ease-pop` | cubic-bezier(.2,1.2,.35,1) | cubic-bezier(.2,1.3,.4,1) | Même courbe. |
| easing | `ease-settle` | `ease-glide` | cubic-bezier(.2,.8,.2,1) | cubic-bezier(.2,.85,.25,1) | Onglet allumé, segments : (.2,.8,.2,1) → (.2,.85,.25,1). |
| easing | `ease-panel` | `ease-glide` | cubic-bezier(.22,.9,.28,1) | cubic-bezier(.2,.85,.25,1) | Palette : (.22,.9,.28,1) → (.2,.85,.25,1). |
| easing | `cubic-bezier(.3,1.5,.5,1) / (.3,1.6,.5,1)` | `ease-press` | barre du haut, pastilles | (.3,1.4,.5,1) | Un seul ressort de pression. |
| type | `body-page` | — | accueil, palette, erreurs 14/1.4 | 14/1.45 | Un seul corps de page. |
| type | `title-library` | `title-settings` | 20px/700 | 22px/600 | Titre de la Bibliothèque aligné sur les autres pages. |
| type | `section-label` | — | .07em / .09em | .08em | Un seul interlettrage. |
| type | `pile de la Bibliothèque` | `font-sans` | -apple-system,Segoe UI,sans-serif | font-sans | Ajoute system-ui. |
| color | `teinte des initiales de site` | — | hsl(h,55%,48%) accueil/Réglages | hsl(h,60%,50%) | Le même site a la même couleur partout. |

## Échelles retenues

- Rayons : 4 (`radius-bar`) · 6 (`radius-chip`) · 8 (`radius-control`) · 10 (`radius-card-sm`) · 12 (`radius-card`) · 14 (`radius-panel`) · 18 (`radius-dialog`) · `radius-pill` · `radius-round`. Rayon intérieur = rayon extérieur − marge (menu 14 − 6 = items 8 ; segments 8 − 2 = 6).
- Espacement : pas de 2px (2 4 6 8 10 12 14 16 18 20 24 28 36 40 56) ; `space-1` reste pour les marges de 1px des puces.
- Opacités : .35 glisser · .4 désactivé · .5 section · .6 atténué · .7 secondaire · .85 doux.
- Durées : .15 · .18 · .2 · .25 · .3 · .4 · .45 · .6 s (+ splash 1.9 s et respiration 5 s).
- Courbes : `ease-standard`, `ease-press`, `ease-pop`, `ease-glide`, `ease-grow`, `ease-switch`.
- Rouges : `danger-fill` (armé, survol destructeur), `danger-solid` (échec), `danger-text` et `danger-text-soft` (libellés), `insecure-fill` / `insecure-border` (connexion non sûre), `media-off`.
- Verts : `success` (points, remplissages), `success-text` (texte, éclair).

## Ce qui reste à faire dans le code

Le dépôt n’a pas été modifié. Pour que l’app corresponde : remplacer les valeurs en dur par les `--bh-*` (une page à la fois), faire passer accueil, Bibliothèque et palette sur `--bh-accent`, donner au bouton principal des erreurs la teinte de la page, et ajouter la règle `body.noanim` + `prefers-reduced-motion` aux pages qui ne l’ont pas (voir Incohérences).
