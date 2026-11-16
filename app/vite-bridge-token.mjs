// Dev-only Vite plugin: hands the dev page the bridge's per-launch token.
// Plain JS (not part of the app's type-checked sources: it uses Node APIs).
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

/** Where `sketchcraft bridge` writes its token (beside the live model). */
function tokenFile() {
  if (process.env.SKETCHCRAFT_BRIDGE_TOKEN_FILE) return process.env.SKETCHCRAFT_BRIDGE_TOKEN_FILE;
  const doc = process.env.SKETCHCRAFT_DOC ?? path.join(os.homedir(), "Documents/SketchCraft/live.sketchcraft");
  return path.join(path.dirname(doc), ".bridge-token");
}

/**
 * The bridge refuses requests without its token. The token file is readable
 * only by you; the page reads it again if the bridge is restarted. Only
 * same-origin requests from the dev page itself are answered.
 */
export default function bridgeToken(port) {
  return {
    name: "sketchcraft-bridge-token",
    configureServer(server) {
      server.middlewares.use("/__bridge-token", (req, res) => {
        const host = String(req.headers.host ?? "");
        const site = req.headers["sec-fetch-site"];
        const ok = (host === `127.0.0.1:${port}` || host === `localhost:${port}`) && (site === undefined || site === "same-origin" || site === "none");
        if (!ok) {
          res.statusCode = 403;
          return res.end();
        }
        let token = process.env.SKETCHCRAFT_BRIDGE_TOKEN ?? "";
        if (!token) {
          try {
            token = fs.readFileSync(tokenFile(), "utf8").trim();
          } catch {
            /* the bridge isn't running yet */
          }
        }
        res.setHeader("Cache-Control", "no-store");
        res.setHeader("Content-Type", "text/plain");
        res.end(token);
      });
    },
  };
}

/**
 * The dev page gets the same kind of content security policy the desktop app
 * has (app/src-tauri/tauri.conf.json), so a plugin behaves the same in both:
 * it can't load code or reach any address but this server and the bridge.
 * (The Vite client needs inline scripts and a websocket back to this server.)
 */
export function devCsp(port, bridgePort) {
  const policy = [
    "default-src 'self'",
    "script-src 'self' 'unsafe-inline'",
    "style-src 'self' 'unsafe-inline'",
    "img-src 'self' data: blob:",
    "media-src 'self' data: blob:",
    "font-src 'self' data:",
    `connect-src 'self' ws://127.0.0.1:${port} ws://localhost:${port} http://127.0.0.1:${bridgePort}`,
    "worker-src 'self' blob:",
    "object-src 'none'",
    "base-uri 'self'",
    "form-action 'none'",
  ].join("; ");
  return {
    name: "sketchcraft-dev-csp",
    apply: "serve",
    transformIndexHtml: () => [{ tag: "meta", attrs: { "http-equiv": "Content-Security-Policy", content: policy }, injectTo: "head-prepend" }],
  };
}
