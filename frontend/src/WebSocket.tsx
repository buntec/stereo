import { useRef, useEffect, useState, useCallback } from "react";

type Callback<T> = (msg: T) => void;

interface BaseMessage {
  [key: string]: any;
}

export function useWebSocket<TMsgI, TMsgO extends BaseMessage>(
  url: string,
  onMessage: (msg: TMsgI | TMsgI[]) => void,
  onError: (err: Event) => void,
  onClose: (ev: CloseEvent) => void,
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
      ws.current = new WebSocket(url);

      ws.current.onopen = () => {
        console.log("WebSocket connected");

        // Start heartbeat
        heartbeatInterval.current = setInterval(() => {
          if (ws.current && ws.current.readyState === WebSocket.OPEN) {
            ws.current.send(
              JSON.stringify({ type: "heartbeat", timestamp: Date.now() }),
            );
          }
        }, 10000);

        setIsReady(true);
      };

      ws.current.onmessage = (event) => {
        try {
          // console.info(`Received WS message: ${event.data}`)
          const data = JSON.parse(event.data);
          if (Object.hasOwn(data, "id")) {
            callbacks.current.get(data.id)?.(data);
            callbacks.current.delete(data.id);
          }
          if (Array.isArray(data)) {
            data.forEach((msg) => {
              if (Object.hasOwn(msg, "id")) {
                callbacks.current.get(msg.id)?.(msg);
                callbacks.current.delete(msg.id);
              }
            });
          }
          onMessage(data);
        } catch (error) {
          console.warn(`Failed to parse WS message to JSON: ${event.data}`);
        }
      };

      ws.current.onerror = (err: Event) => {
        console.error("WebSocket error", err);
        callbacks.current.forEach((cb) => cb({ type: "websocket-error" }));
        callbacks.current.clear();
        setIsReady(false);
        onError(err);
      };

      ws.current.onclose = (event) => {
        console.log("WebSocket closed: ", event);
        callbacks.current.forEach((cb) => cb({ type: "websocket-closed" }));
        callbacks.current.clear();
        setIsReady(false);
        onClose(event);
        if (!isUnmounted) {
          console.log("Attempting to reconnect WebSocket...");
          setTimeout(() => connect(), 3000);
        }
      };
    }

    // if we connect immediately, the first attempt fails (for the dev server)
    const t = setTimeout(() => connect(), 500);

    return () => {
      console.log("useWS cleanup");
      isUnmounted = true;
      clearTimeout(t);
      if (heartbeatInterval.current) {
        clearInterval(heartbeatInterval.current);
      }
      if (ws.current) {
        console.log("closing WS");
        ws.current.close();
      }
    };
  }, [url]);

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
