// Page clean-up, Brave-style "shields", injected at document start in every frame of a page
// tab when switched on in Settings > Privacy (the blocked requests are page_guard.js's job):
// - hide ad slots and the space they leave (generic, well-known ad containers only);
// - video ads (YouTube): taken out of the player answer, else skipped to their end;
// - hide cookie consent banners of the common consent platforms and give back the scrolling
//   they lock.
// Styles go through a constructed sheet, which page CSPs do not block.
(() => {
  const SHIELDS = __SHIELDS__;
  if (window.__bhShields) return;
  Object.defineProperty(window, '__bhShields', { value: true });

  const ADS = [
    'ins.adsbygoogle', '.adsbygoogle', '[id^="google_ads_iframe"]', '[id^="div-gpt-ad"]',
    'div[id^="google_ads_"]', '[data-google-query-id]', 'iframe[src*="doubleclick.net"]',
    'iframe[src*="googlesyndication.com"]', 'iframe[id^="google_ads_"]', 'amp-ad', 'amp-embed[type="taboola"]',
    '[id^="taboola-"]', '.trc_rbox_container', '.OUTBRAIN', '[data-widget-id^="outbrain"]', '#carbonads',
    // YouTube's ad renderers (feed, sidebar, masthead, overlay).
    'ytd-ad-slot-renderer', 'ytd-in-feed-ad-layout-renderer', 'ytd-promoted-sparkles-web-renderer',
    'ytd-display-ad-renderer', 'ytd-banner-promo-renderer', 'ytd-statement-banner-renderer',
    'ytd-rich-item-renderer:has(> #content > ytd-ad-slot-renderer)', '#masthead-ad', '#player-ads',
    '.ytp-ad-overlay-container', '.ytp-ad-image-overlay',
    // Slots that say so (0.8.30). Never a name a classified site uses for its own ads (`.ad`,
    // `.ad-card`, `.ad-list`, `.advert`): only words that mean the place an ad is served in.
    '[data-ad-slot]', '[data-ad-unit]', '[data-ad-unit-path]', '[data-ad-position]', '[data-adunit]', '[data-prebid]',
    '[data-ad-client]', '[id^="dfp-"]', '[id^="gpt-ad"]', '[id^="ad-slot"]', '[id^="adslot"]', '[id^="ad_slot"]',
    '.ad-slot', '.adslot', '.ad-unit', '.ad-placeholder', '.ad-banner', '.banner-ad', '.ad-leaderboard', '.ad-skyscraper',
    '.ad-billboard', '.ad-slot-container', '.js-ad-slot', '.js-ad-placeholder', '.ad-actirise', '.adm-ad-loading',
    '[class*="adPlaceHolder"]', '[class*="adPlaceholder"]', '[class*="__ad-placeholder"]',
    'iframe[title="3rd party ad content"]', 'iframe[aria-label="Advertisement"]', 'iframe[src*="amazon-adsystem.com"]',
    'iframe[src*="smartadserver.com"]', 'iframe[src*="teads.tv"]', '.teads-inread', '[id^="sas_"][class*="sas"]',
    '[id^="taboola_"]', '.taboola', '[id^="outbrain_widget"]', '.ob-widget', '[id^="mgid"]', '[id^="rc-widget-"]',
    '[id^="amzn-native-ad"]', '[id^="amzn-assoc-ad"]', '[id^="ezoic-pub-ad"]', '[id^="st-ad-"]',
  ];
  // What is an ad on one site only, its subdomains included.
  const SITE_ADS = [
    [/(^|\.)google\.[a-z.]+$/, ['#tads', '#tadsb', '#bottomads', '.commercial-unit-desktop-top', '.commercial-unit-desktop-rhs']],
    [/(^|\.)bing\.com$/, ['li.b_ad', '.b_adTop', '.b_adBottom', '.b_adLastChild']],
    [/(^|\.)duckduckgo\.com$/, ['[data-layout="ad"]', '.result--ad', '.js-results-ads']],
    [/(^|\.)yahoo\.com$/, ['.searchCenterTopAds', '.searchCenterBottomAds', '.searchRightTopAds']],
    [/(^|\.)amazon\.[a-z.]+$/, ['.AdHolder', '[data-component-type="sp-sponsored-result"]', '[cel_widget_id*="adplacements"]']],
    [/(^|\.)reddit\.com$/, ['shreddit-ad-post', 'shreddit-comments-page-ad', 'shreddit-comment-tree-ad', '.promotedlink']],
    [/(^|\.)lemonde\.fr$/, ['.ds-area--pub', '.ds-home__pub__card']],
    [/(^|\.)lesnumeriques\.com$|(^|\.)marmiton\.org$/, ['[data-ad]', '[id^="ad-"][id$="-label"]', '.ad-container']],
    [/(^|\.)theguardian\.com$/, ['.top-fronts-banner-ad-container', '.top-banner-ad-container']],
    [/(^|\.)lefigaro\.fr$/, ['.fig-ad-content']],
    [/(^|\.)allocine\.fr$/, ['.ad-item']],
  ];
  for (const [host, list] of SITE_ADS) if (host.test(location.hostname)) ADS.push(...list);
  const BANNERS = [
    '#onetrust-banner-sdk', '#onetrust-consent-sdk', '#CybotCookiebotDialog', '#CybotCookiebotDialogBodyUnderlay',
    '#qc-cmp2-container', '.qc-cmp2-container', '.fc-consent-root', '#didomi-host', '#didomi-popup',
    '#usercentrics-root', '#truste-consent-track', '.truste_box_overlay', '#cookie-law-info-bar',
    '#cmpbox', '#cmpbox2', '#cmpwrapper', '[id^="sp_message_container"]', '.cc-window.cc-banner',
    '#iubenda-cs-banner', '#axeptio_overlay', '#tarteaucitronRoot', '#cookiescript_injected',
  ];

  const css = [];
  if (SHIELDS.hideAds) css.push(ADS.map((s) => s + ' { display: none !important; }').join('\n'));
  if (SHIELDS.cookieBanners) css.push(BANNERS.join(',\n') + ' { display: none !important; }');
  if (css.length) {
    try {
      const sheet = new CSSStyleSheet();
      sheet.replaceSync(css.join('\n'));
      // One rule per selector: one an engine does not know must not cancel the others. Put back
      // when the page assigns its own list, as component libraries do.
      const keep = () => {
        if (!document.adoptedStyleSheets.includes(sheet)) document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
      };
      keep();
      document.addEventListener('DOMContentLoaded', keep);
      addEventListener('load', keep);
      setInterval(keep, 2000);
    } catch (e) {}
  }

  // YouTube video ads (0.8.25: also in the players embedded by other sites, frames included).
  // First, the ads are taken out of what the player is told to play: the player answer
  // (`ytInitialPlayerResponse` in the page, then `/youtubei/v1/player` read as JSON) lists them
  // in `adPlacements`, `playerAds` and `adSlots`; without them the player has no ad to play.
  // Only on youtube.com: no other site sees these functions replaced. Then, for an ad that still
  // plays (one the server stitched in), the old way: jump to its end and press "Skip".
  if (SHIELDS.videoAds && /(^|\.)youtube(-nocookie)?\.com$/.test(location.hostname)) {
    const AD_KEYS = ['adPlacements', 'playerAds', 'adSlots', 'adBreakHeartbeatParams'];
    const prune = (o) => {
      if (!o || typeof o !== 'object') return o;
      for (const k of AD_KEYS) if (k in o) delete o[k];
      if (o.playerResponse && typeof o.playerResponse === 'object') prune(o.playerResponse);
      if (Array.isArray(o)) for (const part of o) if (part && part.playerResponse) prune(part.playerResponse);
      return o;
    };
    // Read as native code, like the engine's own functions (the player checks some of them).
    const wrap = (owner, name, after) => {
      const original = owner[name];
      if (typeof original !== 'function') return;
      try {
        Object.defineProperty(owner, name, {
          value: new Proxy(original, { apply: (fn, self, args) => after(Reflect.apply(fn, self, args)) }),
          writable: true, configurable: true,
        });
      } catch (e) {}
    };
    wrap(JSON, 'parse', prune);
    if (window.Response) wrap(Response.prototype, 'json', (p) => (p && typeof p.then === 'function' ? p.then(prune) : p));
    try {
      let initial;
      Object.defineProperty(window, 'ytInitialPlayerResponse', {
        configurable: true,
        get: () => initial,
        set: (v) => { initial = prune(v); },
      });
    } catch (e) {}
    const SKIP = '.ytp-skip-ad-button, .ytp-ad-skip-button, .ytp-ad-skip-button-modern, .ytp-ad-skip-button-container button, [id^="skip-button"] button';
    const skipAd = () => {
      const player = document.querySelector('.html5-video-player.ad-showing, .html5-video-player.ad-interrupting');
      if (!player) return;
      const video = player.querySelector('video');
      // The ad and the video share this element: jump to the ad's end, never touch the sound.
      if (video && isFinite(video.duration) && video.duration > 0 && video.currentTime < video.duration - 0.1) {
        video.currentTime = video.duration;
      }
      const skip = player.querySelector(SKIP);
      if (skip) skip.click();
    };
    setInterval(skipAd, 300);
  }

  if (window.top !== window) return;

  // The place a hidden ad leaves (0.8.30): a wrapper with a fixed height, a lone "Advertisement"
  // label, a grey box. A wrapper goes only while it holds nothing but ads and their label, never
  // a part of the page's own structure, and comes back if something else arrives in it.
  if (SHIELDS.hideAds) {
    const LABEL = /^(publicit\u00e9|annonce|advertisement|advertising|anzeige|werbung|publicidad|pubblicit\u00e0|advertentie)?$/i;
    const KEEP = 'html, body, main, article, section, nav, header, footer, form, ul, ol, table, tbody, tr, [role=main], [role=dialog]';
    const CONTENT = 'img, picture, video, audio, canvas, iframe, object, embed, input, button, select, textarea, a[href]';
    const slots = ADS.filter((s) => {
      try { document.createDocumentFragment().querySelector(s); return true; } catch (e) { return false; }
    }).join(',');
    const hidden = new Map();
    // Nothing in `box` but ads: the text and the media outside the slots.
    const onlyAds = (box) => {
      for (const e of box.querySelectorAll(CONTENT)) if (!e.closest(slots)) return false;
      const walk = document.createTreeWalker(box, NodeFilter.SHOW_TEXT);
      let text = '';
      for (let n = walk.nextNode(), left = 400; n; n = walk.nextNode()) {
        const p = n.parentElement;
        if (--left < 0) return false;
        if (!n.data.trim() || !p || /^(SCRIPT|STYLE|NOSCRIPT|TEMPLATE)$/.test(p.tagName) || p.closest(slots)) continue;
        text += n.data.trim();
        if (text.length > 20) return false;
      }
      return LABEL.test(text);
    };
    const put = (box) => {
      hidden.set(box, [box.style.getPropertyValue('display'), box.style.getPropertyPriority('display')]);
      box.style.setProperty('display', 'none', 'important');
    };
    const back = (box) => {
      const [value, priority] = hidden.get(box);
      hidden.delete(box);
      if (value) box.style.setProperty('display', value, priority);
      else box.style.removeProperty('display');
    };
    const collapse = () => {
      for (const box of [...hidden.keys()]) {
        if (!box.isConnected) hidden.delete(box);
        else if (!onlyAds(box)) back(box);
      }
      for (const slot of [...document.querySelectorAll(slots)].slice(0, 120)) {
        let box = slot.parentElement;
        for (let i = 0; i < 3 && box && !hidden.has(box) && !box.matches(KEEP) && !box.contains(document.activeElement); i++) {
          // Taller than any ad: a column of the page, not a wrapper.
          if (box.getBoundingClientRect().height > 900 || !onlyAds(box)) break;
          put(box);
          box = box.parentElement;
        }
      }
      // A lone label ("Advertisement") in a box as large as an ad, with nothing else in it.
      let left = 4000;
      for (const e of document.querySelectorAll('span, div, p')) {
        if (--left < 0) break;
        if (e.firstElementChild || e.textContent.length > 20) continue;
        const text = e.textContent.trim();
        if (!text || !LABEL.test(text) || e.closest('a, button, nav, footer, li, label, [role=dialog]')) continue;
        let box = e.parentElement;
        for (let i = 0; i < 3 && box && !hidden.has(box) && !box.matches(KEEP); i++, box = box.parentElement) {
          const r = box.getBoundingClientRect();
          if (r.height > 900 || !onlyAds(box)) break;
          if (r.height >= 60 && r.width >= 120) { put(box); break; }
        }
      }
    };
    const inCall = () => {
      try { return !!(window.__bhRtc && window.__bhRtc.state().call); } catch (_) { return false; }
    };
    let due = 0;
    let quiet = 0;
    const soon = () => {
      if (due) return;
      due = setTimeout(() => {
        due = 0;
        quiet = Date.now();
        // A page in a call (Meet, Teams…) changes its document all the time (tiles, captions,
        // sound levels) and shows no ads: the scan of its boxes is left out while the call
        // lasts, it cost the page every two seconds for hours (0.8.34).
        if (!document.hidden && !inCall()) collapse();
      }, Math.max(300, 2000 - (Date.now() - quiet)));
    };
    const start = () => {
      soon();
      new MutationObserver(soon).observe(document.documentElement, { childList: true, subtree: true });
      document.addEventListener('visibilitychange', soon);
    };
    if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start);
    else start();
  }

  // A hidden consent banner often leaves the page unable to scroll: give it back, once one of
  // them was actually there.
  if (SHIELDS.cookieBanners) {
    const freed = () => {
      if (!document.querySelector(BANNERS.join(','))) return false;
      for (const el of [document.documentElement, document.body]) {
        if (el && getComputedStyle(el).overflow === 'hidden') el.style.setProperty('overflow', 'auto', 'important');
      }
      return true;
    };
    let tries = 0;
    const timer = setInterval(() => {
      if (freed() || ++tries > 20) clearInterval(timer);
    }, 1000);
  }
})();
