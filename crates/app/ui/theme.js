// Shared look of BlackHole's own pages, injected in each of them (with icons.js): the design
// system's tokens as CSS variables, the icon scale, and thin scroll bars in the theme (a
// translucent thumb that takes the space's color under the pointer, no track). A constructed
// sheet, so page CSPs and page styles cannot drop it.
//
// Tokens: `--bh-<token>` for every token of the BlackHole design system (tokens.json of the
// "BlackHole Design System" artifact). The space-dependent ones (`--bh-accent`, `--bh-tint-top`,
// the accent washes and glows) follow the `--accent`/`--glow`/`--glow2`/`--tint1`/`--tint2`/`--sel`
// variables each page sets from the space's color. Everything is under `:where()`, so a page's
// own rule always wins over these defaults.
//
// Icons: `.bh-i` (drawn by icons.js) in four sizes, xs 12, sm 16 (default), md 20, lg 24; a
// 1.5 px stroke at every size, aligned on the text's middle when inline.
(() => {
  if (window.__bhTheme) return;
  Object.defineProperty(window, '__bhTheme', { value: true });
  const css = `
  :where(:root) {
    --bh-ink-950: #07060c;
    --bh-ink-900: #0c0b12;
    --bh-ink-850: #100f17;
    --bh-ink-800: #14131c;
    --bh-ink-700: #1b1a26;
    --bh-ink-650: #1e1d2a;
    --bh-ink-600: var(--bh-ink-550);
    --bh-ink-550: #2b2a3a;
    --bh-fg-100: #eceaf5;
    --bh-white: #ffffff;
    --bh-black: #000000;
    --bh-space-violet: #7c5cff;
    --bh-space-pink: #ff5c8a;
    --bh-space-green: #2fd4a0;
    --bh-space-amber: #ffb020;
    --bh-space-blue: #3aa0ff;
    --bh-space-red: #e5534b;
    --bh-space-teal: #14b8c4;
    --bh-space-grey: #8b8a99;
    --bh-violet-300: #a78bff;
    --bh-violet-200: #c9bcff;
    --bh-violet-100: #beaaff;
    --bh-wordmark: #8f74ff;
    --bh-accretion-hot: #ffaa3c;
    --bh-accretion-warm: #ff5a28;
    --bh-accretion-ring: #ffcd78;
    --bh-loader-gold: #ffb020;
    --bh-red-500: #e5534b;
    --bh-red-600: #b4443c;
    --bh-red-400: #ff5a5a;
    --bh-rose-600: #c2455b;
    --bh-crimson-500: #dc2846;
    --bh-green-300: #6ee7a8;
    --bh-green-200: #8ff0b4;
    --bh-orange-200: #ffb86b;
    --bh-bg-shell: var(--bh-ink-850);
    --bh-bg-page: var(--bh-ink-900);
    --bh-bg-splash: var(--bh-ink-950);
    --bh-bg-board: var(--bh-ink-800);
    --bh-surface-panel: #1b1a26f5;
    --bh-surface-menu: #1e1d2af7;
    --bh-surface-toast: var(--bh-ink-700);
    --bh-surface-toast-easel: var(--bh-surface-toast);
    --bh-surface-ghost: var(--bh-ink-600);
    --bh-scrim: #06050c80;
    --bh-scrim-peek: var(--bh-scrim);
    --bh-scrim-caption: #000000aa;
    --bh-fill-sunken: #ffffff05;
    --bh-fill-faint: #ffffff0a;
    --bh-fill-card: #ffffff0d;
    --bh-fill-field: #ffffff0f;
    --bh-fill-hover: #ffffff12;
    --bh-fill-hover-strong: #ffffff14;
    --bh-fill-raised: #ffffff17;
    --bh-fill-control: #ffffff1a;
    --bh-fill-control-strong: #ffffff1f;
    --bh-fill-pressed: #ffffff24;
    --bh-fill-selected: #ffffff29;
    --bh-fill-control-hover: #ffffff2e;
    --bh-fill-selected-strong: #ffffff33;
    --bh-fill-glint: #ffffff38;
    --bh-fill-dot: #ffffff12;
    --bh-border-hairline: #ffffff0f;
    --bh-border-faint: #ffffff12;
    --bh-border-divider: #ffffff14;
    --bh-border-card: #ffffff17;
    --bh-border-default: #ffffff1a;
    --bh-border-field: #ffffff1f;
    --bh-border-strong: #ffffff24;
    --bh-border-hover: #ffffff2e;
    --bh-border-dashed: #ffffff4c;
    --bh-border-radio: #ffffff66;
    --bh-text-primary: var(--bh-fg-100);
    --bh-text-secondary: #eceaf599;
    --bh-text-on-accent: var(--bh-white);
    --bh-text-placeholder: #eceaf580;
    --bh-text-placeholder-bar: var(--bh-text-placeholder);
    --bh-text-stats: var(--bh-text-secondary);
    --bh-accent: var(--accent, #7c5cff);
    --bh-accent-glow: var(--glow, #7c5cff80);
    --bh-accent-glow-soft: var(--glow2, #7c5cff38);
    --bh-tint-top: var(--tint1, var(--tint, #30265d));
    --bh-tint-bottom: var(--tint2, #2a214f);
    --bh-accent-selection: var(--sel, #7c5cffd9);
    --bh-accent-wash: color-mix(in srgb, var(--accent, #7c5cff) 28%, transparent);
    --bh-accent-lit: color-mix(in srgb, var(--accent, #7c5cff) 42%, transparent);
    --bh-accent-border: color-mix(in srgb, var(--accent, #7c5cff) 45%, transparent);
    --bh-accent-halo: color-mix(in srgb, var(--accent, #7c5cff) 55%, transparent);
    --bh-accent-shadow: color-mix(in srgb, var(--accent, #7c5cff) 60%, transparent);
    --bh-accent-deep: #44338c;
    --bh-meet-fill: #9379ff44;
    --bh-palette-glyph-fill: var(--bh-accent-glow-soft);
    --bh-violet-fixed-85: var(--bh-accent-selection);
    --bh-violet-fixed-wash: var(--bh-accent-glow-soft);
    --bh-violet-fixed-border: var(--bh-accent-border);
    --bh-danger-fill: #ff5a5ad9;
    --bh-danger-solid: var(--bh-red-500);
    --bh-danger-deep: var(--bh-danger-solid);
    --bh-danger-hover-easel: var(--bh-danger-fill);
    --bh-danger-text: #ff8a8a;
    --bh-danger-text-soft: #ffb3bd;
    --bh-insecure-text: var(--bh-danger-text-soft);
    --bh-insecure-fill: #dc284638;
    --bh-insecure-border: #ff606e66;
    --bh-media-off: #ff5a5a66;
    --bh-success: var(--bh-space-green);
    --bh-success-text: var(--bh-green-300);
    --bh-success-flash: var(--bh-success-text);
    --bh-warning-text: var(--bh-orange-200);
    --bh-font-sans: -apple-system, "Segoe UI", system-ui, sans-serif;
    --bh-font-mono: ui-monospace, Menlo, Consolas, monospace;
    --bh-text-title-home: 600 30px/1.4 var(--bh-font-sans);
    --bh-text-title-error: 600 26px/1.4 var(--bh-font-sans);
    --bh-text-title-settings: 600 22px/1.45 var(--bh-font-sans);
    --bh-text-splash-word: 600 22px/1.4 var(--bh-font-sans);
    --bh-text-title-library: 600 22px/1.45 var(--bh-font-sans);
    --bh-text-title-easel: 600 18px/1.45 var(--bh-font-sans);
    --bh-text-palette-query: 400 18px/1.4 var(--bh-font-sans);
    --bh-text-title-dialog: 600 16px/1.4 var(--bh-font-sans);
    --bh-text-title-wait: 600 15px/1.4 var(--bh-font-sans);
    --bh-text-body-page: 400 14px/1.45 var(--bh-font-sans);
    --bh-text-body-shell: 400 13px/1.4 var(--bh-font-sans);
    --bh-text-body-strong: 600 13px/1.4 var(--bh-font-sans);
    --bh-text-body-small: 400 12.5px/1.4 var(--bh-font-sans);
    --bh-text-caption: 400 12px/1.4 var(--bh-font-sans);
    --bh-text-section-label: 600 11px/1.4 var(--bh-font-sans);
    --bh-text-micro: 400 10.5px/1.4 var(--bh-font-sans);
    --bh-text-badge: 700 9px/14px var(--bh-font-sans);
    --bh-text-pairing-code: 600 26px/1.2 var(--bh-font-mono);
    --bh-text-host: 400 13px/1.4 var(--bh-font-mono);
    --bh-text-code: 400 12.5px/1.5 var(--bh-font-mono);
    --bh-text-kbd: 400 12px/1.4 var(--bh-font-mono);
    --bh-space-1: 1px;
    --bh-space-2: 2px;
    --bh-space-3: 3px;
    --bh-space-4: 4px;
    --bh-space-5: 5px;
    --bh-space-6: 6px;
    --bh-space-7: 7px;
    --bh-space-8: 8px;
    --bh-space-9: 9px;
    --bh-space-10: 10px;
    --bh-space-12: 12px;
    --bh-space-14: 14px;
    --bh-space-16: 16px;
    --bh-space-18: 18px;
    --bh-space-20: 20px;
    --bh-space-22: 22px;
    --bh-space-24: 24px;
    --bh-space-28: 28px;
    --bh-space-36: 36px;
    --bh-space-40: 40px;
    --bh-space-56: 56px;
    --bh-radius-hair: 4px;
    --bh-radius-bar: 4px;
    --bh-radius-key: 6px;
    --bh-radius-chip: 6px;
    --bh-radius-chip-button: 6px;
    --bh-radius-control: 8px;
    --bh-radius-row: 8px;
    --bh-radius-card-sm: 10px;
    --bh-radius-space: 12px;
    --bh-radius-card: 12px;
    --bh-radius-menu: 14px;
    --bh-radius-panel: 14px;
    --bh-radius-card-lg: 14px;
    --bh-radius-dialog: 18px;
    --bh-radius-pill: 999px;
    --bh-radius-round: 50%;
    --bh-size-topbar: 38px;
    --bh-size-traffic-lights: 84px;
    --bh-size-sidebar: 240px;
    --bh-size-settings-nav: 210px;
    --bh-size-easel-nav: 210px;
    --bh-size-icon-button: 32px;
    --bh-size-bar-button: 28px;
    --bh-size-window-button: 46px;
    --bh-size-omnibox: 30px;
    --bh-size-tab-dot: 18px;
    --bh-size-pinned: 48px;
    --bh-size-space: 34px;
    --bh-size-switch-w: 30px;
    --bh-size-switch-h: 18px;
    --bh-size-palette: 640px;
    --bh-size-progress: 3px;
    --bh-size-scrollbar: 10px;
    --bh-opacity-disabled: 0.4;
    --bh-opacity-dragging: 0.35;
    --bh-opacity-asleep: 0.5;
    --bh-opacity-section: 0.5;
    --bh-opacity-muted: 0.6;
    --bh-opacity-tertiary: 0.6;
    --bh-opacity-space-idle: 0.6;
    --bh-opacity-secondary: 0.7;
    --bh-opacity-idle: 0.7;
    --bh-opacity-soft: 0.85;
    --bh-opacity-space-hover: 0.85;
    --bh-duration-instant: 0.15s;
    --bh-duration-fast: 0.15s;
    --bh-duration-hover: 0.18s;
    --bh-duration-press: 0.15s;
    --bh-duration-quick: 0.2s;
    --bh-duration-base: 0.25s;
    --bh-duration-spring: 0.3s;
    --bh-duration-panel: 0.4s;
    --bh-duration-switch-space: 0.4s;
    --bh-duration-tint: 0.45s;
    --bh-duration-lit: 0.6s;
    --bh-duration-bloom: 0.6s;
    --bh-duration-splash: 1.9s;
    --bh-duration-breathe: 5s;
    --bh-ease-press: cubic-bezier(.3,1.4,.5,1);
    --bh-ease-grow: cubic-bezier(.3,1.2,.4,1);
    --bh-ease-glide: cubic-bezier(.2,.85,.25,1);
    --bh-ease-settle: cubic-bezier(.2,.85,.25,1);
    --bh-ease-pop: cubic-bezier(.2,1.3,.4,1);
    --bh-ease-toast: cubic-bezier(.2,1.3,.4,1);
    --bh-ease-menu: cubic-bezier(.2,1.3,.4,1);
    --bh-ease-switch: cubic-bezier(.2,.9,.3,1.35);
    --bh-ease-panel: cubic-bezier(.2,.85,.25,1);
    --bh-ease-standard: ease;
    --bh-shadow-page-card: 0 14px 36px #00000080, 0 3px 10px #00000066, 0 0 0 1px #ffffff12;
    --bh-shadow-flyout: 6px 0 24px #00000073;
    --bh-shadow-ghost: 0 8px 24px #00000080;
    --bh-shadow-easel-card: 0 6px 22px #00000059;
    --bh-shadow-palette: 0 30px 80px #0000008c, 0 0 0 1px #00000066, 0 0 70px -20px var(--accent, #7c5cff);
    --bh-shadow-menu: 0 18px 50px #0000008c, 0 0 0 1px #00000059;
    --bh-shadow-ring: 0 0 0 1px #ffffff1a;
    --bh-shadow-ring-update: 0 0 0 1px #ffffff1a;
    --bh-shadow-tab-active: inset 0 0 0 1px #ffffff0f, 0 1px 0 #ffffff0a;
    --bh-shadow-tab-split: inset 3px 0 0 var(--accent, #7c5cff);
    --bh-shadow-pinned-active: 0 0 0 1px #ffffff33, 0 8px 18px -8px color-mix(in srgb, var(--accent, #7c5cff) 50%, transparent);
    --bh-shadow-space-active: 0 0 0 2px #ffffffeb, 0 5px 14px color-mix(in srgb, var(--accent, #7c5cff) 60%, transparent);
    --bh-shadow-update-hover: 0 0 0 1px #ffffff4d, 0 6px 20px color-mix(in srgb, var(--accent, #7c5cff) 50%, transparent);
    --bh-shadow-meet-live: 0 0 0 1px var(--accent, #7c5cff), 0 0 14px -4px color-mix(in srgb, var(--accent, #7c5cff) 50%, transparent);
    --bh-shadow-switch-on: 0 0 10px color-mix(in srgb, var(--accent, #7c5cff) 55%, transparent);
    --bh-shadow-switch-knob: 0 1px 3px #0000004d;
    --bh-shadow-omni-hover: 0 0 0 3px #ffffff0a, 0 6px 18px -10px color-mix(in srgb, var(--accent, #7c5cff) 50%, transparent);
    --bh-shadow-split-focus: inset 0 -2px 0 var(--accent, #7c5cff);
    --bh-shadow-progress: 0 0 8px var(--accent, #7c5cff);
    --bh-shadow-tile-lift: 0 10px 22px -10px #000000b3;
    --bh-shadow-item-hover: 0 10px 24px -16px color-mix(in srgb, var(--accent, #7c5cff) 70%, transparent);
    --bh-shadow-lib-lift: 0 10px 22px -10px #000000b3;
    --bh-glow-logo: 0 0 30px #ff78284d;
    --bh-glow-hole: 0 0 80px #ff782847, 0 0 160px #7c5cff40;
    --bh-glow-star: 0 0 6px 1px #beaaffe6;
    --bh-icon-xs: 12px;
    --bh-icon-sm: 16px;
    --bh-icon-md: 20px;
    --bh-icon-lg: 24px;
    --bh-icon-stroke: 1.5px;
    --bh-size-hit-min: 24px;
  }
  :where(.bh-i) { width: var(--bh-icon-sm); height: var(--bh-icon-sm); flex: none; display: inline-block; vertical-align: -0.2em;
    fill: none; stroke: currentColor; stroke-width: var(--bh-icon-stroke); stroke-linecap: round; stroke-linejoin: round; overflow: visible; pointer-events: none; }
  :where(.bh-i) :where(path, rect, circle, line, polyline, polygon, ellipse) { vector-effect: non-scaling-stroke; }
  :where(.bh-i-xs) { width: var(--bh-icon-xs); height: var(--bh-icon-xs); vertical-align: -0.1em; }
  :where(.bh-i-md) { width: var(--bh-icon-md); height: var(--bh-icon-md); }
  :where(.bh-i-lg) { width: var(--bh-icon-lg); height: var(--bh-icon-lg); }
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
