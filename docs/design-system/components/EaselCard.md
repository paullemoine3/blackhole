# EaselCard

Carte posée librement sur le tableau Easel (easel.html `.card`) : note, extrait (clip) ou région de page en direct (live).

- Tableau : `bg-board` avec une trame de points `fill-dot` tous les 24px.
- Carte : `radius-card`, `shadow-easel-card`, bordure `border-strong`, fond = sa couleur `easel-*` (crates/domain easel::COLORS, défaut `easel-ink`).
- Barre de saisie : fond noir .25, type en 11px capitales .07em à .65 ; outils 12px `radius-chip`, survol `fill-control-hover`.
- Clip : lien 600 blanc, hôte 11.5px. Live : zone noire .18, pied 10.5px.
- Sélecteur de couleur : pastilles 14px, bordure blanche .5.
