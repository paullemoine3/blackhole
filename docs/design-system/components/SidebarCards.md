# SidebarCards

Cartes empilées au-dessus du pied de la barre latérale : réunion (`#meet .mt`), lecteur média (`.mc`), téléchargement (`.dl`), rangement d’onglets (`#tidy .td`).

- Base : `radius-card-sm`, padding 8px 10px, 12px, `fill-raised` + `shadow-ring` ; entrée `dl-in` (.2s, glisse de 6px).
- Réunion : fond `meet-fill`, contour `accent-border` ; en cours `shadow-meet-live` ; bouton Join `accent`, `radius-control`.
- Média : boutons 28 × 26 `fill-control`, coupé `media-off`.
- Téléchargement : barre 4px `fill-pressed`, remplissage `accent` (échec `danger-solid`) ; inconnu = segment de 35 % qui glisse en 1.1s. Terminé ou échoué : bouton de fermeture (icône `close` xs, zone de 24px) ; la carte sort par `dl-out` (`duration-spring`, l’inverse de son entrée), comme celle dont le délai est écoulé.
- Rangement : `fill-hover`, puces de groupes `fill-control` `radius-chip`, bouton principal `accent`.
