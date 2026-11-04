// Lets the shell see and drive the camera and microphone a page opened (video calls), and tells
// it when a video leaves Picture in Picture (the "back to tab" button of the floating window).
(() => {
  if (!/^https?:$/.test(location.protocol)) return;
  const post = (message) => {
    try {
      window.ipc && window.ipc.postMessage(JSON.stringify(message));
    } catch (_) {}
  };

  const tracks = new Set();
  const md = navigator.mediaDevices;
  if (md && md.getUserMedia) {
    const original = md.getUserMedia.bind(md);
    md.getUserMedia = async (constraints) => {
      const stream = await original(constraints);
      stream.getTracks().forEach((t) => tracks.add(t));
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

  const left = (video) => post({ cmd: 'pip_left', paused: !!video.paused });
  document.addEventListener(
    'leavepictureinpicture',
    (e) => e.target instanceof HTMLVideoElement && left(e.target),
    true
  );
  document.addEventListener(
    'webkitpresentationmodechanged',
    (e) => {
      const v = e.target;
      if (!(v instanceof HTMLVideoElement)) return;
      if (v.webkitPresentationMode === 'picture-in-picture') v.__bhPip = true;
      else if (v.__bhPip) {
        v.__bhPip = false;
        left(v);
      }
    },
    true
  );
})();
