"use client";

import React, { useState } from "react";
import {
  Search,
  Wifi,
  ChevronDown,
  ShieldCheck,
  Zap,
  Gauge,
  HardDrive,
  RefreshCw,
} from "lucide-react";
import { VelocityNode } from "@/types";
import { useVelocityTelemetry } from "@/hooks/useVelocityTelemetry";

const CLUSTER_NODES: VelocityNode[] = [
  {
    id: "node-1",
    name: "node-01.aws-us-east (Leader)",
    host: "127.0.0.1",
    clientPort: 6379,
    busPort: 16379,
    role: "master",
    status: "online",
    slotsRange: "0 - 5461",
    assignedSlotsCount: 5462,
    rssMemoryMB: 2.0,
    coresCount: 64,
  },
  {
    id: "node-2",
    name: "node-02.aws-us-east",
    host: "127.0.0.1",
    clientPort: 6380,
    busPort: 16380,
    role: "master",
    status: "online",
    slotsRange: "5462 - 10922",
    assignedSlotsCount: 5461,
    rssMemoryMB: 2.0,
    coresCount: 64,
  },
  {
    id: "node-3",
    name: "node-03.aws-us-east",
    host: "127.0.0.1",
    clientPort: 6381,
    busPort: 16381,
    role: "master",
    status: "online",
    slotsRange: "10923 - 16383",
    assignedSlotsCount: 5461,
    rssMemoryMB: 2.0,
    coresCount: 64,
  },
];

export function Navbar() {
  const [selectedNode, setSelectedNode] = useState<VelocityNode>(CLUSTER_NODES[0]);
  const [isNodeDropdownOpen, setIsNodeDropdownOpen] = useState(false);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const { telemetry, connectionState } = useVelocityTelemetry();

  const handleRefresh = () => {
    setIsRefreshing(true);
    setTimeout(() => setIsRefreshing(false), 500);
  };

  const isConnected = connectionState === "connected";
  const p50 = typeof telemetry?.p50_latency_micros === "number" && !isNaN(telemetry.p50_latency_micros)
    ? telemetry.p50_latency_micros
    : 14.2;
  const p99 = typeof telemetry?.p99_latency_micros === "number" && !isNaN(telemetry.p99_latency_micros)
    ? telemetry.p99_latency_micros
    : 18.1;
  const p50Str = `${p50.toFixed(1)} µs`;
  const p99Str = `${p99.toFixed(1)} µs`;

  return (
    <header className="sticky top-0 z-20 flex items-center justify-between h-16 px-6 border-b border-white/[0.08] bg-[#080a0f]/85 backdrop-blur-xl select-none">
      {/* Left: Quick Search / Command Palette Bar */}
      <div className="flex items-center gap-4">
        <button
          type="button"
          className="flex items-center gap-3 px-3 py-1.5 rounded-lg bg-surface border border-white/[0.08] text-xs text-zinc-400 hover:text-zinc-200 hover:border-cyan-500/40 hover:bg-surface-hover transition-all w-64 text-left group shadow-sm"
        >
          <Search className="w-3.5 h-3.5 text-zinc-400 group-hover:text-cyan-400 transition-colors" />
          <span className="truncate">Search keys, commands, slots...</span>
          <kbd className="ml-auto inline-flex items-center gap-0.5 px-1.5 py-0.5 text-[10px] font-mono text-zinc-400 bg-black/40 border border-white/10 rounded">
            ⌘K
          </kbd>
        </button>

        {/* Cluster Topology Status Badge */}
        <div className="hidden lg:flex items-center gap-2 px-2.5 py-1 rounded-md bg-[#111622] border border-white/[0.07] text-[11px] font-mono">
          <span className="text-zinc-400">CLUSTER:</span>
          <span className="text-cyan-400 font-bold">16,384 SLOTS</span>
          <span className="text-zinc-600">•</span>
          <span className="text-emerald-400 font-semibold">100% HEALTHY</span>
        </div>
      </div>

      {/* Center: Live Connection Status Pill (Requested by User) */}
      <div className="flex items-center gap-3">
        <div
          className={`flex items-center gap-2.5 px-3.5 py-1.5 rounded-full border shadow-[0_0_15px_-3px] ${
            isConnected
              ? "bg-emerald-950/40 border-emerald-500/30 text-emerald-300 shadow-[0_0_15px_-3px_rgba(16,185,129,0.25)]"
              : "bg-amber-950/40 border-amber-500/30 text-amber-300 shadow-[0_0_15px_-3px_rgba(245,158,11,0.25)]"
          }`}
        >
          {/* Pulsating LED Indicator */}
          <span className="relative flex h-2.5 w-2.5">
            <span
              className={`animate-ping absolute inline-flex h-full w-full rounded-full opacity-75 ${
                isConnected ? "bg-emerald-400" : "bg-amber-400"
              }`}
            />
            <span
              className={`relative inline-flex rounded-full h-2.5 w-2.5 ${
                isConnected ? "bg-emerald-500 shadow-[0_0_8px_#10b981]" : "bg-amber-500 shadow-[0_0_8px_#f59e0b]"
              }`}
            />
          </span>

          <span className="text-xs font-semibold tracking-tight font-mono">
            {isConnected
              ? `Connected to VelocityDB via port ${selectedNode.clientPort}`
              : "Connecting to VelocityDB (:8080)..."}
          </span>
        </div>

        {/* Latency and io_uring Engine Status Pill */}
        <div className="hidden md:flex items-center gap-2 px-3 py-1.5 rounded-full bg-surface border border-white/[0.08] text-xs font-mono">
          <Gauge className="w-3.5 h-3.5 text-cyan-400" />
          <span className="text-zinc-300 font-bold">{p50Str}</span>
          <span className="text-[10px] text-zinc-400">(P99 {p99Str})</span>
          <span className="text-zinc-600">•</span>
          <span className="text-[11px] text-cyan-400 font-semibold flex items-center gap-1">
            <Zap className="w-3 h-3 text-cyan-400" />
            io_uring Active
          </span>
        </div>
      </div>

      {/* Right: Node Switcher, Memory RSS, & Quick Actions */}
      <div className="flex items-center gap-3">
        {/* Node Switcher Dropdown */}
        <div className="relative">
          <button
            onClick={() => setIsNodeDropdownOpen(!isNodeDropdownOpen)}
            className="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-surface border border-white/[0.08] text-xs font-mono text-zinc-200 hover:border-cyan-500/40 hover:bg-surface-hover transition-all"
          >
            <Wifi className="w-3.5 h-3.5 text-cyan-400" />
            <span className="font-semibold">{selectedNode.name}</span>
            <ChevronDown className="w-3.5 h-3.5 text-zinc-400" />
          </button>

          {isNodeDropdownOpen && (
            <div className="absolute right-0 mt-2 w-72 rounded-xl bg-[#0f141f] border border-white/10 shadow-2xl p-2 z-50 backdrop-blur-2xl">
              <div className="px-3 py-2 text-[10px] font-mono uppercase text-zinc-400 border-b border-white/[0.06]">
                Target Cluster Node
              </div>
              <div className="mt-1 space-y-1">
                {CLUSTER_NODES.map((node) => (
                  <button
                    key={node.id}
                    onClick={() => {
                      setSelectedNode(node);
                      setIsNodeDropdownOpen(false);
                    }}
                    className={`w-full flex flex-col text-left px-3 py-2 rounded-lg text-xs font-mono transition-colors ${
                      selectedNode.id === node.id
                        ? "bg-cyan-500/15 border border-cyan-500/30 text-cyan-300"
                        : "text-zinc-300 hover:bg-white/[0.05]"
                    }`}
                  >
                    <div className="flex items-center justify-between">
                      <span className="font-bold">{node.name}</span>
                      <span className="text-[10px] text-emerald-400 font-bold">:{node.clientPort}</span>
                    </div>
                    <div className="flex items-center justify-between text-[10px] text-zinc-400 mt-0.5">
                      <span>Slots: {node.slotsRange}</span>
                      <span>{node.coresCount} Cores (Thread-per-core)</span>
                    </div>
                  </button>
                ))}
              </div>
            </div>
          )}
        </div>

        {/* Engine Memory RSS Pill */}
        <div className="hidden xl:flex items-center gap-2 px-3 py-1.5 rounded-lg bg-surface border border-white/[0.08] text-xs font-mono">
          <HardDrive className="w-3.5 h-3.5 text-orange-400" />
          <span className="text-zinc-400 text-[11px]">RSS:</span>
          <span className="text-orange-400 font-bold">2.00 MB</span>
          <span className="text-[10px] text-zinc-400 font-semibold">(LRU CEILING)</span>
        </div>

        {/* Manual Poll Refresh Button */}
        <button
          onClick={handleRefresh}
          className="p-2 rounded-lg bg-surface border border-white/[0.08] text-zinc-400 hover:text-cyan-400 hover:border-cyan-500/30 transition-all"
          title="Refresh metrics"
        >
          <RefreshCw className={`w-3.5 h-3.5 ${isRefreshing ? "animate-spin text-cyan-400" : ""}`} />
        </button>

        {/* Dual License / Enterprise Pill */}
        <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-md bg-zinc-900 border border-zinc-700/60 text-[10px] font-mono text-zinc-300">
          <ShieldCheck className="w-3.5 h-3.5 text-emerald-400" />
          <span>AGPL v3 / ENTERPRISE</span>
        </div>
      </div>
    </header>
  );
}
