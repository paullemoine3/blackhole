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
  ];
  const BANNERS = [
    '#onetrust-banner-sdk', '#onetrust-consent-sdk', '#CybotCookiebotDialog', '#CybotCookiebotDialogBodyUnderlay',
    '#qc-cmp2-container', '.qc-cmp2-container', '.fc-consent-root', '#didomi-host', '#didomi-popup',
    '#usercentrics-root', '#truste-consent-track', '.truste_box_overlay', '#cookie-law-info-bar',
    '#cmpbox', '#cmpbox2', '#cmpwrapper', '[id^="sp_message_container"]', '.cc-window.cc-banner',
    '#iubenda-cs-banner', '#axeptio_overlay', '#tarteaucitronRoot', '#cookiescript_injected',
  ];

  const css = [];
  if (SHIELDS.hideAds) css.push(ADS.join(',\n') + ' { display: none !important; }');
  if (SHIELDS.cookieBanners) css.push(BANNERS.join(',\n') + ' { display: none !important; }');
  if (css.length) {
    try {
      const sheet = new CSSStyleSheet();
      sheet.replaceSync(css.join('\n'));
      document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
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
