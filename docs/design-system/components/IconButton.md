# IconButton

Bouton carré à icône : 32px dans la barre latérale (`size-icon-button`), 28px dans la barre du haut (`size-bar-button`), 46px pour les boutons de fenêtre Windows.

- Repos : transparent, `opacity-secondary` (.75) ; survol : `fill-control`, opacité 1 ; enfoncé : `scale(.88)` avec `ease-press` (barre du haut) ou `.975` (barre latérale).
- Actif (`.on`) : l’icône prend `accent`.
- Désactivé : `opacity-disabled` (.25).
- Fenêtres Windows : survol `fill-control-strong`, Fermer `win-close` avec texte blanc ; icônes 10px, trait 1px.
- Icônes : SVG en ligne, `fill: none`, `stroke: currentColor`, trait 1.5–1.6, extrémités arrondies.
