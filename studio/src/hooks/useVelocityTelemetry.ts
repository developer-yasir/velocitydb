"use client";

import { useState, useEffect, useRef, useCallback } from "react";
import {
  TelemetryPayload,
  TelemetryConnectionState,
} from "@/types";
import { RpsDataPoint } from "@/components/dashboard/LiveRpsChart";
import { CoreTelemetry } from "@/components/dashboard/CoreUtilizationGrid";
import { WS_BASE_URL } from "@/lib/api-config";

interface UseVelocityTelemetryOptions {
  url?: string;
  maxHistoryPoints?: number;
  initialBaseDelayMs?: number;
  maxDelayMs?: number;
}

interface UseVelocityTelemetryReturn {
  telemetry: TelemetryPayload | null;
  rpsHistory: RpsDataPoint[];
  cores: CoreTelemetry[];
  connectionState: TelemetryConnectionState;
  reconnectAttempts: number;
  lastUpdated: number | null;
  isPaused: boolean;
  togglePause: () => void;
  manualReconnect: () => void;
}

const DEFAULT_URL = `${WS_BASE_URL}/ws/telemetry`;
const DEFAULT_MAX_POINTS = 30;
const INITIAL_BACKOFF_MS = 1000;
const MAX_BACKOFF_MS = 16000;

export function useVelocityTelemetry(
  options: UseVelocityTelemetryOptions = {}
): UseVelocityTelemetryReturn {
  const {
    url = DEFAULT_URL,
    maxHistoryPoints = DEFAULT_MAX_POINTS,
    initialBaseDelayMs = INITIAL_BACKOFF_MS,
    maxDelayMs = MAX_BACKOFF_MS,
  } = options;

  const [telemetry, setTelemetry] = useState<TelemetryPayload | null>(null);
  const [rpsHistory, setRpsHistory] = useState<RpsDataPoint[]>([]);
  const [cores, setCores] = useState<CoreTelemetry[]>([]);
  const [connectionState, setConnectionState] =
    useState<TelemetryConnectionState>("connecting");
  const [reconnectAttempts, setReconnectAttempts] = useState<number>(0);
  const [lastUpdated, setLastUpdated] = useState<number | null>(null);
  const [isPaused, setIsPaused] = useState<boolean>(false);

  const wsRef = useRef<WebSocket | null>(null);
  const reconnectTimerRef = useRef<NodeJS.Timeout | null>(null);
  const attemptsRef = useRef<number>(0);
  const isMountedRef = useRef<boolean>(true);
  const isPausedRef = useRef<boolean>(false);

  // Keep isPausedRef synchronized for non-reactive closure access
  useEffect(() => {
    isPausedRef.current = isPaused;
  }, [isPaused]);

  const togglePause = useCallback(() => {
    setIsPaused((prev) => !prev);
  }, []);

  const connect = useCallback(() => {
    // Clear any pending reconnection timer
    if (reconnectTimerRef.current) {
      clearTimeout(reconnectTimerRef.current);
      reconnectTimerRef.current = null;
    }

    // Close any existing socket instance
    if (wsRef.current) {
      try {
        wsRef.current.onclose = null;
        wsRef.current.onerror = null;
        wsRef.current.close();
      } catch {
        // Safe disposal
      }
      wsRef.current = null;
    }

    if (!isMountedRef.current) return;

    setConnectionState((prev) => (prev === "connected" ? prev : "connecting"));

    try {
      const ws = new WebSocket(url);
      wsRef.current = ws;

      ws.onopen = () => {
        if (!isMountedRef.current) return;
        setConnectionState("connected");
        attemptsRef.current = 0;
        setReconnectAttempts(0);
      };

      ws.onmessage = (event: MessageEvent) => {
        if (!isMountedRef.current) return;
        if (isPausedRef.current) return; // Discard live frame if paused

        try {
          const payload: TelemetryPayload = JSON.parse(event.data);
          if (!payload || typeof payload.rps !== "number") return;

          const timestampDate = new Date(payload.timestamp_ms || Date.now());
          const timeString = timestampDate.toTimeString().split(" ")[0];

          // 1. Construct immutable RPS data point strictly from payload
          const newRpsPoint: RpsDataPoint = {
            timestamp: timeString,
            rps: payload.rps,
            latencyMicros: payload.p99_latency_micros,
            batchCommits: Math.floor(payload.rps / 50),
          };

          // 2. Map CPU core telemetry strictly from hardware shards
          const mappedCores: CoreTelemetry[] = Array.isArray(payload.cpu_cores)
            ? payload.cpu_cores.map((c) => ({
                coreId: c.core_id,
                cpuLoad: Math.round(c.load_percent),
                opsSec: c.ops_sec,
                sqeDepth: c.sqe_depth,
                keysCount: c.keys_count,
                isLeader: c.core_id === 0,
              }))
            : [];

          // 3. Batch state updates synchronously in React 18
          setTelemetry(payload);
          setCores(mappedCores);
          setLastUpdated(payload.timestamp_ms);

          setRpsHistory((prev) => {
            const next = [...prev, newRpsPoint];
            if (next.length > maxHistoryPoints) {
              return next.slice(next.length - maxHistoryPoints);
            }
            return next;
          });
        } catch {
          // Ignore malformed wire packet
        }
      };

      ws.onerror = () => {
        if (!isMountedRef.current) return;
        setConnectionState("error");
      };

      ws.onclose = () => {
        if (!isMountedRef.current) return;
        setConnectionState("reconnecting");

        // Exponential backoff calculation
        const currentAttempt = attemptsRef.current + 1;
        attemptsRef.current = currentAttempt;
        setReconnectAttempts(currentAttempt);

        // Exponential factor: 1s, 1.5s, 2.25s, 3.37s, up to 16s max
        const backoffDelay = Math.min(
          maxDelayMs,
          initialBaseDelayMs * Math.pow(1.5, currentAttempt - 1)
        );

        reconnectTimerRef.current = setTimeout(() => {
          if (isMountedRef.current) {
            connect();
          }
        }, backoffDelay);
      };
    } catch {
      if (!isMountedRef.current) return;
      setConnectionState("reconnecting");
      const currentAttempt = attemptsRef.current + 1;
      attemptsRef.current = currentAttempt;
      setReconnectAttempts(currentAttempt);

      reconnectTimerRef.current = setTimeout(() => {
        if (isMountedRef.current) {
          connect();
        }
      }, initialBaseDelayMs);
    }
  }, [url, maxHistoryPoints, initialBaseDelayMs, maxDelayMs]);

  const manualReconnect = useCallback(() => {
    attemptsRef.current = 0;
    setReconnectAttempts(0);
    connect();
  }, [connect]);

  // Primary WebSocket lifecycle
  useEffect(() => {
    isMountedRef.current = true;
    connect();

    return () => {
      isMountedRef.current = false;
      if (reconnectTimerRef.current) {
        clearTimeout(reconnectTimerRef.current);
        reconnectTimerRef.current = null;
      }
      if (wsRef.current) {
        try {
          wsRef.current.onclose = null;
          wsRef.current.onerror = null;
          wsRef.current.close();
        } catch {
          // Safe unmount
        }
        wsRef.current = null;
      }
    };
  }, [connect]);

  return {
    telemetry,
    rpsHistory,
    cores,
    connectionState,
    reconnectAttempts,
    lastUpdated,
    isPaused,
    togglePause,
    manualReconnect,
  };
}