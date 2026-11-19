/**
 * Web 版 `@tauri-apps/api/event`：用一条共享的 SSE 连接（`api/events`）接收后端事件。
 * 目前后端推 `usage-log-recorded`（用量有更新）和 `session-index-status`（正文索引进度），
 * 其他事件名订阅后不会收到消息。
 */

export type UnlistenFn = () => void;
export interface Event<T> {
  event: string;
  id: number;
  payload: T;
}
export type EventCallback<T> = (event: Event<T>) => void;

type Handler = (event: Event<unknown>) => void;

const handlers = new Map<string, Set<Handler>>();
let source: EventSource | null = null;
let nextId = 1;

/** 退出登录 / 会话失效时关闭事件流；重新登录后下一次 listen 会重新建立 */
export function closeEventSource() {
  source?.close();
  source = null;
}

function ensureSource() {
  if (typeof EventSource === "undefined") return;
  // 未登录时连接会被 401 拒绝并进入 CLOSED，不会自动重连：下次订阅时重建
  if (source && source.readyState !== EventSource.CLOSED) return;
  source = new EventSource("api/events", { withCredentials: true });
  source.onmessage = (message) => {
    let parsed: { event?: string; payload?: unknown };
    try {
      parsed = JSON.parse(message.data);
    } catch {
      return;
    }
    if (!parsed.event) return;
    const set = handlers.get(parsed.event);
    if (!set) return;
    const event: Event<unknown> = {
      event: parsed.event,
      id: nextId++,
      payload: parsed.payload ?? null,
    };
    for (const handler of set) handler(event);
  };
}

export async function listen<T>(
  eventName: string,
  handler: EventCallback<T>,
): Promise<UnlistenFn> {
  ensureSource();
  let set = handlers.get(eventName);
  if (!set) {
    set = new Set();
    handlers.set(eventName, set);
  }
  const wrapped = handler as Handler;
  set.add(wrapped);
  return () => {
    set?.delete(wrapped);
  };
}

export async function once<T>(
  eventName: string,
  handler: EventCallback<T>,
): Promise<UnlistenFn> {
  const off = await listen<T>(eventName, (event) => {
    off();
    handler(event);
  });
  return off;
}

export async function emit(): Promise<void> {}
