# Switch

Option de Réglages : une rangée cliquable entière (`.opt`) avec un interrupteur (`.check`) ou un bouton radio (`.radio`).

- Rangée : padding 10px, `radius-control`, survol `fill-hover`, écart 12px.
- Interrupteur : `size-switch-w` × `size-switch-h` (30 × 18), piste `fill-control-hover` ; allumé : `accent` + `shadow-switch-on`. Curseur blanc 14px, `shadow-switch-knob`, glisse de 12px en `duration-spring` avec `ease-switch`.
- Radio : 15px, bordure `border-radio` ; sélectionné : `accent` plein avec un liseré intérieur `ink-700`.
- Le consommateur fournit le libellé et un sous-titre optionnel (`.sub`, 12.5px, `opacity-muted`).
