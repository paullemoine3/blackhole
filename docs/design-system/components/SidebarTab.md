# SidebarTab

Onglet de la barre latérale (index.html `.tab`), avec ses variantes épinglée (`#pinned .tab`), dans un dossier (`.fh` + `.fb`), en vue partagée et endormie.

- Rangée : padding 8px 8px 8px 12px, `radius-control`, écart 6px ; survol `fill-hover-strong` ; actif `fill-selected` + `shadow-tab-active` ; enfoncé `scale(.975)`.
- Pastille : `size-tab-dot` (18px), `radius-chip`, initiale 10px/700 sur une teinte `hsl(h,60%,50%)` dérivée de l’hôte, remplacée par le favicon sur `fill-selected`.
- Vue partagée : `shadow-tab-split` (filet d’accent 3px) sur `fill-raised`. Endormi : pastille et titre à `opacity-section`.
- Épinglés : grille de 4, hauteur `size-pinned`, `fill-hover` ; actif `fill-selected-strong` + `shadow-pinned-active`, se soulève de 1px au survol.
- Les boutons ✕ / ⋯ / partage n’existent qu’au survol (leur place revient au titre).
- Quand un onglet devient actif, il s’allume (`tab-lit`, `duration-bloom`, départ `accent-lit`).
- Fond de la colonne : lueur `accent-glow` en haut, `accent-glow-soft` en bas à droite, dégradé `tint-top` → `tint-bottom`.
