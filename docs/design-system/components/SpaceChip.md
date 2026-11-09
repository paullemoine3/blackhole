# SpaceChip

Puce d’espace en pied de barre latérale (index.html `.space`) : la couleur de l’espace en dégradé, son emoji ou son initiale par-dessus.

- Hauteur `size-space` (34px), `radius-card` (11px), dégradé 145° de la couleur vers `accent-deep` (55 % vers le noir).
- Repos `opacity-muted` (.62), survol `opacity-soft` (.92) et −1px ; enfoncé `scale(.94)`.
- Active : s’élargit (flex-grow 2.4, `ease-grow`), affiche le nom (12.5px/600), anneau blanc 2px et ombre colorée (`shadow-space-active`).
- Les couleurs disponibles sont `space-*` ; un nouvel espace prend la suivante dans SPACE_COLORS.
- Changer d’espace fait glisser la liste (`duration-panel`, `ease-glide`) et fond la teinte en `duration-tint`.
