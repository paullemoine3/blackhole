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

  const tracks = new Set();
  const md = navigator.mediaDevices;
  if (md && md.getUserMedia) {
    const original = md.getUserMedia.bind(md);
    md.getUserMedia = async (constraints) => {
      const stream = await original(constraints);
      stream.getTracks().forEach((t) => tracks.add(t));
      hint();
      return stream;
    };
  }
  if (md && md.getDisplayMedia) {
    const display = md.getDisplayMedia.bind(md);
    // Screen shares favour sharpness over frame rate unless the page chose otherwise.
    md.getDisplayMedia = async (constraints) => {
      const stream = await display(constraints);
      stream.getVideoTracks().forEach((t) => {
        if (!t.contentHint) t.contentHint = 'detail';
      });
      return stream;
    };
  }
  if (window.RTCRtpSender && RTCRtpSender.prototype.setParameters) {
    const setParameters = RTCRtpSender.prototype.setParameters;
    // A screen share keeps its resolution (text stays readable) and may use more bandwidth.
    RTCRtpSender.prototype.setParameters = function (params) {
      const track = this.track;
      if (track && track.kind === 'video' && track.contentHint === 'detail' && params) {
        try {
          const tuned = { ...params, degradationPreference: 'maintain-resolution' };
          tuned.encodings = (params.encodings || []).map((e) => ({
            ...e,
            scaleResolutionDownBy: 1,
            maxBitrate: Math.max(e.maxBitrate || 0, 3000000),
          }));
          return setParameters.call(this, tuned).catch(() => setParameters.call(this, params));
        } catch (_) {}
      }
      return setParameters.call(this, params);
    };
  }
  const live = (kind) => {
    for (const t of tracks) if (t.readyState === 'ended') tracks.delete(t);
    return [...tracks].filter((t) => t.kind === kind);
  };
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
  };

  // Paused while it floats: the window's close button (it pauses the video first). Left playing:
  // its "back to tab" button. `paused` read when it leaves cannot tell: WebKit pauses a muted
  // video (the tiles of a call) that comes back into a page not on screen, on its way in, and
  // that `pause` event only comes after the video has left. So pauses are noted as they happen.
  const video = (e) => (e.target instanceof HTMLVideoElement ? e.target : null);
  document.addEventListener('pause', (e) => { const v = video(e); if (v && v.__bhPip) v.__bhPaused = true; }, true);
  document.addEventListener('play', (e) => { const v = video(e); if (v) v.__bhPaused = false; }, true);
  document.addEventListener(
    'leavepictureinpicture',
    (e) => {
      const v = video(e);
      // WebKit already said so below; elsewhere this is the only event.
      if (v && !('webkitPresentationMode' in v)) post({ cmd: 'pip_left', paused: !!v.paused });
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
