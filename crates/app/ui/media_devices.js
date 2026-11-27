// Lets the shell see and drive the camera and microphone a page opened (video calls), and tells
// it when a video leaves Picture in Picture (the "back to tab" button of the floating window).
(() => {
  if (!/^https?:$/.test(location.protocol)) return;
  // Document editors are left untouched: they check their environment very strictly.
  if (/^(docs|drive|sheets|slides|script)\.google\.com$/.test(location.hostname)) return;
  const post = (message) => {
    try {
      window.__bhipc && window.__bhipc.postMessage(JSON.stringify(message));
    } catch (_) {}
  };

  // The page's own Media Session play and pause (its player's buttons): the sidebar card uses
  // them before touching the element, since a player that thinks it is paused (YouTube) pauses
  // again a video started behind its back (0.8.26). Kept out of sight in `__bhMs`.
  const session = navigator.mediaSession;
  const setAction = session && session.setActionHandler ? session.setActionHandler.bind(session) : null;
  const own = {};
  if (setAction) {
    try {
      Object.defineProperty(window, '__bhMs', { value: own });
      session.setActionHandler = function setActionHandler(action, handler) {
        if (action === 'play' || action === 'pause') own[action] = typeof handler === 'function' ? handler : null;
        return setAction(action, handler);
      };
    } catch (_) {}
  }

  // The shell only probes the tabs that say something may have changed (and those that play
  // or are in a call): at most one message a second, the shell then looks for a few seconds.
  let hinted = 0;
  const hint = () => {
    const now = Date.now();
    if (now - hinted < 1000) return;
    hinted = now;
    post({ cmd: 'media' });
  };
  // Media events do not bubble, but they are seen on the way down.
  for (const type of ['play', 'playing', 'pause', 'ended', 'emptied', 'volumechange', 'loadedmetadata', 'enterpictureinpicture']) {
    document.addEventListener(type, hint, true);
  }

  // Held weakly (0.8.34): a track the page lets go of without stopping it (Meet asks for the
  // microphone several times before a call, WebKit counted 7 capture sessions for one microphone
  // during a whole call) is the engine's to collect, not kept alive here for hours.
  const tracks = new Set();
  const Weak = window.WeakRef;
  const hold = (t) => tracks.add(Weak ? new Weak(t) : { deref: () => t });
  const held = () => {
    const list = [];
    for (const ref of tracks) {
      const t = ref.deref();
      if (!t || t.readyState === 'ended') tracks.delete(ref);
      else list.push(t);
    }
    return list;
  };
  const md = navigator.mediaDevices;
  if (md && md.getUserMedia) {
    const original = md.getUserMedia.bind(md);
    md.getUserMedia = async (constraints) => {
      const stream = await original(constraints);
      stream.getTracks().forEach(hold);
      hint();
      return stream;
    };
  }
  // Screen shares: the old `setParameters` wrapper (until 0.8.27) forced 3 Mb/s and full
  // resolution on every layer, against the call's own congestion control. Bitrate and layers
  // stay the page's. WebKit only (macOS, 0.8.33): it does not tell its encoder that a screen is
  // shared, so under load it lowers the resolution like for a camera and the text comes out
  // blocky whatever the page asks (Meet at 720p). A shared screen keeps its resolution and drops
  // frames instead, as Chromium does on its own: `contentHint` "detail" and
  // `degradationPreference` "maintain-resolution", unless the page chose them.
  const screens = new WeakSet();
  const webkit = !!(window.webkit && window.webkit.messageHandlers) && !window.chrome;
  const Sender = window.RTCRtpSender && RTCRtpSender.prototype;
  const screenOf = (sender) => {
    try { return !!(sender && sender.track && screens.has(sender.track)); } catch (_) { return false; }
  };
  const keepSharp = (params) => (params && !params.degradationPreference ? { ...params, degradationPreference: 'maintain-resolution' } : params);
  const setParams = Sender && Sender.setParameters;
  // Each sender once per screen track it carries.
  const tuned = new WeakMap();
  const tune = (sender) => {
    if (!screenOf(sender) || tuned.get(sender) === sender.track || !Sender.getParameters) return;
    tuned.set(sender, sender.track);
    try {
      const params = sender.getParameters();
      if (params && !params.degradationPreference) setParams.call(sender, keepSharp(params)).catch(() => {});
    } catch (_) {}
  };
  const tuneAll = () => {
    for (const pc of peers) {
      try { pc.getSenders().forEach(tune); } catch (_) {}
    }
  };
  if (webkit && md && md.getDisplayMedia && setParams) {
    const display = md.getDisplayMedia.bind(md);
    md.getDisplayMedia = async (constraints) => {
      const stream = await display(constraints);
      stream.getVideoTracks().forEach((t) => {
        screens.add(t);
        try { if (!t.contentHint) t.contentHint = 'detail'; } catch (_) {}
      });
      // The page attaches it to its call right after: looked for a few times.
      for (const wait of [0, 1000, 3000]) setTimeout(tuneAll, wait);
      return stream;
    };
    Sender.setParameters = function setParameters(params, ...rest) {
      return setParams.call(this, screenOf(this) ? keepSharp(params) : params, ...rest);
    };
    if (Sender.replaceTrack) {
      const replaceTrack = Sender.replaceTrack;
      Sender.replaceTrack = function (track, ...rest) {
        return replaceTrack.call(this, track, ...rest).then((r) => { tune(this); return r; });
      };
    }
  }
  const live = (kind) => held().filter((t) => t.kind === kind);
  const peers = new Set();
  const NativePeer = window.RTCPeerConnection;
  if (NativePeer) {
    window.RTCPeerConnection = new Proxy(NativePeer, {
      construct(target, args, newTarget) {
        const pc = Reflect.construct(target, args, newTarget);
        peers.add(pc);
        hint();
        return pc;
      },
    });
  }
  const inCall = () => {
    for (const pc of peers) if (pc.connectionState === 'closed') peers.delete(pc);
    return [...peers].some((pc) => pc.connectionState === 'connected' || pc.connectionState === 'connecting');
  };
  window.__bhRtc = {
    state() {
      const cam = live('video');
      const mic = live('audio');
      return {
        cam: cam.length > 0,
        mic: mic.length > 0,
        call: inCall(),
        camOn: cam.some((t) => t.enabled),
        micOn: mic.some((t) => t.enabled),
      };
    },
    toggle(kind) {
      const list = live(kind);
      const enable = !list.some((t) => t.enabled);
      list.forEach((t) => (t.enabled = enable));
    },
    // Leaves the call (the sidebar card's close button): the page's own leave button when one is
    // found (Meet, Teams, Jitsi…), so it shows its own "you left" screen; then, if the call is
    // still up, the camera and microphone are released and the connections closed.
    hangUp() {
      const LEAVE = /^(leave call|leave|hang up|end call|quitter l.appel|quitter|raccrocher)$/i;
      const button = document.querySelector('[jsname="CQylAd"], [data-tid="hangup-main-btn"], #hangup-button, .hangup-button') ||
        [...document.querySelectorAll('button[aria-label], [role="button"][aria-label]')].find((b) => LEAVE.test(b.getAttribute('aria-label').trim()));
      if (button) button.click();
      setTimeout(() => {
        if (!inCall() && !live('audio').length && !live('video').length) return;
        for (const t of held()) t.stop();
        tracks.clear();
        for (const pc of peers) try { pc.close(); } catch (_) {}
        peers.clear();
        hint();
      }, button ? 1500 : 0);
    },
  };

  // Paused while it floats: the window's close button (it pauses the video first). Left playing:
  // its "back to tab" button. `paused` read when it leaves cannot tell: WebKit pauses a muted
  // video (the tiles of a call) that comes back into a page not on screen, on its way in, and
  // that `pause` event only comes after the video has left. So pauses are noted as they happen.
  const video = (e) => (e.target instanceof HTMLVideoElement ? e.target : null);
  document.addEventListener('pause', (e) => { const v = video(e); if (v && v.__bhPip) v.__bhPaused = true; }, true);
  document.addEventListener('play', (e) => { const v = video(e); if (v) v.__bhPaused = false; }, true);
  // A live stream (Twitch: endless duration) floats without a pause button in Chromium's window
  // unless the page answers the Media Session's play and pause: they are answered for the time
  // it floats (0.8.25), then the page's own handlers are put back (0.8.26).
  // The window draws play or pause from `playbackState`, which must follow the video: set once,
  // it stayed "playing" after a pause, and its button paused again instead of resuming (0.8.26).
  let sessionFor = null;
  let stateBefore = 'none';
  const follow = (e) => {
    if (e.target === sessionFor) {
      try {
        session.playbackState = sessionFor.paused ? 'paused' : 'playing';
      } catch (_) {}
    }
  };
  document.addEventListener('play', follow, true);
  document.addEventListener('pause', follow, true);
  const sessionActions = (v) => {
    if (!setAction) return;
    try {
      if (v && !sessionFor) stateBefore = session.playbackState;
      setAction('play', v ? () => v.play().catch(() => {}) : own.play || null);
      setAction('pause', v ? () => v.pause() : own.pause || null);
      session.playbackState = v ? (v.paused ? 'paused' : 'playing') : stateBefore;
    } catch (_) {}
    sessionFor = v;
  };
  document.addEventListener(
    'enterpictureinpicture',
    (e) => {
      const v = video(e);
      // Live: no end (Infinity), a placeholder of days (some live players), or a stream.
      if (v && !('webkitPresentationMode' in v) && (!isFinite(v.duration) || v.duration > 172800 || v.srcObject)) sessionActions(v);
    },
    true
  );
  document.addEventListener(
    'leavepictureinpicture',
    (e) => {
      if (sessionFor && video(e) === sessionFor) sessionActions(null);
    },
    true
  );
  document.addEventListener(
    'leavepictureinpicture',
    (e) => {
      const v = video(e);
      // WebKit already said so below; elsewhere this is the only event. Chromium pauses the
      // video before this event when the window is closed, but cannot pause a live stream
      // (`srcObject`): for those the two buttons look the same, and the shell is told.
      if (v && !('webkitPresentationMode' in v)) post({ cmd: 'pip_left', paused: !!v.paused, stream: !!v.srcObject });
    },
    true
  );
  document.addEventListener(
    'webkitpresentationmodechanged',
    (e) => {
      const v = video(e);
      if (!v) return;
      hint();
      if (v.webkitPresentationMode === 'picture-in-picture') {
        v.__bhPip = true;
        v.__bhPaused = !!v.paused;
      } else if (v.__bhPip) {
        v.__bhPip = false;
        post({ cmd: 'pip_left', paused: !!v.__bhPaused });
      }
    },
    true
  );
})();
