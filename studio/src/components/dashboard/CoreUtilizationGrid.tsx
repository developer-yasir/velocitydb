"use client";

import React, { useState, memo } from "react";
import { Cpu, ShieldCheck, Zap, Layers, Activity } from "lucide-react";

export interface CoreTelemetry {
  coreId: number;
  cpuLoad: number; // 0% to 100%
  opsSec: number;
  sqeDepth: number;
  keysCount: number;
  isLeader: boolean;
}

interface CoreUtilizationGridProps {
  cores: CoreTelemetry[];
}

export const CoreUtilizationGrid = memo(function CoreUtilizationGrid({
  cores,
}: CoreUtilizationGridProps) {
  const [selectedCoreId, setSelectedCoreId] = useState<number>(0);

  // Stable 16-shard fallback to eliminate layout shift during initial socket handshake
  const displayCores =
    cores && cores.length >= 16
      ? cores
      : Array.from({ length: 16 }, (_, i) => ({
          coreId: i,
          cpuLoad: 0,
          opsSec: 0,
          sqeDepth: 0,
          keysCount: 0,
          isLeader: i === 0,
        }));

  const selectedCore =
    displayCores.find((c) => c.coreId === selectedCoreId) || displayCores[0];

  return (
    <div className="glass-panel p-6 rounded-2xl flex flex-col justify-between space-y-6 relative overflow-hidden">
      {/* Header */}
      <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4 pb-4 border-b border-white/[0.06]">
        <div>
          <div className="flex items-center gap-2.5">
            <div className="p-1.5 rounded-lg bg-purple-500/10 border border-purple-500/20 text-purple-400">
              <Cpu className="w-4 h-4" />
            </div>
            <h2 className="text-base font-black font-mono tracking-tight text-white uppercase flex items-center gap-2">
              <span>CORE UTILIZATION GRID</span>
              <span className="text-[10px] font-mono px-2 py-0.5 rounded uppercase font-bold bg-purple-950/80 text-purple-300 border border-purple-800/60 shadow-[0_0_10px_rgba(168,85,247,0.3)]">
                16 PINNED SHARDS
              </span>
            </h2>
          </div>
          <p className="text-xs text-zinc-400 font-mono mt-1">
            Proving 100% lock-free Thread-per-Core hardware saturation across all online CPU cores.
          </p>
        </div>

        <div className="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-[#0e131d] border border-white/[0.08] text-xs font-mono">
          <ShieldCheck className="w-4 h-4 text-emerald-400" />
          <span className="text-zinc-300">Lock Contention:</span>
          <strong className="text-emerald-400">0 Cross-Thread Mutexes</strong>
        </div>
      </div>

      {/* 16-Core Matrix (Sleek Circular Gauges + Progress Bars) */}
      <div className="grid grid-cols-2 sm:grid-cols-4 lg:grid-cols-8 gap-3">
        {displayCores.map((core) => {
          const isSelected = selectedCoreId === core.coreId;

          // Color theme based on CPU utilization load
          const isHeavy = core.cpuLoad >= 80;
          const isOptimal = core.cpuLoad >= 50 && core.cpuLoad < 80;

          const strokeColor = isHeavy
            ? "#f97316" // Orange
            : isOptimal
            ? "#00f2fe" // Electric Cyan
            : "#10b981"; // Emerald

          const glowClass = isHeavy
            ? "shadow-[0_0_15px_-3px_rgba(249,115,22,0.3)] border-orange-500/40"
            : isOptimal
            ? "shadow-[0_0_15px_-3px_rgba(0,242,254,0.25)] border-cyan-500/40"
            : "border-white/[0.06]";

          // Circular gauge math (radius 18, circumference 2 * PI * 18 = ~113.1)
          const radius = 18;
          const circumference = 2 * Math.PI * radius;
          const strokeDashoffset = circumference - (core.cpuLoad / 100) * circumference;

          return (
            <button
              key={core.coreId}
              onClick={() => setSelectedCoreId(core.coreId)}
              className={`p-3 rounded-xl border text-left font-mono transition-all relative overflow-hidden group ${
                isSelected
                  ? "bg-white/[0.08] border-cyan-400 shadow-[0_0_20px_-3px_rgba(0,242,254,0.35)]"
                  : `bg-[#0d121c]/80 hover:bg-white/[0.04] ${glowClass}`
              }`}
            >
              {/* Header inside mini core card */}
              <div className="flex items-center justify-between text-[10px]">
                <span className="font-bold text-zinc-300">
                  C{core.coreId < 10 ? `0${core.coreId}` : core.coreId}
                </span>

                {core.isLeader ? (
                  <span className="text-[8px] px-1 py-0.2 rounded font-bold uppercase bg-cyan-950 text-cyan-300 border border-cyan-800">
                    LEAD
                  </span>
                ) : (
                  <span className="text-[8px] text-zinc-400">SHARD</span>
                )}
              </div>

              {/* Circular Gauge Meter */}
              <div className="flex items-center justify-center my-2 relative">
                <svg className="w-12 h-12 transform -rotate-90">
                  {/* Background track circle */}
                  <circle
                    cx="24"
                    cy="24"
                    r={radius}
                    stroke="rgba(255, 255, 255, 0.08)"
                    strokeWidth="3.5"
                    fill="transparent"
                  />
                  {/* Active progress circle */}
                  <circle
                    cx="24"
                    cy="24"
                    r={radius}
                    stroke={strokeColor}
                    strokeWidth="3.5"
                    strokeDasharray={circumference}
                    strokeDashoffset={strokeDashoffset}
                    strokeLinecap="round"
                    fill="transparent"
                    className="transition-all duration-700 ease-out"
                  />
                </svg>

                {/* Center Percentage Display */}
                <span className="absolute font-black text-xs text-white">
                  {core.cpuLoad}%
                </span>
              </div>

              {/* Progress bar underneath */}
              <div className="w-full bg-white/[0.08] h-1 rounded-full overflow-hidden">
                <div
                  className="h-full rounded-full transition-all duration-700"
                  style={{
                    width: `${core.cpuLoad}%`,
                    backgroundColor: strokeColor,
                  }}
                />
              </div>

              {/* Micro stats */}
              <div className="flex items-center justify-between text-[9px] text-zinc-400 mt-2 truncate">
                <span>{(core.opsSec / 1000).toFixed(0)}k RPS</span>
                <span className="text-zinc-400">{core.sqeDepth} SQE</span>
              </div>
            </button>
          );
        })}
      </div>

      {/* Selected Core Detailed Diagnostic Panel */}
      <div className="p-4 rounded-xl bg-black/40 border border-white/[0.08] flex flex-col md:flex-row items-start md:items-center justify-between gap-4 font-mono text-xs">
        <div className="flex items-center gap-3">
          <div className="p-2 rounded-xl bg-cyan-500/10 border border-cyan-500/20 text-cyan-400">
            <Activity className="w-4 h-4" />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <span className="font-bold text-white text-sm">
                SELECTED INSPECTOR: CPU SHARD {selectedCore.coreId}
              </span>
              {selectedCore.isLeader && (
                <span className="text-[10px] px-1.5 py-0.2 rounded bg-cyan-950 text-cyan-300 border border-cyan-800 font-bold">
                  CORE 0: GOSSIP BUS (PORT 16379)
                </span>
              )}
            </div>
            <span className="text-[11px] text-zinc-400">
              Owns independent lock-free <code className="text-cyan-300">LocalCache</code> with zero atomic CAS overhead.
            </span>
          </div>
        </div>

        <div className="flex items-center gap-6 self-stretch md:self-auto justify-between md:justify-end border-t md:border-t-0 border-white/[0.06] pt-3 md:pt-0">
          <div>
            <span className="text-[10px] text-zinc-400 block uppercase">Dedicated Keys</span>
            <span className="text-white font-bold">{selectedCore.keysCount.toLocaleString()}</span>
          </div>
          <div>
            <span className="text-[10px] text-zinc-400 block uppercase">io_uring SQE Depth</span>
            <span className="text-cyan-400 font-bold">{selectedCore.sqeDepth} / 1024</span>
          </div>
          <div>
            <span className="text-[10px] text-zinc-400 block uppercase">Core Affinity</span>
            <span className="text-emerald-400 font-bold">glommio::CpuSet</span>
          </div>
        </div>
      </div>
    </div>
  );
});
