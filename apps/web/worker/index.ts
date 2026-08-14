/**
 * Serves the site, and forwards its JSON-RPC to an endpoint whose key the
 * browser never sees.
 *
 * The key stops being public but this endpoint does not, so the control that
 * matters is the method allowlist: an endpoint answering only what one page
 * needs is worth little to whoever finds it. The origin check is a speed bump,
 * since a header is whatever a non-browser client says it is.
 *
 * The cost is that an operator now sits between a visitor and the chain. It
 * cannot forge a signature, but it can stall or answer stale; `VITE_RPC_URL`
 * builds a copy that never comes here.
 */

export interface Env {
  /** The built site. Bound by `assets.binding` in wrangler.jsonc. */
  ASSETS: Fetcher;
  /** The real RPC, key included. A secret: `wrangler secret put UPSTREAM_RPC_URL`. */
  UPSTREAM_RPC_URL: string;
  /** Optional, comma separated. Checked only when the request carries an origin. */
  ALLOWED_ORIGINS?: string;
}

const RPC_PATH = '/rpc';

/**
 * Every method this front end calls, and nothing else. One missing comes back as
 * a refusal naming itself, so a client change that needs it says so plainly.
 */
const ALLOWED_METHODS: ReadonlySet<string> = new Set([
  // Reads.
  'getAccountInfo',
  'getMultipleAccounts',
  'getProgramAccounts',
  'getTokenAccountBalance',
  'getBalance',
  // The chain clock, for the countdowns.
  'getSlot',
  'getBlockTime',
  'getBlockHeight',
  // Sending, and waiting for what was sent.
  'getLatestBlockhash',
  'sendTransaction',
  'simulateTransaction',
  'getSignatureStatuses',
  'getTransaction',
  'getVersion',
]);

/** Generous for a batch of account reads, far under anything worth sending. */
const MAX_BODY_BYTES = 256 * 1024;

const json = (status: number, body: unknown) =>
  new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  });

/** The method names in a request, which may be a single call or a batch. */
const methodsIn = (payload: unknown): string[] | null => {
  const one = (entry: unknown): string | null =>
    entry !== null && typeof entry === 'object' && typeof (entry as { method?: unknown }).method === 'string'
      ? (entry as { method: string }).method
      : null;

  if (Array.isArray(payload)) {
    const found = payload.map(one);
    return found.every((method): method is string => method !== null) ? found : null;
  }
  const single = one(payload);
  return single === null ? null : [single];
};

const originAllowed = (request: Request, env: Env): boolean => {
  const allowed = env.ALLOWED_ORIGINS?.split(',').map((entry) => entry.trim()).filter(Boolean);
  if (!allowed || allowed.length === 0) return true;
  const origin = request.headers.get('origin');
  // Same-origin requests may carry no origin at all; there is nothing to check.
  return origin === null || allowed.includes(origin);
};

/**
 * Subscriptions, how a client learns its transaction landed. Passed through
 * unfiltered: a socket can only subscribe, so there is little left to narrow.
 */
const proxyWebsocket = async (env: Env): Promise<Response> => {
  // Kept on http(s): switching to ws(s) first is what a browser does and what
  // this runtime refuses.
  const response = await fetch(env.UPSTREAM_RPC_URL, { headers: { Upgrade: 'websocket' } });
  const socket = response.webSocket;
  if (!socket) return new Response('the upstream refused a websocket', { status: 502 });
  // Handed on, not accepted: accepting would make this worker read the frames.
  return new Response(null, { status: 101, webSocket: socket });
};

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    if (url.pathname !== RPC_PATH) return env.ASSETS.fetch(request);

    if (!env.UPSTREAM_RPC_URL) {
      return json(500, { error: 'UPSTREAM_RPC_URL is not set on this worker' });
    }
    if (!originAllowed(request, env)) return json(403, { error: 'origin not allowed' });

    if (request.headers.get('upgrade')?.toLowerCase() === 'websocket') {
      return proxyWebsocket(env);
    }
    if (request.method !== 'POST') {
      return json(405, { error: 'this endpoint takes JSON-RPC over POST' });
    }

    const body = await request.text();
    if (body.length > MAX_BODY_BYTES) return json(413, { error: 'request too large' });

    let payload: unknown;
    try {
      payload = JSON.parse(body);
    } catch {
      return json(400, { error: 'body is not JSON' });
    }

    const methods = methodsIn(payload);
    if (methods === null) return json(400, { error: 'body is not JSON-RPC' });
    const refused = methods.find((method) => !ALLOWED_METHODS.has(method));
    if (refused !== undefined) {
      return json(403, { error: `method not allowed through this endpoint: ${refused}` });
    }

    const upstream = await fetch(env.UPSTREAM_RPC_URL, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body,
    });
    return new Response(upstream.body, {
      status: upstream.status,
      headers: { 'content-type': 'application/json' },
    });
  },
} satisfies ExportedHandler<Env>;
