# HomeTiles

Page d’accueil (home.html) : cartes Favoris, Récents et Nouveautés, posées sur `bg-page` avec la lueur de l’espace (`accent-glow-soft`).

- Cartes : voir SettingsCard (variante contenu, `radius-panel`, padding 18px).
- Favori : tuile 46px `radius-panel`, initiale 20px/700 sur `hsl(h,60%,50%)` (même teinte que la barre latérale) ; survol `fill-raised`, −2px, tuile `scale(1.08)` + `shadow-tile-lift`, ressort `ease-press`.
- Récent : bordure `border-hairline`, tuile 36px `radius-card-sm` ; survol `border-strong` + `shadow-item-hover` (lueur de l’espace).
- Nouveautés : dégradé `accent-glow-soft` → `fill-faint`, bordure `accent-border`, bouton primaire.
