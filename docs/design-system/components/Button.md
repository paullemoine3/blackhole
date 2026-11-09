# Button

Bouton texte de toutes les pages internes, en une seule famille : variantes neutre, primaire, armée, fantôme et risquée, tailles sm, md et lg.

## Variantes
- **Neutre** (défaut) : `fill-control`, survol `fill-control-hover`.
- **Primaire** `.primary` : fond `accent`, texte `text-on-accent`. Sur une page d’erreur, il prend la teinte de la page (`error-*-accent` à 85 %, plein au survol).
- **Armé** `.armed` : premier clic d’une action destructrice, fond `danger-fill` ; le second clic confirme. Easel suit la même règle (son survol `danger-fill`).
- **Fantôme** `.ghost` : `fill-hover-strong`, survol `fill-pressed` (action secondaire des pages d’erreur).
- **Risqué** `.risky` : `insecure-fill`, texte `danger-text-soft`, bordure `insecure-border`.

## Tailles
- **sm** : 12px/600, padding 4px 10px, `radius-chip` (toasts, rangement d’onglets, lignes de mot de passe).
- **md** (défaut) : 13px, padding 8px 14px, `radius-control`.
- **lg** : padding 8px 20px (actions des pages d’erreur).

## Règles
- Un seul primaire par groupe d’actions.
- Pas de boîte de confirmation : une action irréversible s’arme.
- Désactivé : `opacity-disabled` (.4).
- Le blanc sur l’accent reste sous 4,5:1 (voir Contraste dans le README).
