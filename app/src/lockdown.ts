// The lockdown that runs first inside every plugin worker (JavaScript plugins
// and the SketchUp Ruby runtime): no network, no more code, and no way to
// start another worker (a new worker would begin without any of this).
// Kept as a string because it runs inside the worker; checks/run.mjs runs it
// against a pretend global to prove it does what it says.

export const LOCKDOWN_SOURCE = `
for (const k of ["fetch", "XMLHttpRequest", "WebSocket", "WebSocketStream", "WebTransport", "EventSource", "importScripts", "indexedDB", "caches", "Worker", "SharedWorker", "BroadcastChannel", "Request", "RTCPeerConnection", "fetchLater"]) {
  try { Object.defineProperty(self, k, { value: undefined, writable: false, configurable: false }); } catch {}
}
try { Object.defineProperty(self.navigator, "sendBeacon", { value: undefined, configurable: false }); } catch {}
`;
