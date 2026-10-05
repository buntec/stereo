import { useRef, useEffect, useState, useCallback } from "react";

import logger from "./logger.tsx";

type Callback<T> = (msg: T) => void;

interface BaseMessage {
  [key: string]: unknown;
}

export function useWebSocket<TMsgI, TMsgO extends BaseMessage>(
  url: string,
  protocols: string[] | undefined,
  onMessage: (msg: TMsgI | TMsgI[]) => void,
  onError: (err: Event) => void,
  onClose: (ev: CloseEvent) => void,
  onOpen?: () => void,
) {
  const ws = useRef<WebSocket>(null);
  const queue = useRef<TMsgO[]>([]);
  const heartbeatInterval = useRef<number>(null);
  const [isReady, setIsReady] = useState<boolean>(false);
  const callbacks = useRef<Map<number, Callback<TMsgI | { type: string }>>>(
    new Map(),
  );
  const idRef = useRef<number>(0);

  useEffect(() => {
    let isUnmounted = false;

    function connect() {
      ws.current = new WebSocket(url, protocols);

      ws.current.onopen = () => {
        logger.info("WebSocket connected");

        heartbeatInterval.current = setInterval(() => {
          if (ws.current && ws.current.readyState === WebSocket.OPEN) {
            ws.current.send(
              JSON.stringify({ type: "heartbeat", timestamp: Date.now() }),
            );
          }
        }, 10000);

        setIsReady(true);
        onOpen?.();
      };

      // A throwing reply callback must not stop the rest of the batch from being handled.
      const resolveCallback = (msg: { id?: unknown }) => {
        if (typeof msg.id !== "number") return;
        const cb = callbacks.current.get(msg.id);
        callbacks.current.delete(msg.id);
        try {
          cb?.(msg as TMsgI);
        } catch (err) {
          logger.error(`Reply callback for request ${msg.id} failed`, err);
        }
      };

      ws.current.onmessage = (event) => {
        let data;
        try {
          logger.debug(`Received WS message: ${event.data}`);
          data = JSON.parse(event.data);
        } catch {
          logger.warn(`Failed to parse WS message to JSON: ${event.data}`);
          return;
        }
        if (Array.isArray(data)) {
          data.forEach(resolveCallback);
        } else {
          resolveCallback(data);
        }
        onMessage(data);
      };

      ws.current.onerror = (err: Event) => {
        logger.error("WebSocket error", err);
        callbacks.current.forEach((cb) => cb({ type: "websocket-error" }));
        callbacks.current.clear();
        setIsReady(false);
        onError(err);
      };

      ws.current.onclose = (event: CloseEvent) => {
        logger.info("WebSocket closed: ", event);
        callbacks.current.forEach((cb) => cb({ type: "websocket-closed" }));
        callbacks.current.clear();
        setIsReady(false);
        onClose(event);
        if (!isUnmounted) {
          logger.info("Attempting to reconnect WebSocket...");
          setTimeout(() => connect(), 3000);
        }
      };
    }

    // if we connect immediately, the first attempt fails (for the dev server)
    const t = setTimeout(() => connect(), 500);

    return () => {
      isUnmounted = true;
      clearTimeout(t);
      if (heartbeatInterval.current) {
        clearInterval(heartbeatInterval.current);
      }
      if (ws.current) {
        logger.info("closing WebSocket");
        ws.current.close();
      }
    };
  }, [url, protocols, onClose, onError, onMessage, onOpen]);

  useEffect(() => {
    if (ws.current?.readyState === WebSocket.OPEN) {
      while (queue.current.length > 0) {
        const msg = queue.current.shift();
        ws.current.send(JSON.stringify(msg));
      }
    }
  }, [isReady]);

  const sendMsg = useCallback((o: TMsgO) => {
    queue.current.push(o);
    if (ws.current?.readyState === WebSocket.OPEN) {
      while (queue.current.length > 0) {
        const msg = queue.current.shift();
        if (msg) {
          ws.current.send(JSON.stringify(msg));
        }
      }
    }
  }, []);

  const requestReply = useCallback(
    (
      request: TMsgO,
      cb: (response: TMsgI | { type: string }) => void,
      timeout?: number,
    ) => {
      const id = ++idRef.current;
      const msgWithId = { ...request, id };
      callbacks.current.set(id, cb);

      if (timeout) {
        setTimeout(() => {
          callbacks.current.get(id)?.({ type: "timeout" });
          callbacks.current.delete(id);
        }, timeout);
      }

      sendMsg(msgWithId);
    },
    [sendMsg],
  );

  return { sendMsg, requestReply, isReady };
}
