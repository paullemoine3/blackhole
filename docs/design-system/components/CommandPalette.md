# CommandPalette

Palette de commandes (palette.html `#panel`) au-dessus d’un voile `scrim`, à 19vh du haut.

- Panneau : `size-palette` (640px ; 1000px en comparateur), `surface-panel`, bordure `border-default`, `radius-dialog`, `shadow-palette` (halo d’accent).
- Recherche : 18px, padding 16px 20px, séparateur `border-divider`.
- Ligne : padding 8px 10px, `radius-card-sm` ; sélectionnée `accent-selection` + blanc. Icône 22px `radius-chip` ; glyphe `accent` sur `accent-glow-soft`.
- Pied : 12px, `opacity-section`, touches `fill-control-strong` `radius-chip`.
- Ouverture : un seul mouvement. Voile en `duration-quick` ; panneau qui glisse en place (`palette-in` : −12px, échelle .97, `duration-panel`, `ease-glide`) ; rangées qui montent de 6px (`row-in`, `duration-quick`, `ease-glide`), décalées de 14ms. Nouvel onglet seulement : une lueur d’accrétion (`accretion-hot` vers `accent-halo`) enfle derrière le panneau et s’éteint, une fois (`duration-bloom`). Ni écrasement, ni rotation, ni flou. Coupée par le réglage « animations » (`body.noanim`) et par `prefers-reduced-motion`.
- Vues internes : Boost (éditeur CSS mono 12.5/1.5), authentification HTTP, emoji (grille de 10), comparateur (meilleure valeur `success-text`).
