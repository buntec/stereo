import { useEffect, useRef, useState } from "react";

export interface YouTubeOptions {
  playerVars?: YT.PlayerVars;
  onReady?: (event: YT.PlayerEvent) => void;
  onStateChange?: (event: YT.OnStateChangeEvent) => void;
}

declare global {
  interface Window {
    onYouTubeIframeAPIReady: () => void;
    YT: typeof YT;
  }
}

interface UseYouTubeReturn {
  containerRef: React.RefObject<HTMLDivElement | null>;
  player: YT.Player | null;
  isReady: boolean;
}

// Shared state outside the hook instance
let apiLoadingStatus: "uninitialized" | "loading" | "ready" = "uninitialized";

export const useYTPlayer = (options: YouTubeOptions = {}): UseYouTubeReturn => {
  const [player, setPlayer] = useState<YT.Player | null>(null);
  const [isReady, setIsReady] = useState<boolean>(false);

  // The hook now owns the reference to the DOM element
  const containerRef = useRef<HTMLDivElement>(null);
  const playerRef = useRef<YT.Player | null>(null);

  useEffect(() => {
    const initPlayer = () => {
      // Ensure the div exists and the API is ready
      if (!containerRef.current || !window.YT || !window.YT.Player) return;

      playerRef.current = new window.YT.Player(containerRef.current, {
        playerVars: options.playerVars,
        events: {
          onReady: (event: YT.PlayerEvent) => {
            setIsReady(true);
            options.onReady?.(event);
          },
          onStateChange: (event: YT.OnStateChangeEvent) => {
            options.onStateChange?.(event);
          },
        },
      });
      setPlayer(playerRef.current);
    };

    if (window.YT && window.YT.Player) {
      initPlayer();
    } else {
      window.addEventListener("youtube-api-ready", initPlayer);

      if (apiLoadingStatus === "uninitialized") {
        apiLoadingStatus = "loading";

        window.onYouTubeIframeAPIReady = () => {
          apiLoadingStatus = "ready";
          window.dispatchEvent(new CustomEvent("youtube-api-ready"));
        };

        const tag = document.createElement("script");
        tag.src = "https://www.youtube.com/iframe_api";
        document.head.appendChild(tag);
      }
    }

    return () => {
      window.removeEventListener("youtube-api-ready", initPlayer);
      if (playerRef.current) {
        playerRef.current.destroy();
        playerRef.current = null;
      }
    };
  }, [options]);

  return { containerRef, player, isReady };
};
