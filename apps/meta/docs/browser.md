# Calling meta from the browser (testbed / UI)

The browser testbed (Vite, `http://localhost:5173`) runs on a different origin than meta
(`http://127.0.0.1:8090`), so the browser needs CORS — or a same-origin proxy. Pick one.

## Option A — CORS (meta answers cross-origin)

Start meta with the testbed origin allowed (`just meta` already does this for 5173 and 4173):

```sh
WP_META_CORS_ORIGINS=http://localhost:5173,http://localhost:4173 \
WP_META_INSTANCE_KEY=dev-instance-key-000 WP_META_FAST_HASH=1 cargo run -p wp_meta --bin wp-meta
```

- Comma-separated exact origins (`scheme://host[:port]`, no path). `*` allows any origin —
  dev only.
- Only the player API gets CORS. `/v1/admin/*` and the instance API (`/v1/instances/{id}/…`)
  never do: admin and instance tokens do not belong in a browser.
- Auth is `Authorization: Bearer <token>`, not cookies; keep the session token in memory or
  `sessionStorage`.

## Option B — same-origin proxy (no CORS needed)

Proxy `/v1` through the Vite dev server (in `apps/testbed-web/vite.config.ts`, owned by the
testbed workstream):

```ts
server: { port: 5173, proxy: { "/v1": "http://127.0.0.1:8090" } }
```

then call `fetch("/v1/...")` with no base URL. In a deployment, the reverse proxy that
terminates TLS does the same.

## Example: sign up, queue, wait for a seat

```ts
const META = "http://127.0.0.1:8090"; // "" with option B

async function meta<T>(path: string, init: RequestInit = {}, token?: string): Promise<T> {
  const res = await fetch(META + path, {
    ...init,
    headers: {
      "content-type": "application/json",
      ...(token ? { authorization: `Bearer ${token}` } : {}),
    },
  });
  const body = await res.json().catch(() => ({}));
  // Errors are `{ "error": "<code>", "message": "..." }` (see openapi.yaml).
  if (!res.ok) throw new Error(`${res.status} ${body.error}: ${body.message}`);
  return body as T;
}

const s = await meta<{ token: string; account_id: number }>("/v1/auth/signup", {
  method: "POST",
  body: JSON.stringify({ email: "me@example.test", password: "a-long-password", display_name: "Me" }),
});

await meta("/v1/matchmaking/queue", {
  method: "POST",
  body: JSON.stringify({ prefs: { role: "Racer", mode: "Arcade", bracket: "C", region: "eu-west" } }),
}, s.token);

let assignment;
while (!(assignment = (await meta<any>("/v1/matchmaking/status", {}, s.token)).assignment)) {
  await new Promise((r) => setTimeout(r, 1000));
}
// Connect to assignment.address and send assignment.join_token in the hello (MC-6).

const board = await meta<any>("/v1/leaderboards/clean_rating?period=weekly");
```

Shapes: `apps/meta/openapi.yaml` (served at `/openapi.yaml`).
