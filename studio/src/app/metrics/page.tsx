"use client";

import React, { useState } from "react";
import {
  Cpu,
  HardDrive,
  Activity,
  Layers,
  Zap,
  CheckCircle2,
  AlertTriangle,
  ArrowDownRight,
  TrendingDown,
} from "lucide-react";

export default function MetricsPage() {
  const [selectedCore, setSelectedCore] = useState<number>(0);

  const cores = Array.from({ length: 16 }, (_, i) => ({
    id: i,
    cpuUsage: Math.floor(70 + Math.random() * 25),
    rps: Math.floor(95000 + Math.random() * 20000),
    sqeDepth: Math.floor(32 + Math.random() * 15),
    cqePending: Math.floor(8 + Math.random() * 12),
    keysCount: Math.floor(92000 + Math.random() * 5000),
    isLeader: i === 0,
  }));

  const activeCoreData = cores[selectedCore];

  return (
    <div className="space-y-8 animate-in fade-in duration-300">
      {/* Page Header */}
      <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4 pb-2 border-b border-white/[0.06]">
        <div>
          <div className="flex items-center gap-2.5">
            <h1 className="text-2xl font-black tracking-tight text-white font-mono flex items-center gap-2">
              CPU &amp; MEMORY HARDWARE MONITOR
            </h1>
            <span className="px-2 py-0.5 text-[10px] font-mono font-bold uppercase rounded bg-purple-950/80 text-purple-300 border border-purple-800/60">
              THREAD-PER-CORE (io_uring)
            </span>
          </div>
          <p className="text-xs text-zinc-400 font-mono mt-1">
            Zero-contention core pinning via Linux CpuSet. No global locks, no cache-line bouncing.
          </p>
        </div>

        <div className="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-[#0e131d] border border-white/[0.08] text-xs font-mono">
          <span className="w-2 h-2 rounded-full bg-emerald-400 animate-pulse" />
          <span className="text-zinc-300">RSS Status:</span>
          <strong className="text-emerald-400">Strict 2.00 MB Ceiling Maintained</strong>
        </div>
      </div>

      {/* Top 3 Diagnostic Cards */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
        {/* Memory Diagnostic */}
        <div className="glass-panel p-6 rounded-2xl space-y-4">
          <div className="flex items-center justify-between text-zinc-400">
            <span className="text-xs font-mono uppercase font-semibold">RSS Memory Allocation</span>
            <HardDrive className="w-4 h-4 text-orange-400" />
          </div>

          <div className="flex items-baseline gap-2">
            <span className="text-3xl font-black font-mono text-white">2.00 MB</span>
            <span className="text-xs font-mono text-zinc-400">/ 2.00 MB Strict Limit</span>
          </div>

          <div className="w-full bg-white/[0.07] h-2 rounded-full overflow-hidden">
            <div className="bg-gradient-to-r from-orange-500 to-amber-400 h-full rounded-full w-full" />
          </div>

          <div className="pt-2 border-t border-white/[0.06] space-y-2 text-xs font-mono text-zinc-400">
            <div className="flex justify-between">
              <span>Synchronous Evictions:</span>
              <span className="text-white font-bold">43,740 Keys</span>
            </div>
            <div className="flex justify-between">
              <span>Eviction Algorithm:</span>
              <span className="text-cyan-400 font-bold">Intrusive O(1) LRU</span>
            </div>
            <div className="flex justify-between">
              <span>Heap Fragmentation:</span>
              <span className="text-emerald-400 font-bold">0.0% (MiMalloc)</span>
            </div>
          </div>
        </div>

        {/* io_uring Queue Depths */}
        <div className="glass-panel p-6 rounded-2xl space-y-4">
          <div className="flex items-center justify-between text-zinc-400">
            <span className="text-xs font-mono uppercase font-semibold">io_uring Ring Engine</span>
            <Zap className="w-4 h-4 text-cyan-400" />
          </div>

          <div className="flex items-baseline gap-2">
            <span className="text-3xl font-black font-mono text-white">1,024</span>
            <span className="text-xs font-mono text-zinc-400">Entries / Ring</span>
          </div>

          <div className="w-full bg-white/[0.07] h-2 rounded-full overflow-hidden">
            <div className="bg-gradient-to-r from-cyan-500 to-blue-500 h-full rounded-full w-[42%]" />
          </div>

          <div className="pt-2 border-t border-white/[0.06] space-y-2 text-xs font-mono text-zinc-400">
            <div className="flex justify-between">
              <span>Submission Queue (SQE):</span>
              <span className="text-cyan-400 font-bold">{activeCoreData.sqeDepth} Active</span>
            </div>
            <div className="flex justify-between">
              <span>Completion Queue (CQE):</span>
              <span className="text-purple-400 font-bold">{activeCoreData.cqePending} Pending</span>
            </div>
            <div className="flex justify-between">
              <span>Kernel Syscalls / Op:</span>
              <span className="text-emerald-400 font-bold">0 (Batch Committed)</span>
            </div>
          </div>
        </div>

        {/* Linux Kernel Primitives */}
        <div className="glass-panel p-6 rounded-2xl space-y-4">
          <div className="flex items-center justify-between text-zinc-400">
            <span className="text-xs font-mono uppercase font-semibold">Kernel &amp; OS Primitives</span>
            <Layers className="w-4 h-4 text-purple-400" />
          </div>

          <div className="flex items-baseline gap-2">
            <span className="text-3xl font-black font-mono text-white">Linux 6.8+</span>
            <span className="text-xs font-mono text-emerald-400 font-bold">io_uring Native</span>
          </div>

          <div className="pt-2 border-t border-white/[0.06] space-y-2 text-xs font-mono text-zinc-400">
            <div className="flex justify-between">
              <span>Socket Load Balancing:</span>
              <span className="text-white font-bold">SO_REUSEPORT (Kernel)</span>
            </div>
            <div className="flex justify-between">
              <span>Memory Lock (memlock):</span>
              <span className="text-emerald-400 font-bold">Unlimited (Direct DMA)</span>
            </div>
            <div className="flex justify-between">
              <span>Direct I/O Persistence:</span>
              <span className="text-orange-400 font-bold">O_DIRECT 4096-Aligned</span>
            </div>
          </div>
        </div>
      </div>

      {/* 16-Core Matrix Visualizer */}
      <div className="glass-panel p-6 rounded-2xl space-y-4">
        <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-2 pb-3 border-b border-white/[0.06]">
          <div className="flex items-center gap-2">
            <Cpu className="w-4 h-4 text-cyan-400" />
            <h3 className="text-sm font-black font-mono tracking-tight text-white uppercase">
              Individual CPU Shard Affinity (Click to Inspect)
            </h3>
          </div>
          <span className="text-xs font-mono text-zinc-400">
            Showing Selected: <strong className="text-cyan-400">Core {selectedCore}</strong>
          </span>
        </div>

        <div className="grid grid-cols-2 sm:grid-cols-4 md:grid-cols-8 gap-3">
          {cores.map((c) => {
            const isSelected = selectedCore === c.id;
            return (
              <button
                key={c.id}
                onClick={() => setSelectedCore(c.id)}
                className={`p-3 rounded-xl border text-left font-mono transition-all ${
                  isSelected
                    ? "bg-cyan-500/20 border-cyan-400 shadow-[0_0_15px_-3px_rgba(0,242,254,0.3)]"
                    : "bg-[#0d121c] border-white/[0.06] hover:border-white/20"
                }`}
              >
                <div className="flex items-center justify-between text-[10px] text-zinc-400">
                  <span className="font-bold">CORE {c.id}</span>
                  {c.isLeader && (
                    <span className="text-[8px] px-1 rounded bg-cyan-950 text-cyan-400 border border-cyan-800">
                      LEADER
                    </span>
                  )}
                </div>

                <div className="text-lg font-black text-white mt-1">
                  {c.cpuUsage}%
                </div>

                <div className="w-full bg-white/10 h-1.5 rounded-full mt-2 overflow-hidden">
                  <div
                    className="bg-cyan-400 h-full rounded-full"
                    style={{ width: `${c.cpuUsage}%` }}
                  />
                </div>

                <div className="text-[9px] text-zinc-400 mt-2 truncate">
                  {(c.rps / 1000).toFixed(0)}k ops/sec
                </div>
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
}
