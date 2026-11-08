// Page clean-up, Brave-style "shields", injected at document start in every frame of a page
// tab when switched on in Settings > Privacy (the blocked requests are page_guard.js's job):
// - hide ad slots and the space they leave (generic, well-known ad containers only);
// - skip video ads (YouTube): jump to their end and press "Skip";
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

  // YouTube video ads: the player plays them in the same <video>, marked by `.ad-showing`.
  if (SHIELDS.videoAds && /(^|\.)youtube\.com$/.test(location.hostname)) {
    setInterval(() => {
      const player = document.querySelector('.html5-video-player.ad-showing');
      if (!player) return;
      const video = player.querySelector('video');
      // The ad and the video share this element: jump to the ad's end, never touch the sound.
      if (video && isFinite(video.duration) && video.duration > 0) video.currentTime = video.duration;
      const skip = player.querySelector('.ytp-ad-skip-button, .ytp-ad-skip-button-modern, .ytp-skip-ad-button');
      if (skip) skip.click();
    }, 400);
  }
})();
