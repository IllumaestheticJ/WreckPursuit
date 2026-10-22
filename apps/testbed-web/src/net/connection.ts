// WebSocket transport for the net client. Binary frames only: one frame = one protocol
// message. Both protocol channels map onto this one reliable stream (prototype); a
// WebTransport implementation can replace this class without touching the rest.
import * as net from "./wasm";

export type ConnState = "connecting" | "open" | "closed";

export class Connection {
  state: ConnState = "connecting";
  private ws: WebSocket | null = null;
  private reconnectUntil = 0;

  constructor(private url: string, private onState: (s: ConnState) => void) {}

  /** Open the socket; `token` reclaims our player within the server's 30 s window. A meta
   * matchmaking join token (MC-6) can be passed as `?join=<token>` in the page URL. */
  open(token = ""): void {
    const joinToken = token ? "" : new URLSearchParams(location.search).get("join") ?? "";
    const ws = new WebSocket(this.url);
    ws.binaryType = "arraybuffer";
    this.ws = ws;
    this.setState("connecting");
    ws.onopen = () => {
      net.connect(performance.now(), token, joinToken);
      this.setState("open");
      this.flush();
    };
    ws.onmessage = (ev) => {
      if (ev.data instanceof ArrayBuffer) {
        try {
          net.receive(new Uint8Array(ev.data), performance.now());
        } catch (e) {
          console.warn("bad frame", e);
        }
      }
    };
    ws.onclose = () => {
      this.setState("closed");
      this.ws = null;
      this.scheduleReconnect();
    };
  }

  /** Send everything the client queued. */
  flush(): void {
    const frames = net.takeOutgoing();
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) return;
    for (const f of frames) this.ws.send(f);
  }

  /** Drop the socket as a network failure would (test hook): the client reconnects with
   * its token inside the server's 30 s window and keeps its car. */
  dropForTest(): void {
    this.ws?.close();
  }

  close(): void {
    this.reconnectUntil = 0;
    net.bye(performance.now());
    this.flush();
    this.ws?.close();
  }

  private scheduleReconnect(): void {
    const token = net.status().token;
    if (!token || token === "0") return;
    if (this.reconnectUntil === 0) this.reconnectUntil = performance.now() + 30_000;
    if (performance.now() > this.reconnectUntil) return;
    setTimeout(() => this.open(token), 1000);
  }

  private setState(s: ConnState): void {
    this.state = s;
    if (s === "open") this.reconnectUntil = 0;
    this.onState(s);
  }
}
