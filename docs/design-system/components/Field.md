# Field

Champ de saisie des pages internes (settings `.field`, palette `#auth input`, `#eq`, `#css`) et champ de renommage en place (index `.fedit`).

- Fond `fill-field`, bordure 1px `border-field`, `radius-control`, padding 8px 10px, 13px (14px dans l’authentification, 15px pour la recherche d’emoji).
- Focus : la bordure passe à `accent`, pas d’anneau supplémentaire.
- Placeholder : `text-placeholder`.
- Renommage : `fill-control-strong`, bordure `accent`, `radius-chip`, padding 1px 6px, gras.
- Les champs sont les seuls éléments où la sélection de texte est permise (`user-select: text`).
