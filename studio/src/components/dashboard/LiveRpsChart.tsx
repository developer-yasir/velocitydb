"use client";

import React, { useState, memo } from "react";
import {
  AreaChart,
  Area,
  XAxis,
  YAxis,
  Tooltip,
  ResponsiveContainer,
  CartesianGrid,
} from "recharts";
import { Activity, Zap, Play, Pause, ArrowUpRight } from "lucide-react";

export interface RpsDataPoint {
  timestamp: string;
  rps: number;
  latencyMicros: number;
  batchCommits: number;
}

interface LiveRpsChartProps {
  data: RpsDataPoint[];
  isPaused: boolean;
  onTogglePause: () => void;
}

export const LiveRpsChart = memo(function LiveRpsChart({
  data,
  isPaused,
  onTogglePause,
}: LiveRpsChartProps) {
  const [timeWindow, setTimeWindow] = useState<"10s" | "30s" | "1m" | "5m">("30s");

  const currentPoint =
    data.length > 0
      ? data[data.length - 1]
      : {
          timestamp: "--:--:--",
          rps: 0,
          latencyMicros: 0,
          batchCommits: 0,
        };

  // Peak RPS calculation strictly across the received hardware frames
  const peakRps =
    data.length > 0
      ? Math.max(...data.map((d) => d.rps))
      : currentPoint.rps;

  return (
    <div className="glass-panel p-6 rounded-2xl flex flex-col justify-between space-y-4 relative overflow-hidden">
      {/* Top Controls Bar */}
      <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4 pb-4 border-b border-white/[0.06]">
        <div>
          <div className="flex items-center gap-2.5">
            <div className="p-1.5 rounded-lg bg-cyan-500/10 border border-cyan-500/20 text-cyan-400">
              <Activity className="w-4 h-4 animate-pulse" />
            </div>
            <h2 className="text-base font-black font-mono tracking-tight text-white uppercase flex items-center gap-2">
              <span>LIVE RPS MONITOR</span>
              <span className="text-[10px] font-mono px-2 py-0.5 rounded uppercase font-bold bg-cyan-950/80 text-cyan-300 border border-cyan-800/60 shadow-[0_0_10px_rgba(0,242,254,0.3)]">
                UP TO 1.6M+ RPS
              </span>
            </h2>
          </div>
          <p className="text-xs text-zinc-400 font-mono mt-1">
            Real-time throughput telemetry sampled via zero-copy Linux io_uring completion rings.
          </p>
        </div>

        {/* Action Controls */}
        <div className="flex items-center gap-2 self-stretch sm:self-auto">
          {/* Pause / Resume button */}
          <button
            onClick={onTogglePause}
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-mono font-bold border transition-colors ${
              isPaused
                ? "bg-amber-500/20 text-amber-300 border-amber-500/40"
                : "bg-white/[0.05] text-zinc-300 border-white/[0.08] hover:bg-white/[0.1]"
            }`}
          >
            {isPaused ? <Play className="w-3.5 h-3.5 fill-current" /> : <Pause className="w-3.5 h-3.5 fill-current" />}
            <span>{isPaused ? "RESUME" : "PAUSE"}</span>
          </button>

          {/* Time Resolution Tabs */}
          <div className="flex items-center p-1 rounded-lg bg-black/40 border border-white/[0.08] text-xs font-mono">
            {(["10s", "30s", "1m", "5m"] as const).map((t) => (
              <button
                key={t}
                onClick={() => setTimeWindow(t)}
                className={`px-2.5 py-1 rounded uppercase font-semibold transition-colors ${
                  timeWindow === t
                    ? "bg-cyan-500/20 text-cyan-300 border border-cyan-500/40"
                    : "text-zinc-400 hover:text-white"
                }`}
              >
                {t}
              </button>
            ))}
          </div>
        </div>
      </div>

      {/* Instantaneous Telemetry Row */}
      <div className="grid grid-cols-2 sm:grid-cols-4 gap-4 py-2 font-mono">
        <div className="p-3 rounded-xl bg-black/40 border border-white/[0.05]">
          <span className="text-[10px] text-zinc-400 uppercase block">Current Throughput</span>
          <span className="text-xl font-black text-cyan-300 text-glow-cyan">
            {(currentPoint.rps / 1_000_000).toFixed(3)}M
          </span>
          <span className="text-[10px] text-zinc-400 ml-1">ops/sec</span>
        </div>

        <div className="p-3 rounded-xl bg-black/40 border border-white/[0.05]">
          <span className="text-[10px] text-zinc-400 uppercase block">Window Peak RPS</span>
          <span className="text-xl font-black text-white">
            {(peakRps / 1_000_000).toFixed(3)}M
          </span>
          <span className="text-[10px] text-emerald-400 ml-1 font-bold">↑ MAX</span>
        </div>

        <div className="p-3 rounded-xl bg-black/40 border border-white/[0.05]">
          <span className="text-[10px] text-zinc-400 uppercase block">Batch Commits / Sec</span>
          <span className="text-xl font-black text-zinc-200">
            {currentPoint.batchCommits.toLocaleString()}
          </span>
          <span className="text-[10px] text-zinc-400 ml-1">syscalls saved</span>
        </div>

        <div className="p-3 rounded-xl bg-black/40 border border-white/[0.05]">
          <span className="text-[10px] text-zinc-400 uppercase block">Wire Engine</span>
          <span className="text-xl font-black text-emerald-400 flex items-center gap-1">
            <Zap className="w-4 h-4 text-emerald-400" />
            io_uring
          </span>
          <span className="text-[10px] text-zinc-400">Lock-Free</span>
        </div>
      </div>

      {/* Main Recharts Area Chart */}
      <div className="h-72 w-full pt-2">
        <ResponsiveContainer width="100%" height="100%">
          <AreaChart data={data} margin={{ top: 10, right: 10, left: 0, bottom: 0 }}>
            <defs>
              {/* Glowing Gradient Fill */}
              <linearGradient id="neonRpsFill" x1="0" y1="0" x2="0" y2="1">
                <stop offset="0%" stopColor="#00f2fe" stopOpacity={0.4} />
                <stop offset="50%" stopColor="#0070f3" stopOpacity={0.15} />
                <stop offset="100%" stopColor="#080a0f" stopOpacity={0.0} />
              </linearGradient>

              {/* SVG Drop Shadow Filter for Glow */}
              <filter id="neonGlow" x="-20%" y="-20%" width="140%" height="140%">
                <feGaussianBlur stdDeviation="3" result="blur" />
                <feMerge>
                  <feMergeNode in="blur" />
                  <feMergeNode in="SourceGraphic" />
                </feMerge>
              </filter>
            </defs>

            <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.05)" vertical={false} />

            <XAxis
              dataKey="timestamp"
              stroke="#374151"
              tick={{ fill: "#6b7280", fontSize: 10, fontFamily: "monospace" }}
              tickLine={{ stroke: "#374151" }}
            />

            <YAxis
              stroke="#374151"
              domain={[0, (dataMax: number) => Math.max(Math.ceil(dataMax * 1.1), 1_800_000)]}
              tick={{ fill: "#6b7280", fontSize: 10, fontFamily: "monospace" }}
              tickLine={{ stroke: "#374151" }}
              tickFormatter={(val) => `${(val / 1_000_000).toFixed(2)}M`}
            />

            <Tooltip
              content={({ active, payload }) => {
                if (active && payload && payload.length) {
                  const p = payload[0].payload as RpsDataPoint;
                  return (
                    <div className="p-3 rounded-xl bg-[#090d14]/95 border border-cyan-500/40 shadow-[0_0_20px_rgba(0,242,254,0.25)] font-mono text-xs backdrop-blur-md">
                      <div className="text-[10px] text-zinc-400 border-b border-white/[0.08] pb-1 mb-1.5 flex justify-between gap-4">
                        <span>TIMESTAMP: {p.timestamp}</span>
                        <span className="text-emerald-400 font-bold">io_uring SYNC</span>
                      </div>
                      <div className="flex items-baseline justify-between gap-6">
                        <span className="text-zinc-300">Throughput:</span>
                        <span className="text-cyan-300 font-bold text-sm">
                          {p.rps.toLocaleString()} ops/sec
                        </span>
                      </div>
                      <div className="flex items-baseline justify-between gap-6 mt-1">
                        <span className="text-zinc-300">P99 Latency:</span>
                        <span className="text-emerald-400 font-bold">
                          {p.latencyMicros.toFixed(1)} µs
                        </span>
                      </div>
                      <div className="flex items-baseline justify-between gap-6 mt-1 text-[10px] text-zinc-400">
                        <span>Pipelined Batch:</span>
                        <span className="text-white">50 commands / syscall</span>
                      </div>
                    </div>
                  );
                }
                return null;
              }}
            />

            <Area
              type="monotone"
              dataKey="rps"
              stroke="#00f2fe"
              strokeWidth={2.5}
              fill="url(#neonRpsFill)"
              filter="url(#neonGlow)"
              isAnimationActive={false}
            />
          </AreaChart>
        </ResponsiveContainer>
      </div>

      {/* Bottom Live Streaming Indicator */}
      <div className="flex items-center justify-between pt-3 border-t border-white/[0.06] text-xs font-mono">
        <div className="flex items-center gap-2">
          <span className="relative flex h-2 w-2">
            <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-cyan-400 opacity-75" />
            <span className="relative inline-flex rounded-full h-2 w-2 bg-cyan-500 shadow-[0_0_8px_#00f2fe]" />
          </span>
          <span className="text-zinc-400">
            Streaming socket telemetry via zero-copy io_uring (500ms real broadcast)
          </span>
        </div>

        <div className="flex items-center gap-1.5 text-zinc-400 text-[11px]">
          <span>Linear Scaling:</span>
          <strong className="text-white">16 Cores Pinned</strong>
        </div>
      </div>
    </div>
  );
});
