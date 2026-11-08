"use client";

import React, { useState } from "react";
import {
  Database,
  Search,
  Filter,
  Trash2,
  Edit3,
  Copy,
  Clock,
  HardDrive,
  Hash,
  List,
  Layers,
  Radio,
  Binary,
  Check,
  Plus,
} from "lucide-react";
import { KeyRecord, KeyType } from "@/types";

const INITIAL_KEYS: KeyRecord[] = [
  {
    key: "{tenant:101}session:user_token:9941",
    type: "string",
    slot: 4921,
    targetNode: "node-01:6379",
    ttlSeconds: 3412,
    sizeBytes: 256,
    lastAccessed: "2 secs ago",
  },
  {
    key: "{user:8912}profile_metadata",
    type: "hash",
    slot: 8195,
    targetNode: "node-02:6380",
    ttlSeconds: null,
    sizeBytes: 1048,
    lastAccessed: "14 secs ago",
  },
  {
    key: "{orders:queue}pending_checkouts",
    type: "list",
    slot: 2100,
    targetNode: "node-01:6379",
    ttlSeconds: 86400,
    sizeBytes: 4096,
    lastAccessed: "1 min ago",
  },
  {
    key: "{game:leaderboard}season_14_scores",
    type: "zset",
    slot: 14201,
    targetNode: "node-03:6381",
    ttlSeconds: null,
    sizeBytes: 8192,
    lastAccessed: "5 mins ago",
  },
  {
    key: "{stream:events}telemetry_ingest_v1",
    type: "stream",
    slot: 1284,
    targetNode: "node-01:6379",
    ttlSeconds: null,
    sizeBytes: 32768,
    lastAccessed: "Just now",
  },
  {
    key: "{analytics:daily}active_users_bitmap",
    type: "bitmap",
    slot: 3410,
    targetNode: "node-01:6379",
    ttlSeconds: 43200,
    sizeBytes: 65536,
    lastAccessed: "Just now",
  },
];

export default function ExplorerPage() {
  const [keys] = useState<KeyRecord[]>(INITIAL_KEYS);
  const [searchQuery, setSearchQuery] = useState("");
  const [selectedType, setSelectedType] = useState<KeyType | "all">("all");
  const [selectedKey, setSelectedKey] = useState<KeyRecord>(INITIAL_KEYS[0]);
  const [copied, setCopied] = useState(false);

  const filteredKeys = keys.filter((k) => {
    const matchesSearch = k.key.toLowerCase().includes(searchQuery.toLowerCase());
    const matchesType = selectedType === "all" || k.type === selectedType;
    return matchesSearch && matchesType;
  });

  const handleCopy = () => {
    navigator.clipboard.writeText(selectedKey.key);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  return (
    <div className="space-y-6 animate-in fade-in duration-300">
      {/* Page Header */}
      <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4 pb-2 border-b border-white/[0.06]">
        <div>
          <div className="flex items-center gap-2.5">
            <h1 className="text-2xl font-black tracking-tight text-white font-mono flex items-center gap-2">
              DATA EXPLORER
            </h1>
            <span className="px-2 py-0.5 text-[10px] font-mono font-bold uppercase rounded bg-cyan-950/80 text-cyan-300 border border-cyan-800/60">
              16,384 HASH SLOTS
            </span>
          </div>
          <p className="text-xs text-zinc-400 font-mono mt-1">
            Zero-copy direct inspection across Strings, Hashes, Lists, Sorted Sets, Streams, and SIMD Bitmaps.
          </p>
        </div>

        <button className="flex items-center gap-2 px-3.5 py-2 rounded-xl bg-cyan-500 text-black font-mono font-bold text-xs hover:bg-cyan-400 transition-colors shadow-[0_0_15px_-3px_rgba(0,242,254,0.4)]">
          <Plus className="w-4 h-4" />
          <span>NEW KEY</span>
        </button>
      </div>

      {/* Main Explorer Two-Pane Layout */}
      <div className="grid grid-cols-1 lg:grid-cols-12 gap-6 h-[calc(100vh-220px)]">
        {/* Left Pane: Key List & Filters (5 cols) */}
        <div className="lg:col-span-5 glass-panel rounded-2xl p-4 flex flex-col space-y-4 overflow-hidden">
          {/* Search Bar */}
          <div className="relative">
            <Search className="w-4 h-4 absolute left-3 top-1/2 -translate-y-1/2 text-zinc-400" />
            <input
              type="text"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder="Search keys by pattern (e.g. {user:*)..."
              className="w-full pl-9 pr-4 py-2 bg-black/40 border border-white/[0.08] rounded-xl text-xs font-mono text-white placeholder:text-zinc-500 focus:outline-none focus:border-cyan-500/50"
            />
          </div>

          {/* Type Filter Pills */}
          <div className="flex items-center gap-1.5 overflow-x-auto pb-1 text-[11px] font-mono scrollbar-none">
            {(["all", "string", "hash", "list", "zset", "stream", "bitmap"] as const).map((t) => (
              <button
                key={t}
                onClick={() => setSelectedType(t)}
                className={`px-2.5 py-1 rounded-lg uppercase font-semibold transition-colors shrink-0 ${
                  selectedType === t
                    ? "bg-cyan-500/20 text-cyan-300 border border-cyan-500/40"
                    : "text-zinc-400 hover:text-white bg-white/[0.03] border border-transparent"
                }`}
              >
                {t}
              </button>
            ))}
          </div>

          {/* Key List Table */}
          <div className="flex-1 overflow-y-auto space-y-1.5 pr-1">
            {filteredKeys.map((k) => {
              const isSelected = selectedKey.key === k.key;
              return (
                <button
                  key={k.key}
                  onClick={() => setSelectedKey(k)}
                  className={`w-full text-left p-3 rounded-xl border font-mono transition-all ${
                    isSelected
                      ? "bg-cyan-500/10 border-cyan-500/40 shadow-[0_0_15px_-4px_rgba(0,242,254,0.15)]"
                      : "bg-[#0d121c]/60 border-white/[0.05] hover:bg-white/[0.03]"
                  }`}
                >
                  <div className="flex items-center justify-between">
                    <span className="text-[10px] font-bold px-1.5 py-0.5 rounded uppercase bg-black/50 text-cyan-400 border border-white/10">
                      {k.type}
                    </span>
                    <span className="text-[10px] text-purple-400 font-bold">
                      Slot {k.slot}
                    </span>
                  </div>

                  <div className="text-xs font-semibold text-white mt-1.5 truncate">
                    {k.key}
                  </div>

                  <div className="flex items-center justify-between text-[10px] text-zinc-400 mt-2">
                    <span>{k.targetNode}</span>
                    <span>{k.sizeBytes} Bytes</span>
                  </div>
                </button>
              );
            })}
          </div>
        </div>

        {/* Right Pane: Key Value & Metadata Inspector (7 cols) */}
        <div className="lg:col-span-7 glass-panel rounded-2xl p-6 flex flex-col justify-between overflow-y-auto space-y-6">
          <div className="space-y-6">
            {/* Key Title Header */}
            <div className="flex items-start justify-between gap-4 pb-4 border-b border-white/[0.08]">
              <div className="space-y-1">
                <span className="text-[10px] font-mono uppercase tracking-wider text-cyan-400 font-bold">
                  {selectedKey.type.toUpperCase()} KEY RECORD
                </span>
                <h2 className="text-lg font-bold font-mono text-white break-all flex items-center gap-2">
                  <span>{selectedKey.key}</span>
                  <button
                    onClick={handleCopy}
                    className="p-1 rounded text-zinc-400 hover:text-cyan-400 transition-colors"
                    title="Copy Key"
                  >
                    {copied ? <Check className="w-4 h-4 text-emerald-400" /> : <Copy className="w-4 h-4" />}
                  </button>
                </h2>
              </div>

              <div className="flex items-center gap-2">
                <button className="p-2 rounded-xl bg-white/[0.05] border border-white/[0.08] text-zinc-300 hover:text-white hover:border-white/20 transition-colors">
                  <Edit3 className="w-4 h-4" />
                </button>
                <button className="p-2 rounded-xl bg-red-950/30 border border-red-800/40 text-red-400 hover:bg-red-950/60 transition-colors">
                  <Trash2 className="w-4 h-4" />
                </button>
              </div>
            </div>

            {/* Key Properties Grid */}
            <div className="grid grid-cols-2 sm:grid-cols-4 gap-3 font-mono text-xs">
              <div className="p-3 rounded-xl bg-[#0d121c] border border-white/[0.06]">
                <span className="text-[10px] text-zinc-400 uppercase block">Slot Number</span>
                <span className="text-purple-400 font-bold text-sm">{selectedKey.slot}</span>
              </div>
              <div className="p-3 rounded-xl bg-[#0d121c] border border-white/[0.06]">
                <span className="text-[10px] text-zinc-400 uppercase block">Hosting Node</span>
                <span className="text-cyan-400 font-bold text-sm">{selectedKey.targetNode}</span>
              </div>
              <div className="p-3 rounded-xl bg-[#0d121c] border border-white/[0.06]">
                <span className="text-[10px] text-zinc-400 uppercase block">Time-To-Live</span>
                <span className="text-amber-400 font-bold text-sm">
                  {selectedKey.ttlSeconds ? `${selectedKey.ttlSeconds}s` : "Persistent"}
                </span>
              </div>
              <div className="p-3 rounded-xl bg-[#0d121c] border border-white/[0.06]">
                <span className="text-[10px] text-zinc-400 uppercase block">Memory Size</span>
                <span className="text-emerald-400 font-bold text-sm">{selectedKey.sizeBytes} Bytes</span>
              </div>
            </div>

            {/* Raw Value Payload Preview */}
            <div className="space-y-2">
              <div className="flex items-center justify-between text-xs font-mono">
                <span className="text-zinc-400 uppercase">Value Payload (JSON / Binary Safe)</span>
                <span className="text-zinc-500">Encoding: raw-utf8</span>
              </div>

              <div className="p-4 rounded-xl bg-black/60 border border-white/[0.08] font-mono text-xs text-zinc-300 overflow-x-auto leading-relaxed">
                <pre>
{`{
  "tenant_id": 101,
  "user_id": 9941,
  "role": "enterprise_admin",
  "auth_token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...",
  "permissions": [
    "cluster:read",
    "cluster:write",
    "acl:admin",
    "replication:sync"
  ],
  "session_created": 1731057400,
  "ip_address": "192.168.1.100"
}`}
                </pre>
              </div>
            </div>
          </div>

          {/* Quick Command Helper at Bottom */}
          <div className="p-3 rounded-xl bg-[#0d121c] border border-white/[0.06] text-xs font-mono flex items-center justify-between">
            <span className="text-zinc-400">Terminal Command:</span>
            <code className="text-cyan-400 font-bold">GET {selectedKey.key}</code>
          </div>
        </div>
      </div>
    </div>
  );
}
