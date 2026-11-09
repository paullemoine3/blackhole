// Interface language of BlackHole's own pages, injected in each of them with the catalog of the
// current language (`__BH_I18N__` = {lang, s: {key: text}}, see src/i18n.rs).
// `bhT('settings.title')`, `bhT('toast.saved', {host})`, `bhTn('tabs.count', n)` (`.one`/`.other`).
// Markup: `data-i18n` (text), `data-i18n-title`, `data-i18n-placeholder`, `data-i18n-aria`
// (aria-label), applied with textContent / attributes only. The shell sends `bhI18n.set(catalog)`
// when the language changes: markup is applied again and pages hear a `bhlang` event to re-render.
(() => {
  if (window.bhI18n) return;
  let cat = __BH_I18N__;
  const one = (n) => (cat.lang === 'fr' ? Math.abs(n) < 2 : n === 1);
  const fill = (text, p) => (p ? text.replace(/\{(\w+)\}/g, (m, k) => (k in p ? String(p[k]) : m)) : text);
  const t = (key, p) => fill(cat.s[key] ?? key, p);
  const tn = (key, n, p) =>
    fill(cat.s[key + (one(n) ? '.one' : '.other')] ?? cat.s[key + '.other'] ?? key, Object.assign({ n }, p));
  const ATTRS = [['i18nTitle', 'title'], ['i18nPlaceholder', 'placeholder'], ['i18nAria', 'aria-label']];
  const apply = (root) => {
    const scope = root || document;
    if (!scope.querySelectorAll) return;
    for (const el of scope.querySelectorAll('[data-i18n],[data-i18n-title],[data-i18n-placeholder],[data-i18n-aria]')) {
      if (el.dataset.i18n) el.textContent = t(el.dataset.i18n);
      for (const [data, attr] of ATTRS) if (el.dataset[data]) el.setAttribute(attr, t(el.dataset[data]));
    }
  };
  const set = (next) => {
    cat = next;
    document.documentElement.lang = cat.lang;
    apply(document);
    window.dispatchEvent(new Event('bhlang'));
  };
  Object.defineProperty(window, 'bhT', { value: t });
  Object.defineProperty(window, 'bhTn', { value: tn });
  Object.defineProperty(window, 'bhI18n', { value: Object.freeze({ set, apply, get lang() { return cat.lang; } }) });
  const ready = () => { document.documentElement.lang = cat.lang; apply(document); };
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', ready, { once: true });
  else ready();
})();
