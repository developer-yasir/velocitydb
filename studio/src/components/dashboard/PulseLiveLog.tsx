"use client";

import React from "react";
import { Terminal, ShieldAlert, ArrowRight, Zap, CheckCircle2 } from "lucide-react";

export interface WireCommand {
  id: string;
  timestamp: string;
  cmd: string;
  key: string;
  slot: number;
  node: string;
  latencyMicros: number;
  status: "EXECUTED" | "-MOVED REDIRECT" | "STREAMED" | "SIMD ACCELERATED";
}

interface PulseLiveLogProps {
  commands: WireCommand[];
}

export function PulseLiveLog({ commands }: PulseLiveLogProps) {
  return (
    <div className="glass-panel p-6 rounded-2xl space-y-4">
      {/* Header */}
      <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-2 pb-3 border-b border-white/[0.06]">
        <div className="flex items-center gap-2">
          <Terminal className="w-4 h-4 text-emerald-400" />
          <h3 className="text-sm font-black font-mono tracking-tight text-white uppercase">
            LIVE PROTOCOL WIRE STREAM &amp; HASH SLOT ROUTING
          </h3>
        </div>
        <div className="flex items-center gap-2 text-xs font-mono">
          <span className="relative flex h-2 w-2">
            <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75" />
            <span className="relative inline-flex rounded-full h-2 w-2 bg-emerald-500 shadow-[0_0_8px_#10b981]" />
          </span>
          <span className="text-emerald-400 font-bold">100% RESP3 Direct TCP Sockets</span>
        </div>
      </div>

      {/* Wire Log Table */}
      <div className="overflow-x-auto">
        <table className="w-full text-left font-mono text-xs">
          <thead>
            <tr className="border-b border-white/[0.08] text-zinc-400 text-[11px] uppercase">
              <th className="pb-3 font-semibold">Time</th>
              <th className="pb-3 font-semibold">Command</th>
              <th className="pb-3 font-semibold">Target Key</th>
              <th className="pb-3 font-semibold">Hash Slot</th>
              <th className="pb-3 font-semibold">Node Socket</th>
              <th className="pb-3 font-semibold">P99 Latency</th>
              <th className="pb-3 font-semibold text-right">Cluster Status</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-white/[0.04]">
            {commands.map((tx) => (
              <tr key={tx.id} className="hover:bg-white/[0.02] transition-colors">
                <td className="py-2.5 text-zinc-400 text-[11px]">{tx.timestamp}</td>
                <td className="py-2.5">
                  <span className="px-1.5 py-0.5 rounded text-[10px] font-bold bg-cyan-950/80 text-cyan-300 border border-cyan-800/60">
                    {tx.cmd}
                  </span>
                </td>
                <td className="py-2.5 font-semibold text-zinc-200">{tx.key}</td>
                <td className="py-2.5 text-purple-400 font-bold">{tx.slot}</td>
                <td className="py-2.5 text-zinc-400">{tx.node}</td>
                <td className="py-2.5 text-emerald-400 font-bold">
                  {tx.latencyMicros.toFixed(1)} µs
                </td>
                <td className="py-2.5 text-right">
                  <span
                    className={`text-[10px] px-2 py-0.5 rounded font-bold ${
                      tx.status.includes("-MOVED")
                        ? "bg-amber-950/80 text-amber-300 border border-amber-800/60"
                        : "bg-emerald-950/80 text-emerald-300 border border-emerald-800/60"
                    }`}
                  >
                    {tx.status}
                  </span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
