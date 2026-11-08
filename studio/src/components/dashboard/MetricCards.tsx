"use client";

import React from "react";
import {
  HardDrive,
  Gauge,
  Users,
  ArrowUpRight,
  ShieldCheck,
  Zap,
  Activity,
  Layers,
} from "lucide-react";

interface MetricCardsProps {
  currentRps: number;
  currentLatency: number;
  activeConnections: number;
  evictionCount: number;
}

export function MetricCards({
  currentRps,
  currentLatency,
  activeConnections,
  evictionCount,
}: MetricCardsProps) {
  return (
    <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
      {/* CARD 1: Total RAM Used (LRU) */}
      <div className="glass-card-interactive p-6 rounded-2xl relative overflow-hidden group">
        <div className="flex items-center justify-between text-zinc-400 mb-3">
          <div className="flex items-center gap-2">
            <div className="p-2 rounded-xl bg-orange-500/10 border border-orange-500/20 text-orange-400">
              <HardDrive className="w-4 h-4" />
            </div>
            <span className="text-xs font-mono uppercase tracking-wider font-semibold text-zinc-300">
              Total RAM Used (LRU)
            </span>
          </div>
          <span className="text-[10px] font-mono px-2 py-0.5 rounded uppercase font-bold bg-orange-950/80 text-orange-300 border border-orange-800/60">
            STRICT CEILING
          </span>
        </div>

        <div className="flex items-baseline gap-2">
          <span className="text-3xl font-black font-mono text-white tracking-tight">
            2.00 MB
          </span>
          <span className="text-xs font-mono text-zinc-400">/ 2.00 MB Limit</span>
        </div>

        {/* Dynamic Progress Bar */}
        <div className="w-full bg-white/[0.07] h-2 rounded-full mt-4 overflow-hidden relative">
          <div
            className="h-full rounded-full bg-gradient-to-r from-orange-500 via-amber-400 to-yellow-400 shadow-[0_0_12px_rgba(249,115,22,0.6)] transition-all duration-500"
            style={{ width: "99.8%" }}
          />
        </div>

        <div className="grid grid-cols-2 gap-2 pt-4 mt-2 border-t border-white/[0.06] text-xs font-mono">
          <div>
            <span className="text-[10px] text-zinc-400 block uppercase">Synchronous Evictions</span>
            <span className="text-orange-400 font-bold flex items-center gap-1 mt-0.5">
              <Zap className="w-3 h-3 text-orange-400" />
              {evictionCount.toLocaleString()} keys
            </span>
          </div>
          <div>
            <span className="text-[10px] text-zinc-400 block uppercase">Policy &amp; Engine</span>
            <span className="text-zinc-200 font-bold flex items-center gap-1 mt-0.5">
              <Layers className="w-3 h-3 text-cyan-400" />
              O(1) Intrusive List
            </span>
          </div>
        </div>

        <div className="absolute -right-6 -bottom-6 w-28 h-28 bg-orange-500/5 rounded-full blur-2xl group-hover:bg-orange-500/10 transition-colors pointer-events-none" />
      </div>

      {/* CARD 2: Current Latency (avg 18µs) */}
      <div className="glass-card-interactive p-6 rounded-2xl relative overflow-hidden group">
        <div className="flex items-center justify-between text-zinc-400 mb-3">
          <div className="flex items-center gap-2">
            <div className="p-2 rounded-xl bg-cyan-500/10 border border-cyan-500/20 text-cyan-400">
              <Gauge className="w-4 h-4" />
            </div>
            <span className="text-xs font-mono uppercase tracking-wider font-semibold text-zinc-300">
              Current Latency (avg 18µs)
            </span>
          </div>
          <span className="text-[10px] font-mono px-2 py-0.5 rounded uppercase font-bold bg-cyan-950/80 text-cyan-300 border border-cyan-800/60 shadow-[0_0_10px_rgba(0,242,254,0.2)]">
            276x FASTER
          </span>
        </div>

        <div className="flex items-baseline gap-2">
          <span className="text-3xl font-black font-mono text-cyan-300 tracking-tight text-glow-cyan">
            {currentLatency.toFixed(1)}
          </span>
          <span className="text-xs font-mono text-cyan-400 font-bold">µs (Microseconds)</span>
        </div>

        {/* SLA Bar Indicator */}
        <div className="w-full bg-white/[0.07] h-2 rounded-full mt-4 overflow-hidden relative">
          <div
            className="h-full rounded-full bg-gradient-to-r from-cyan-400 to-blue-500 shadow-[0_0_12px_rgba(0,242,254,0.6)] transition-all duration-300"
            style={{ width: `${Math.min(100, (currentLatency / 25) * 100)}%` }}
          />
        </div>

        <div className="grid grid-cols-3 gap-1 pt-4 mt-2 border-t border-white/[0.06] text-center font-mono">
          <div>
            <span className="text-[9px] text-zinc-400 uppercase block">P50 Median</span>
            <span className="text-emerald-400 font-bold text-xs">4.5 µs</span>
          </div>
          <div>
            <span className="text-[9px] text-zinc-400 uppercase block">P90 Target</span>
            <span className="text-cyan-400 font-bold text-xs">9.2 µs</span>
          </div>
          <div>
            <span className="text-[9px] text-zinc-400 uppercase block">P99 SLA</span>
            <span className="text-white font-bold text-xs">18.1 µs</span>
          </div>
        </div>

        <div className="absolute -right-6 -bottom-6 w-28 h-28 bg-cyan-500/5 rounded-full blur-2xl group-hover:bg-cyan-500/10 transition-colors pointer-events-none" />
      </div>

      {/* CARD 3: Active Connections */}
      <div className="glass-card-interactive p-6 rounded-2xl relative overflow-hidden group">
        <div className="flex items-center justify-between text-zinc-400 mb-3">
          <div className="flex items-center gap-2">
            <div className="p-2 rounded-xl bg-emerald-500/10 border border-emerald-500/20 text-emerald-400">
              <Users className="w-4 h-4" />
            </div>
            <span className="text-xs font-mono uppercase tracking-wider font-semibold text-zinc-300">
              Active Connections
            </span>
          </div>
          <span className="text-[10px] font-mono px-2 py-0.5 rounded uppercase font-bold bg-emerald-950/80 text-emerald-400 border border-emerald-800/60">
            SO_REUSEPORT
          </span>
        </div>

        <div className="flex items-baseline gap-2">
          <span className="text-3xl font-black font-mono text-white tracking-tight">
            {activeConnections.toLocaleString()}
          </span>
          <span className="text-xs font-mono text-emerald-400 font-bold">TCP Sockets</span>
        </div>

        {/* Sockets Distribution Bar */}
        <div className="w-full bg-white/[0.07] h-2 rounded-full mt-4 overflow-hidden relative">
          <div
            className="h-full rounded-full bg-gradient-to-r from-emerald-500 to-teal-400 shadow-[0_0_12px_rgba(16,185,129,0.5)] transition-all duration-300"
            style={{ width: "84%" }}
          />
        </div>

        <div className="grid grid-cols-2 gap-2 pt-4 mt-2 border-t border-white/[0.06] text-xs font-mono">
          <div>
            <span className="text-[10px] text-zinc-400 block uppercase">Kernel Distribution</span>
            <span className="text-emerald-400 font-bold flex items-center gap-1 mt-0.5">
              <Activity className="w-3 h-3 text-emerald-400" />
              16 Cores Balanced
            </span>
          </div>
          <div>
            <span className="text-[10px] text-zinc-400 block uppercase">Batch Pipelining</span>
            <span className="text-zinc-200 font-bold flex items-center gap-1 mt-0.5">
              <ArrowUpRight className="w-3 h-3 text-cyan-400" />
              50 Cmds / Buffer
            </span>
          </div>
        </div>

        <div className="absolute -right-6 -bottom-6 w-28 h-28 bg-emerald-500/5 rounded-full blur-2xl group-hover:bg-emerald-500/10 transition-colors pointer-events-none" />
      </div>
    </div>
  );
}
