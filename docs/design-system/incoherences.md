# Incohérences

Valeurs relevées dans `crates/app/ui/` qui diffèrent pour un même rôle, ou qui ne suivent pas la teinte de l’espace. Le design system les a unifiées (section Unification) ; le code, lui, n’a pas encore été modifié. Statut en fin de point : **unifié** (une valeur retenue dans le système), **gardé** (différence voulue), **code** (règle posée, reste à appliquer dans l’app).

## Couleur

1. **La Bibliothèque a sa propre palette.** library.html déclare `--a:#6a8cff`, `--bg:#16171c`, `--fg:#e8e9ee`, `--dim:#8b8e99`, `--card:#22242b`, alors que tout le reste utilise `bg-shell` #100f17 et `text-primary` #eceaf5. Son accent par défaut (#6a8cff) n’est aucune des couleurs d’espace. — **unifié**
2. **Violet en dur hors du shell.** L’accueil (lueur, carte Nouveautés, bouton, ombre des récents), les boutons des pages d’erreur (rgba(124,92,255,.85)) et le fond des glyphes de la palette (rgba(124,92,255,.22)) restent violets quel que soit l’espace ; le bouton principal d’une page d’erreur « danger » ou « calm » est violet sur fond rouge ou bleu. — **unifié**
3. **Rouge « danger » en sept valeurs** : #ff5a5a à .85 (Réglages, palette, barre de partage, Bibliothèque `#ff5a5ad9`), à .9 (accueil), #ff5c5c à .4 (carte média), #e5534b (téléchargement en échec), #b4443c (mise à jour en erreur), #c2455b (survol danger d’Easel), #ff8a8a / #ffb3b3 / #ffb3bd pour les textes. — **unifié**
4. **« Non sécurisé » en deux réglages** : fond crimson .22 et bordure .4 dans la barre du haut, .18 et .45 sur le bouton risqué des pages d’erreur. — **unifié**
5. **Trois verts de succès** : #2fd4a0 (statut), #6ee7a8 (comparateur), #8ff0b4 (compteur de bloqueurs). — **unifié**
6. **Deux gris foncés quasi identiques** : #2a2838 (fantôme d’onglet) et #2b2a3a (toast d’Easel, carte Easel par défaut). — **unifié**
7. **Deux toasts différents** : barre latérale #1b1a26 (12px, `radius-card-sm`) et Easel #2b2a3a (14px, padding 8px 14px). — **unifié**
8. **Teinte des initiales de site** : `hsl(h,60%,50%)` dans la barre latérale, la palette et la barre de partage, `hsl(h,55%,48%)` dans l’accueil et Réglages. Le même site n’a pas la même couleur partout. — **unifié**
9. **Placeholder** : rgba(236,234,245,.4) dans la palette, .5 dans la barre d’adresse. — **unifié**
10. **Voile** : rgba(6,5,12,.46) derrière la palette, .55 en mode aperçu. — **unifié**
11. **Or de l’anneau** : rgba(255,205,120,.9) (logo, splash, erreurs) et rgba(255,200,110,.85) (ouverture de la palette) ; orange du trou noir rgba(255,170,60) en CSS, #ff9a3c / #ffb35a dans le canvas de la Bibliothèque et title.svg. — **unifié**
12. **Valeur de repli de `--glow`** : .5 dans index.html, .35 dans le CSS de bar.html, settings.html et easel.html, alors que leur JS pose .5. Avant le premier état, la barre du haut est moins lumineuse que la barre latérale. — **code**
13. **Fond de page** : #0c0b12 (accueil, erreurs) contre #100f17 (shell). L’accueil apparaît légèrement plus sombre que le cadre qui l’entoure. — **gardé**

## Contraste

14. **Blanc sur l’accent** sous 4,5:1 dans tous les espaces : violet 4,35, rouge 3,70, gris 3,39, rose 2,94, bleu 2,74, sarcelle 2,42, vert 1,90, ambre 1,83. — **gardé, correction proposée**

## Typographie

15. **Corps** : 13/1.4 (shell), 14/1.45 (Réglages, Easel), 14/1.4 (accueil, palette, erreurs), 12.5/1.4 (barre de partage), 13 sans interligne (Bibliothèque, aperçu). — **unifié**
16. **Pile de polices** : la Bibliothèque utilise `-apple-system,Segoe UI,sans-serif`, sans `system-ui` et sans guillemets. — **unifié**
17. **Titres de section** : interlettrage .08em (barre latérale, menu), .09em (Réglages, accueil), .07em (cartes Easel). — **unifié**
18. **Titre h1** : 20px en gras par défaut (700) dans la Bibliothèque, 22–30px en 600 ailleurs. — **unifié**

## Rayons et tailles

19. **Seize rayons** (3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 16, 18, 999px, 50 %). Boutons à 8px dans la barre du haut et la Bibliothèque, 9px dans Réglages, Easel et la palette. — **unifié**
20. **Padding des boutons** : 7px 14px (Réglages), 7px 12px (Easel), 6px 12px (palette), 8px 12px (Bibliothèque), 8px 20px (erreurs), 6px 14px (accueil). — **unifié**
21. **Navigation latérale** : 210px (Réglages, Bibliothèque) contre 220px (Easel), padding 16px 12px contre 18px 10px. — **unifié**
22. **Opacité « désactivé »** : .25 dans la barre du haut, .4 dans Réglages. — **unifié**

## Mouvement

23. **Réglage « animations » non respecté partout.** Seuls index.html, bar.html et home.html posent `body.noanim`. error.html utilise un autre nom (`body.still`). La palette ne fait que sauter l’animation d’ouverture. Réglages, Easel, la Bibliothèque, la barre de partage et l’aperçu l’ignorent. La refonte des Réglages (travail en cours, hors de cette branche) gère `body.noanim`. — **code**
24. **`prefers-reduced-motion` incomplet.** Absent de palette.html (ressort, trou noir, rangées décalées), easel.html, splitbar.html et peek.html. Dans settings.html, il ne coupe que l’interrupteur : l’entrée des sections (`@keyframes in`) et le contrôle segmenté restent animés. La refonte des Réglages (travail en cours, hors de cette branche) étend `prefers-reduced-motion` à toute la page. — **code**
25. **Courbes de ressort voisines** : cubic-bezier(.3,1.4,.5,1), (.3,1.5,.5,1) et (.3,1.6,.5,1) pour le même geste ; (.2,.85,.25,1) et (.2,.85,.25,1.1) pour la même entrée. — **unifié**
26. **Durées de survol** : .15s (barre du haut, Bibliothèque), .18s (barre latérale, accueil), .12s (bouton « … » de l’accueil). — **unifié**

## Doublons de code

27. `mix()`, `rgba()` et `hue()` sont recopiés dans index.html, settings.html, easel.html, bar.html, palette.html, splitbar.html et home.html. La base de `mix()` ([16, 15, 23]) duplique `SHELL_BG` de views.rs. — **code**
28. Les boutons de fenêtre Windows (`.win`) sont recopiés à l’identique dans bar.html, settings.html et easel.html ; l’en-tête `#top` de 38px est recopié dans settings.html et easel.html. — **code**
29. `SPACE_COLORS` (main.rs:121) et `PALETTE` (index.html:265) listent les mêmes cinq couleurs ; `COLORS` (index.html:393) en ajoute trois. — **code**
