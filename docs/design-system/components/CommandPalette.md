# CommandPalette

Palette de commandes (palette.html `#panel`) au-dessus d’un voile `scrim`, à 19vh du haut.

- Panneau : `size-palette` (640px ; 1000px en comparateur), `surface-panel`, bordure `border-default`, `radius-dialog`, `shadow-palette` (halo d’accent).
- Recherche : 18px, padding 16px 20px, séparateur `border-divider`.
- Ligne : padding 8px 10px, `radius-card-sm` ; sélectionnée `accent-selection` + blanc. Icône 22px `radius-chip` ; glyphe `accent` sur `accent-glow-soft`.
- Pied : 12px, `opacity-section`, touches `fill-control-strong` `radius-chip`.
- Ouverture : trou noir qui éclot derrière (`bloom` .6s), panneau en ressort (`duration-panel`, `ease-glide`), rangées décalées de 14ms. Seulement si les animations sont activées (pas de garde `prefers-reduced-motion`).
- Vues internes : Boost (éditeur CSS mono 12.5/1.5), authentification HTTP, emoji (grille de 10), comparateur (meilleure valeur `success-text`).
