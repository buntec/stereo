import { useState, useEffect } from "react";
import type { ICollectionStats } from "./Types.tsx";

export function shuffleArray<T>(array: T[]): T[] {
  for (let i = array.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));

    [array[i], array[j]] = [array[j], array[i]];
  }
  return array;
}

export function formatDuration(totalSeconds: number): string {
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = Math.floor(totalSeconds % 60);

  const paddedMinutes = String(minutes).padStart(2, "0");
  const paddedSeconds = String(seconds).padStart(2, "0");

  return `${paddedMinutes}:${paddedSeconds}`;
}

export function useLocalStorage<T>(key: string, initialValue: T) {
  const [storedValue, setStoredValue] = useState<T>(() => {
    try {
      const item = window.localStorage.getItem(key);
      return item ? JSON.parse(item) : initialValue;
    } catch (error) {
      console.error(`Error reading local storage at "${key}":`, error);
      return initialValue;
    }
  });

  useEffect(() => {
    if (setStoredValue !== undefined) {
      try {
        window.localStorage.setItem(key, JSON.stringify(storedValue));
      } catch (error) {
        console.error(`Error setting local storage at "${key}":`, error);
      }
    }
  }, [key, storedValue]);

  return [storedValue, setStoredValue] as const;
}

export function hasCollectionStats(stats?: ICollectionStats): boolean {
  return (
    !!stats &&
    (stats.top_artists_five_star.length > 0 ||
      stats.top_artists_four_plus.length > 0 ||
      stats.top_labels_five_star.length > 0 ||
      stats.top_labels_four_plus.length > 0)
  );
}
