# EaselCard

Carte posée librement sur le tableau Easel (easel.html `.card`) : note, extrait (clip) ou région de page en direct (live).

- Tableau : `bg-board` avec une trame de points `fill-dot` tous les 24px.
- Carte : `radius-card`, `shadow-easel-card` et un contour blanc .1 (ombre, pas de bordure) ; au survol l’ombre s’approfondit, rien ne bouge (la vue native d’une carte en direct est posée sur `.slot`). Sa couleur `easel-*` (crates/domain easel::COLORS) arrive en `--c` ; la première, `easel-ink`, vaut « sans couleur » (classe `plain`) : la carte suit alors `accent`.
- Barre de saisie : transparente, type en 10.5px capitales 700 .08em ; les outils (24px, `radius-chip`) n’apparaissent qu’au survol, au focus ou pendant le choix de couleur.
- Note : une feuille. Fond `lib-note-bg`, texte `lib-note-fg`, placeholder `lib-note-dim` ; avec une couleur, le papier en prend 16 % et un filet de 3px en haut.
- Clip : fond `ink-700`, bandeau de 48px en dégradé de sa couleur vers `easel-blue` (c’est la poignée), tuile du site (initiale sur la teinte de l’hôte) à cheval dessous, lien 600 blanc, hôte 11.5px, commentaire.
- Live : fond `ink-700`, contour et lueur de sa couleur, point devant le type : rouge et battant (`danger-text`, 1,6 s) quand la zone joue, gris en pause ; zone noire .3 en retrait de 8px, pied 11px qui ouvre la page. Le bouton lecture reste visible.
- Animations coupées par `body.noanim` (champ `animations` de l’état) et `prefers-reduced-motion`.
- Sélecteur de couleur : pastilles 14px, bordure blanche .5.
