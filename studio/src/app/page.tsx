"use client";

import React, { useState, useEffect } from "react";
import {
  Zap,
  Gauge,
  HardDrive,
  Network,
  Activity,
  ArrowUpRight,
  ShieldCheck,
  Cpu,
  Layers,
  Terminal,
  Database,
  CheckCircle2,
  Lock,
} from "lucide-react";
import {
  AreaChart,
  Area,
  XAxis,
  YAxis,
  Tooltip,
  ResponsiveContainer,
  CartesianGrid,
} from "recharts";

interface LiveMetricPoint {
  time: string;
  rps: number;
  p99Latency: number;
  evictions: number;
}

const INITIAL_GRAPH_DATA: LiveMetricPoint[] = [
  { time: "11:50:00", rps: 1540000, p99Latency: 17.8, evictions: 120 },
  { time: "11:51:00", rps: 1580000, p99Latency: 18.2, evictions: 240 },
  { time: "11:52:00", rps: 1620000, p99Latency: 17.5, evictions: 310 },
  { time: "11:53:00", rps: 1610000, p99Latency: 19.1, evictions: 450 },
  { time: "11:54:00", rps: 1640000, p99Latency: 18.0, evictions: 580 },
  { time: "11:55:00", rps: 1675000, p99Latency: 18.4, evictions: 690 },
  { time: "11:56:00", rps: 1648392, p99Latency: 18.1, evictions: 820 },
];

const CORES_DATA = Array.from({ length: 16 }, (_, i) => ({
  coreId: i,
  loadPercent: Math.floor(75 + Math.random() * 20),
  opsSec: Math.floor(95000 + Math.random() * 15000),
  sqeDepth: Math.floor(32 + Math.random() * 20),
  cqePending: Math.floor(10 + Math.random() * 10),
  isLeader: i === 0,
}));

const RECENT_COMMANDS = [
  { id: "tx-901", cmd: "SET", key: "{tenant:101}session:auth_token", slot: 4921, node: "node-01:6379", size: "256 B", latency: "4.2 µs", status: "COMMITTED" },
  { id: "tx-902", cmd: "XADD", key: "{stream:orders}telemetry:event", slot: 1284, node: "node-01:6379", size: "1.2 KB", latency: "5.8 µs", status: "STREAMED" },
  { id: "tx-903", cmd: "HSET", key: "{user:8912}profile_metadata", slot: 8195, node: "node-02:6380", size: "512 B", latency: "6.1 µs", status: "-MOVED REDIRECT" },
  { id: "tx-904", cmd: "ZADD", key: "{game:leaderboard}highscores", slot: 14201, node: "node-03:6381", size: "48 B", latency: "3.9 µs", status: "-MOVED REDIRECT" },
  { id: "tx-905", cmd: "GETBIT", key: "{analytics:daily}active_users_bitmap", slot: 3410, node: "node-01:6379", size: "64 B", latency: "1.2 µs", status: "SIMD POPCNT" },
];

export default function DashboardPage() {
  const [data, setData] = useState<LiveMetricPoint[]>(INITIAL_GRAPH_DATA);
  const [activeTab, setActiveTab] = useState<"1m" | "5m" | "15m" | "live">("live");

  // Dynamic live metric simulation
  useEffect(() => {
    const interval = setInterval(() => {
      const now = new Date();
      const timeStr = now.toTimeString().split(" ")[0];
      const nextRps = Math.floor(1620000 + Math.random() * 60000);
      const nextLatency = parseFloat((17.5 + Math.random() * 1.5).toFixed(1));
      
      setData((prev) => [
        ...prev.slice(1),
        {
          time: timeStr,
          rps: nextRps,
          p99Latency: nextLatency,
          evictions: prev[prev.length - 1].evictions + Math.floor(Math.random() * 15),
        },
      ]);
    }, 2000);
    return () => clearInterval(interval);
  }, []);

  const currentPoint = data[data.length - 1];

  return (
    <div className="space-y-8 animate-in fade-in duration-300">
      {/* Top Banner / Welcome Ribbon */}
      <div className="flex flex-col md:flex-row items-start md:items-center justify-between gap-4 pb-2 border-b border-white/[0.06]">
        <div>
          <div className="flex items-center gap-2.5">
            <h1 className="text-2xl font-black tracking-tight text-white font-mono flex items-center gap-2">
              ENGINE DASHBOARD
            </h1>
            <span className="px-2 py-0.5 text-[10px] font-mono font-bold uppercase rounded bg-cyan-950/80 text-cyan-300 border border-cyan-800/60 shadow-[0_0_10px_rgba(0,242,254,0.2)]">
              HIGH-THROUGHPUT CLUSTER
            </span>
          </div>
          <p className="text-xs text-zinc-400 font-mono mt-1">
            Real-time telemetry streaming from Linux io_uring submission queues across 64 pinned CPU shards.
          </p>
        </div>

        {/* Global Cluster Status Pill */}
        <div className="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-[#0e131d] border border-white/[0.08] shadow-sm">
          <ShieldCheck className="w-4 h-4 text-emerald-400" />
          <span className="text-xs font-mono text-zinc-300">
            Cluster Mode: <strong className="text-emerald-400">16,384 Slots OK</strong>
          </span>
          <span className="text-zinc-600">|</span>
          <span className="text-xs font-mono text-cyan-400">Lock-Free Gossip Bus Active</span>
        </div>
      </div>

      {/* 4 Hero Stats Cards (Silicon Valley Style) */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
        {/* Card 1: Live RPS */}
        <div className="glass-card-interactive p-5 rounded-2xl relative overflow-hidden group">
          <div className="flex items-center justify-between text-zinc-400 mb-2">
            <span className="text-xs font-mono uppercase tracking-wider font-semibold">Throughput (RPS)</span>
            <div className="p-2 rounded-xl bg-cyan-500/10 border border-cyan-500/20 text-cyan-400">
              <Zap className="w-4 h-4" />
            </div>
          </div>
          <div className="flex items-baseline gap-2">
            <span className="text-3xl font-black font-mono text-white tracking-tight">
              {(currentPoint.rps / 1_000_000).toFixed(2)}M
            </span>
            <span className="text-xs font-mono text-zinc-400">ops / sec</span>
          </div>
          <div className="flex items-center gap-1.5 mt-3 text-[11px] font-mono text-emerald-400">
            <ArrowUpRight className="w-3.5 h-3.5" />
            <span className="font-bold">24.57M peak</span>
            <span className="text-zinc-400">(Zero-syscall io_uring)</span>
          </div>
          <div className="absolute -right-4 -bottom-4 w-24 h-24 bg-cyan-500/5 rounded-full blur-2xl group-hover:bg-cyan-500/10 transition-colors" />
        </div>

        {/* Card 2: P99 Tail Latency */}
        <div className="glass-card-interactive p-5 rounded-2xl relative overflow-hidden group">
          <div className="flex items-center justify-between text-zinc-400 mb-2">
            <span className="text-xs font-mono uppercase tracking-wider font-semibold">P99 Tail Latency</span>
            <div className="p-2 rounded-xl bg-emerald-500/10 border border-emerald-500/20 text-emerald-400">
              <Gauge className="w-4 h-4" />
            </div>
          </div>
          <div className="flex items-baseline gap-2">
            <span className="text-3xl font-black font-mono text-white tracking-tight">
              {currentPoint.p99Latency}
            </span>
            <span className="text-xs font-mono text-emerald-400 font-bold">µs (Microseconds)</span>
          </div>
          <div className="flex items-center gap-1.5 mt-3 text-[11px] font-mono text-cyan-400">
            <span className="font-bold text-white">276x Faster</span>
            <span className="text-zinc-400">than Redis 7.2 (1,250 µs)</span>
          </div>
          <div className="absolute -right-4 -bottom-4 w-24 h-24 bg-emerald-500/5 rounded-full blur-2xl group-hover:bg-emerald-500/10 transition-colors" />
        </div>

        {/* Card 3: Memory RSS Ceiling */}
        <div className="glass-card-interactive p-5 rounded-2xl relative overflow-hidden group">
          <div className="flex items-center justify-between text-zinc-400 mb-2">
            <span className="text-xs font-mono uppercase tracking-wider font-semibold">Memory RSS Ceiling</span>
            <div className="p-2 rounded-xl bg-orange-500/10 border border-orange-500/20 text-orange-400">
              <HardDrive className="w-4 h-4" />
            </div>
          </div>
          <div className="flex items-baseline gap-2">
            <span className="text-3xl font-black font-mono text-white tracking-tight">
              2.00 MB
            </span>
            <span className="text-xs font-mono text-zinc-400">/ 2.00 MB Limit</span>
          </div>
          {/* Progress bar */}
          <div className="w-full bg-white/[0.07] h-1.5 rounded-full mt-3 overflow-hidden">
            <div className="bg-gradient-to-r from-orange-500 to-amber-400 h-full rounded-full w-full" />
          </div>
          <div className="flex items-center justify-between mt-2 text-[10px] font-mono text-zinc-400">
            <span>O(1) LRU Synchronous</span>
            <span className="text-orange-400 font-bold">43,740 Evictions</span>
          </div>
          <div className="absolute -right-4 -bottom-4 w-24 h-24 bg-orange-500/5 rounded-full blur-2xl group-hover:bg-orange-500/10 transition-colors" />
        </div>

        {/* Card 4: Consistent Hash Routing */}
        <div className="glass-card-interactive p-5 rounded-2xl relative overflow-hidden group">
          <div className="flex items-center justify-between text-zinc-400 mb-2">
            <span className="text-xs font-mono uppercase tracking-wider font-semibold">Hash Slots & Bus</span>
            <div className="p-2 rounded-xl bg-purple-500/10 border border-purple-500/20 text-purple-400">
              <Network className="w-4 h-4" />
            </div>
          </div>
          <div className="flex items-baseline gap-2">
            <span className="text-3xl font-black font-mono text-white tracking-tight">
              16,384
            </span>
            <span className="text-xs font-mono text-purple-400 font-bold">Slots Assigned</span>
          </div>
          <div className="flex items-center gap-1.5 mt-3 text-[11px] font-mono text-zinc-300">
            <span className="w-2 h-2 rounded-full bg-emerald-400 inline-block animate-pulse" />
            <span>3 Master Nodes • 0% Loss</span>
          </div>
          <div className="absolute -right-4 -bottom-4 w-24 h-24 bg-purple-500/5 rounded-full blur-2xl group-hover:bg-purple-500/10 transition-colors" />
        </div>
      </div>

      {/* Main Charts & Live Feed Grid */}
      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        {/* Left 2 Cols: Real-Time Throughput & Latency Chart */}
        <div className="lg:col-span-2 glass-panel p-6 rounded-2xl flex flex-col justify-between">
          <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4 pb-4 border-b border-white/[0.06]">
            <div>
              <div className="flex items-center gap-2">
                <Activity className="w-4 h-4 text-cyan-400" />
                <h3 className="text-sm font-black font-mono tracking-tight text-white uppercase">
                  Live Throughput &amp; P99 Latency Stream
                </h3>
              </div>
              <p className="text-xs text-zinc-400 font-mono mt-0.5">
                Sampling socket rings at 2-second intervals via glommio ring metrics.
              </p>
            </div>

            {/* Time Window Tabs */}
            <div className="flex items-center p-1 rounded-lg bg-black/40 border border-white/[0.08] text-xs font-mono">
              {(["1m", "5m", "15m", "live"] as const).map((tab) => (
                <button
                  key={tab}
                  onClick={() => setActiveTab(tab)}
                  className={`px-2.5 py-1 rounded uppercase font-semibold transition-colors ${
                    activeTab === tab
                      ? "bg-cyan-500/20 text-cyan-300 border border-cyan-500/40"
                      : "text-zinc-400 hover:text-white"
                  }`}
                >
                  {tab}
                </button>
              ))}
            </div>
          </div>

          {/* Recharts Area Chart */}
          <div className="h-64 w-full mt-4">
            <ResponsiveContainer width="100%" height="100%">
              <AreaChart data={data}>
                <defs>
                  <linearGradient id="cyanRpsGrad" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0%" stopColor="#00f2fe" stopOpacity={0.3} />
                    <stop offset="100%" stopColor="#00f2fe" stopOpacity={0.0} />
                  </linearGradient>
                </defs>
                <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.05)" />
                <XAxis dataKey="time" stroke="#4b5563" tick={{ fill: "#6b7280", fontSize: 10, fontFamily: "monospace" }} />
                <YAxis
                  stroke="#4b5563"
                  tick={{ fill: "#6b7280", fontSize: 10, fontFamily: "monospace" }}
                  tickFormatter={(val) => `${(val / 1000000).toFixed(1)}M`}
                />
                <Tooltip
                  contentStyle={{
                    backgroundColor: "#0d1117",
                    borderColor: "rgba(255,255,255,0.1)",
                    borderRadius: "10px",
                    fontFamily: "monospace",
                    fontSize: "12px",
                  }}
                  formatter={(value: any) => [`${Number(value).toLocaleString()} ops/sec`, "RPS"]}
                />
                <Area
                  type="monotone"
                  dataKey="rps"
                  stroke="#00f2fe"
                  strokeWidth={2}
                  fill="url(#cyanRpsGrad)"
                  isAnimationActive={false}
                />
              </AreaChart>
            </ResponsiveContainer>
          </div>

          <div className="grid grid-cols-3 gap-4 pt-4 border-t border-white/[0.06] text-center font-mono">
            <div>
              <span className="text-[10px] text-zinc-400 uppercase block">Min P50 Latency</span>
              <span className="text-emerald-400 font-bold text-sm">4.5 µs</span>
            </div>
            <div>
              <span className="text-[10px] text-zinc-400 uppercase block">Average P90</span>
              <span className="text-cyan-400 font-bold text-sm">9.2 µs</span>
            </div>
            <div>
              <span className="text-[10px] text-zinc-400 uppercase block">Current P99 SLA</span>
              <span className="text-white font-bold text-sm">{currentPoint.p99Latency} µs</span>
            </div>
          </div>
        </div>

        {/* Right 1 Col: Thread-per-Core CPU Matrix (Silicon Valley Feature) */}
        <div className="glass-panel p-6 rounded-2xl flex flex-col justify-between">
          <div>
            <div className="flex items-center justify-between pb-3 border-b border-white/[0.06]">
              <div className="flex items-center gap-2">
                <Cpu className="w-4 h-4 text-orange-400" />
                <h3 className="text-sm font-black font-mono tracking-tight text-white uppercase">
                  Thread-per-Core Shards
                </h3>
              </div>
              <span className="text-[10px] font-mono text-emerald-400 font-bold uppercase bg-emerald-950/80 px-2 py-0.5 rounded border border-emerald-800/60">
                16 Shards Live
              </span>
            </div>
            <p className="text-[11px] text-zinc-400 font-mono mt-2">
              Each core owns an isolated <code className="text-cyan-400">LocalCache</code> with 0 cross-thread locks.
            </p>

            {/* Core Affinity Grid */}
            <div className="grid grid-cols-4 gap-2 mt-4">
              {CORES_DATA.map((core) => (
                <div
                  key={core.coreId}
                  className={`p-2 rounded-lg border text-center font-mono transition-all ${
                    core.isLeader
                      ? "bg-cyan-950/40 border-cyan-500/40 shadow-[0_0_10px_rgba(0,242,254,0.15)]"
                      : "bg-[#101520] border-white/[0.06] hover:border-white/20"
                  }`}
                >
                  <div className="flex items-center justify-between text-[9px] text-zinc-400">
                    <span>C{core.coreId}</span>
                    {core.isLeader ? (
                      <span className="text-[8px] text-cyan-400 font-bold">BUS</span>
                    ) : (
                      <span className="text-[8px] text-emerald-400">OK</span>
                    )}
                  </div>
                  <div className="text-xs font-bold text-white mt-0.5">
                    {core.loadPercent}%
                  </div>
                  <div className="w-full bg-white/10 h-1 rounded-full mt-1 overflow-hidden">
                    <div
                      className="bg-cyan-400 h-full rounded-full"
                      style={{ width: `${core.loadPercent}%` }}
                    />
                  </div>
                </div>
              ))}
            </div>
          </div>

          <div className="p-3 mt-4 rounded-xl bg-black/40 border border-white/[0.07] text-[11px] font-mono flex items-center justify-between">
            <span className="text-zinc-400">Kernel Mechanism:</span>
            <span className="text-orange-400 font-bold">SO_REUSEPORT + io_uring</span>
          </div>
        </div>
      </div>

      {/* Cluster Nodes & Hash Slot Distribution */}
      <div className="glass-panel p-6 rounded-2xl space-y-4">
        <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-2 pb-3 border-b border-white/[0.06]">
          <div className="flex items-center gap-2">
            <Layers className="w-4 h-4 text-purple-400" />
            <h3 className="text-sm font-black font-mono tracking-tight text-white uppercase">
              Distributed Cluster Topology &amp; Hash Slot Balance
            </h3>
          </div>
          <span className="text-xs font-mono text-zinc-400">
            Gossip Bus Protocol: <strong className="text-cyan-400">RCmb Async</strong>
          </span>
        </div>

        {/* 3 Nodes Slot Bar */}
        <div className="w-full h-8 rounded-xl bg-[#090d16] border border-white/10 p-1 flex gap-1 overflow-hidden">
          <div
            className="h-full rounded-lg bg-gradient-to-r from-cyan-500/80 to-blue-600/80 flex items-center justify-center text-[10px] font-mono font-bold text-white shadow-sm"
            style={{ width: "33.33%" }}
            title="Node 1: Slots 0 to 5461"
          >
            Node-01: Slots 0-5461 (33.3%)
          </div>
          <div
            className="h-full rounded-lg bg-gradient-to-r from-purple-500/80 to-pink-600/80 flex items-center justify-center text-[10px] font-mono font-bold text-white shadow-sm"
            style={{ width: "33.33%" }}
            title="Node 2: Slots 5462 to 10922"
          >
            Node-02: Slots 5462-10922 (33.3%)
          </div>
          <div
            className="h-full rounded-lg bg-gradient-to-r from-orange-500/80 to-amber-600/80 flex items-center justify-center text-[10px] font-mono font-bold text-white shadow-sm"
            style={{ width: "33.34%" }}
            title="Node 3: Slots 10923 to 16383"
          >
            Node-03: Slots 10923-16383 (33.4%)
          </div>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-3 gap-4 pt-2">
          {[
            { name: "node-01.aws-us-east", port: 6379, bus: 16379, slots: "0 - 5461", status: "LEADER", color: "text-cyan-400" },
            { name: "node-02.aws-us-east", port: 6380, bus: 16380, slots: "5462 - 10922", status: "MASTER", color: "text-purple-400" },
            { name: "node-03.aws-us-east", port: 6381, bus: 16381, slots: "10923 - 16383", status: "MASTER", color: "text-orange-400" },
          ].map((n, idx) => (
            <div key={idx} className="p-3.5 rounded-xl bg-surface border border-white/[0.07] font-mono text-xs">
              <div className="flex items-center justify-between">
                <span className={`font-bold ${n.color}`}>{n.name}</span>
                <span className="text-[10px] px-1.5 py-0.5 rounded bg-emerald-950 text-emerald-400 border border-emerald-800">
                  {n.status}
                </span>
              </div>
              <div className="mt-2 space-y-1 text-zinc-400 text-[11px]">
                <div className="flex justify-between">
                  <span>Client Port:</span>
                  <span className="text-zinc-200">:{n.port}</span>
                </div>
                <div className="flex justify-between">
                  <span>Cluster Bus:</span>
                  <span className="text-zinc-200">:{n.bus}</span>
                </div>
                <div className="flex justify-between">
                  <span>Assigned Slots:</span>
                  <span className="text-zinc-200 font-semibold">{n.slots}</span>
                </div>
              </div>
            </div>
          ))}
        </div>
      </div>

      {/* Live Transaction & Command Stream Table */}
      <div className="glass-panel p-6 rounded-2xl space-y-4">
        <div className="flex items-center justify-between pb-3 border-b border-white/[0.06]">
          <div className="flex items-center gap-2">
            <Terminal className="w-4 h-4 text-emerald-400" />
            <h3 className="text-sm font-black font-mono tracking-tight text-white uppercase">
              Live Ingest Stream &amp; Cluster Redirection Log
            </h3>
          </div>
          <span className="text-xs font-mono text-emerald-400 flex items-center gap-1.5">
            <span className="w-2 h-2 rounded-full bg-emerald-400 animate-ping" />
            Real-Time Wire Feed
          </span>
        </div>

        <div className="overflow-x-auto">
          <table className="w-full text-left font-mono text-xs">
            <thead>
              <tr className="border-b border-white/[0.08] text-zinc-400 text-[11px] uppercase">
                <th className="pb-3 font-semibold">Tx ID</th>
                <th className="pb-3 font-semibold">Command</th>
                <th className="pb-3 font-semibold">Key</th>
                <th className="pb-3 font-semibold">Slot #</th>
                <th className="pb-3 font-semibold">Node</th>
                <th className="pb-3 font-semibold">Size</th>
                <th className="pb-3 font-semibold">Latency</th>
                <th className="pb-3 font-semibold text-right">Routing Status</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/[0.04]">
              {RECENT_COMMANDS.map((tx) => (
                <tr key={tx.id} className="hover:bg-white/[0.02] transition-colors">
                  <td className="py-3 text-zinc-400">{tx.id}</td>
                  <td className="py-3">
                    <span className="px-1.5 py-0.5 rounded text-[10px] font-bold bg-cyan-950/80 text-cyan-300 border border-cyan-800/60">
                      {tx.cmd}
                    </span>
                  </td>
                  <td className="py-3 font-semibold text-zinc-200">{tx.key}</td>
                  <td className="py-3 text-purple-400 font-bold">{tx.slot}</td>
                  <td className="py-3 text-zinc-400">{tx.node}</td>
                  <td className="py-3 text-zinc-400">{tx.size}</td>
                  <td className="py-3 text-emerald-400 font-bold">{tx.latency}</td>
                  <td className="py-3 text-right">
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
    </div>
  );
}
