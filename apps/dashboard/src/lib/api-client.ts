// The one place in the frontend that knows how to reach the backend.
//
// Every command used to leave through `invoke(name, args)` and every live
// notification arrived through `listen(event, handler)`. Both now travel over
// plain HTTP against the server that also serves these assets:
//
//   - `call(name, args)`      → `POST /api/{name}`, the argument object as the
//                               JSON body, the command's own JSON value back.
//   - `subscribe(event, fn)`  → one shared `EventSource` on `/api/events`,
//                               fanned out by the SSE event name.
//
// The dashboard guards already forbid a component from calling the transport
// directly, so this module has exactly the reach the old one had: the wrappers
// under `lib/` and `api/`, nothing else.
//
// The rejection shape is preserved on purpose. A command returning `Err(msg)`
// used to reject with `msg`; the server answers `{ "error": msg }` with a 4xx,
// and `call` throws an `ApiError` whose `message` IS that string — so the
// `e instanceof Error ? e.message : String(e)` reads scattered across the
// pages keep rendering the same text.

/** Path prefix every route shares. Relative on purpose: the server that
 *  answers these is the one that served the page, and `pnpm dev` proxies the
 *  prefix across (see `vite.config.ts`), so no host is ever hardcoded. */
const API_PREFIX = "/api";

/** Status used for a request that never reached the server at all (the
 *  process is down, or the network dropped mid-flight). Distinguishes a
 *  transport failure from a command that ran and returned `Err`. */
export const TRANSPORT_FAILED = 0;

/**
 * A command that did not produce a value.
 *
 * `message` carries the backend's own error string, so callers that only read
 * `.message` behave exactly as they did against the previous transport;
 * `status` and `command` are additive, for the few call sites that want to
 * tell "the server is not running" apart from "this call was rejected".
 */
export class ApiError extends Error {
  readonly status: number;
  readonly command: string;

  constructor(command: string, status: number, message: string) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.command = command;
  }
}

/** Pull the backend's error string out of a failed response body, falling
 *  back to the status line when the body is not the `{ error }` envelope
 *  (a proxy's own 502 page, say). */
function errorMessage(body: string, response: Response): string {
  try {
    const parsed: unknown = JSON.parse(body);
    if (parsed && typeof parsed === "object" && "error" in parsed) {
      const message = (parsed as { error: unknown }).error;
      if (typeof message === "string" && message) return message;
    }
  } catch {
    // Not JSON — fall through to the status line below.
  }
  return `HTTP ${response.status} ${response.statusText}`.trim();
}

/**
 * Run one backend command and resolve with its JSON value.
 *
 * `args` keys stay camelCase (`repoPath`, `specName`): the server maps each
 * one onto the snake_case parameter it names, so the serialization contract
 * the guards describe is unchanged.
 */
export async function call<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  let response: Response;
  try {
    response = await fetch(`${API_PREFIX}/${command}`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(args ?? {}),
    });
  } catch (cause) {
    throw new ApiError(
      command,
      TRANSPORT_FAILED,
      `cannot reach the dashboard server: ${
        cause instanceof Error ? cause.message : String(cause)
      }`,
    );
  }

  const body = await response.text();
  if (!response.ok) {
    throw new ApiError(command, response.status, errorMessage(body, response));
  }
  // A command returning `()` answers with the JSON `null`, which is what the
  // `Promise<void>` wrappers already resolved with.
  if (!body) return null as T;
  try {
    return JSON.parse(body) as T;
  } catch (cause) {
    throw new ApiError(
      command,
      response.status,
      `malformed response body: ${
        cause instanceof Error ? cause.message : String(cause)
      }`,
    );
  }
}

// ---------------------------------------------------------------------------
// GET /api/events
// ---------------------------------------------------------------------------

/** Where the live stream lives. One connection serves every event name. */
const EVENTS_URL = `${API_PREFIX}/events`;

type Handler = (payload: never) => void;

/** The open stream, or `null` while nothing is subscribed. Opened on the
 *  first subscription and closed again when the last one is dropped, so a
 *  full unmount leaves no socket behind. */
let source: EventSource | null = null;

/** Subscribers per SSE event name, plus the single DOM listener that feeds
 *  them. Kept together so unsubscribing can detach the listener once a name
 *  has no readers left. */
const channels = new Map<
  string,
  { handlers: Set<Handler>; listener: (event: MessageEvent<string>) => void }
>();

/** Open the stream if it is not already open. `EventSource` reconnects on its
 *  own after a drop, which is the reason this transport was chosen: the
 *  operator reaches the dashboard over the network, where drops happen. */
function ensureSource(): EventSource {
  if (!source) source = new EventSource(EVENTS_URL);
  return source;
}

/** Close the stream once nothing is listening on any name. */
function closeIfIdle(): void {
  if (channels.size > 0 || !source) return;
  source.close();
  source = null;
}

/**
 * Subscribe to one named server-sent event and return the unsubscribe.
 *
 * Handlers receive the already-parsed `data` payload — the same value that
 * used to arrive as `{ payload }`. A frame whose body does not parse is
 * dropped rather than thrown: the stream must survive one bad frame, and the
 * queries carry their own refetch fallback for the update it cost.
 */
export function subscribe<T>(event: string, handler: (payload: T) => void): () => void {
  let channel = channels.get(event);
  if (!channel) {
    const handlers = new Set<Handler>();
    const listener = (message: MessageEvent<string>) => {
      let payload: T;
      try {
        payload = JSON.parse(message.data) as T;
      } catch {
        return;
      }
      for (const fn of handlers) (fn as (payload: T) => void)(payload);
    };
    channel = { handlers, listener };
    channels.set(event, channel);
    ensureSource().addEventListener(event, listener as EventListener);
  }
  channel.handlers.add(handler as Handler);

  return () => {
    const open = channels.get(event);
    if (!open) return;
    open.handlers.delete(handler as Handler);
    if (open.handlers.size > 0) return;
    source?.removeEventListener(event, open.listener as EventListener);
    channels.delete(event);
    closeIfIdle();
  };
}
