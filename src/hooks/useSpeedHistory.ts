import { useState, useEffect, useRef } from "react";
import { useDownloadStore } from "@/stores/downloadStore";

export interface SpeedSample {
  time: number;
  speed: number;
}

const MAX_SAMPLES = 60; // 2 min at 2s intervals
const SAMPLE_INTERVAL_MS = 2000;
const MAX_AGE_MS = 120_000; // 2 minutes

export function useSpeedHistory(downloadId: string): SpeedSample[] {
  const [samples, setSamples] = useState<SpeedSample[]>([]);
  const samplesRef = useRef<SpeedSample[]>([]);

  useEffect(() => {
    samplesRef.current = [];
    setSamples([]);

    const sample = () => {
      const progress = useDownloadStore.getState().progressMap[downloadId];
      const speed = progress?.speedBytesPerSec ?? 0;
      const now = Date.now();
      const cutoff = now - MAX_AGE_MS;

      samplesRef.current = [
        ...samplesRef.current.filter((s) => s.time > cutoff),
        { time: now, speed },
      ].slice(-MAX_SAMPLES);

      setSamples([...samplesRef.current]);
    };

    sample();
    const interval = setInterval(sample, SAMPLE_INTERVAL_MS);

    return () => clearInterval(interval);
  }, [downloadId]);

  return samples;
}
