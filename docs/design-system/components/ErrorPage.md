# ErrorPage

Pages d’erreur de navigation (error.html) : un trou noir teinté selon le type d’erreur, un titre, l’URL, une aide et des actions.

- Fond `bg-page` + lueur radiale de la teinte à 22 %.
- Teintes : défaut `error-violet-*` ; `danger` (certificat, accès refusé, serveur) ; `calm` (hors ligne, bloqué) ; `amber` (horloge, http seul, connexion requise) ; `green` (adresse introuvable). Trois tons chacune : `accent`, `hot`, `warm`.
- Titre `title-error` (26px/600), URL 13px `opacity-muted`, aide 13px `opacity-secondary`, largeur max 460px.
- Actions : Button en taille lg. Le primaire prend la teinte de la page ; `ghost` pour l’action secondaire ; `risky` pour passer outre.
- Le texte de cette page est en français. Animations coupées par `body.noanim` (aujourd’hui `body.still` dans le code) ou `prefers-reduced-motion`.
