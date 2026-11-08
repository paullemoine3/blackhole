// Shared look of BlackHole's own pages, injected in each of them: thin scroll bars in the theme
// (a translucent thumb that takes the space's color under the pointer, no track). A constructed
// sheet, so page CSPs and page styles cannot drop it.
(() => {
  if (window.__bhTheme) return;
  Object.defineProperty(window, '__bhTheme', { value: true });
  const css = `
    ::-webkit-scrollbar { width: 10px; height: 10px; }
    ::-webkit-scrollbar-track, ::-webkit-scrollbar-corner { background: transparent; }
    ::-webkit-scrollbar-thumb { background-color: rgba(255,255,255,.14); border-radius: 10px; border: 3px solid transparent; background-clip: padding-box; }
    ::-webkit-scrollbar-thumb:hover { background-color: color-mix(in srgb, var(--accent, #7c5cff) 70%, transparent); }
    ::-webkit-scrollbar-thumb:active { background-color: var(--accent, #7c5cff); }
    ::-webkit-scrollbar-button { display: none; }
  `;
  try {
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(css);
    document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
  } catch (e) {}
})();
