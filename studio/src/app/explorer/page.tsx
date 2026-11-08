"use client";

import React, { useState, useEffect } from "react";
import {
  Database,
  Search,
  Filter,
  Plus,
  Layers,
  Sparkles,
  HardDrive,
  Clock,
  RefreshCw,
  SlidersHorizontal,
} from "lucide-react";
import { KeyRecord, KeyType } from "@/types";
import { KeyTable } from "@/components/explorer/KeyTable";
import { KeyDrawer } from "@/components/explorer/KeyDrawer";
import { NewKeyModal } from "@/components/explorer/NewKeyModal";

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
  {
    key: "{tenant:102}rate_limit:ip_192.168.1.50",
    type: "string",
    slot: 5012,
    targetNode: "node-01:6379",
    ttlSeconds: 58,
    sizeBytes: 64,
    lastAccessed: "Just now",
  },
  {
    key: "{catalog:inventory}sku_994812",
    type: "hash",
    slot: 9412,
    targetNode: "node-02:6380",
    ttlSeconds: null,
    sizeBytes: 512,
    lastAccessed: "12 mins ago",
  },
  {
    key: "{pubsub:backlog}replica_sync_stream",
    type: "stream",
    slot: 10950,
    targetNode: "node-03:6381",
    ttlSeconds: 1200,
    sizeBytes: 16384,
    lastAccessed: "10 secs ago",
  },
];

export default function ExplorerPage() {
  const [keys, setKeys] = useState<KeyRecord[]>(INITIAL_KEYS);
  const [searchQuery, setSearchQuery] = useState("");
  const [selectedType, setSelectedType] = useState<KeyType | "all">("all");
  const [selectedNode, setSelectedNode] = useState<string>("all");
  const [activeDrawerKey, setActiveDrawerKey] = useState<KeyRecord | null>(null);
  const [isDrawerOpen, setIsDrawerOpen] = useState(false);
  const [isModalOpen, setIsModalOpen] = useState(false);

  // Sync with embedded VelocityDB REST management API (port 8080)
  useEffect(() => {
    fetch("http://localhost:8080/api/keys")
      .then((res) => {
        if (!res.ok) throw new Error("API status not ok");
        return res.json();
      })
      .then((data) => {
        if (Array.isArray(data) && data.length > 0) {
          const remoteKeys: KeyRecord[] = data.map((d: {
            key: string;
            key_type: string;
            slot: number;
            target_node: string;
            ttl_seconds: number | null;
            size_bytes: number;
            last_accessed: string;
          }) => ({
            key: d.key,
            type: (d.key_type.toLowerCase() || "string") as KeyType,
            slot: d.slot,
            targetNode: d.target_node,
            ttlSeconds: d.ttl_seconds,
            sizeBytes: d.size_bytes,
            lastAccessed: d.last_accessed || "Just now",
          }));
          setKeys(remoteKeys);
        }
      })
      .catch(() => {
        // Quiet fallback to INITIAL_KEYS when engine offline
      });
  }, []);

  // Filter keys based on search and dropdown filters
  const filteredKeys = keys.filter((k) => {
    const matchesSearch =
      k.key.toLowerCase().includes(searchQuery.toLowerCase()) ||
      String(k.slot).includes(searchQuery);

    const matchesType = selectedType === "all" || k.type === selectedType;
    const matchesNode = selectedNode === "all" || k.targetNode.includes(selectedNode);

    return matchesSearch && matchesType && matchesNode;
  });

  const handleCreateKey = (newKey: {
    key: string;
    type: KeyType;
    ttlSeconds: number | null;
    value: string;
  }) => {
    // Compute simple deterministic slot (0 to 16383)
    const slot = Math.abs(
      newKey.key.split("").reduce((acc, c) => (acc << 5) - acc + c.charCodeAt(0), 0)
    ) % 16384;

    const targetNode =
      slot < 5462
        ? "node-01:6379"
        : slot < 10923
        ? "node-02:6380"
        : "node-03:6381";

    const createdRecord: KeyRecord = {
      key: newKey.key,
      type: newKey.type,
      slot,
      targetNode,
      ttlSeconds: newKey.ttlSeconds,
      sizeBytes: new TextEncoder().encode(newKey.value).length,
      lastAccessed: "Just now",
    };

    setKeys((prev) => [createdRecord, ...prev]);
    setActiveDrawerKey(createdRecord);
    setIsDrawerOpen(true);

    // Call REST endpoint
    fetch("http://localhost:8080/api/keys", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        key: newKey.key,
        key_type: newKey.type,
        ttl_seconds: newKey.ttlSeconds,
        value: newKey.value,
      }),
    }).catch(() => {});
  };

  const handleUpdateValue = (keyName: string, newValue: string, newTtl: number | null) => {
    setKeys((prev) =>
      prev.map((k) => {
        if (k.key === keyName) {
          return {
            ...k,
            ttlSeconds: newTtl,
            sizeBytes: new TextEncoder().encode(newValue).length,
            lastAccessed: "Just now",
          };
        }
        return k;
      })
    );

    // Call REST endpoint
    fetch(`http://localhost:8080/api/keys/${encodeURIComponent(keyName)}`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        value: newValue,
        ttl_seconds: newTtl,
      }),
    }).catch(() => {});
  };

  const handleDeleteKey = (keyName: string) => {
    setKeys((prev) => prev.filter((k) => k.key !== keyName));

    // Call REST endpoint
    fetch(`http://localhost:8080/api/keys/${encodeURIComponent(keyName)}`, {
      method: "DELETE",
    }).catch(() => {});
  };

  return (
    <div className="space-y-6 animate-in fade-in duration-300">
      {/* Top Banner & Quick Controls */}
      <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4 pb-2 border-b border-white/[0.06]">
        <div>
          <div className="flex items-center gap-2.5">
            <h1 className="text-2xl font-black tracking-tight text-white font-mono flex items-center gap-2">
              DATA EXPLORER
            </h1>
            <span className="px-2.5 py-0.5 text-[10px] font-mono font-bold uppercase rounded bg-cyan-950/80 text-cyan-300 border border-cyan-800/60 shadow-[0_0_10px_rgba(0,242,254,0.3)]">
              EXCEL-GRADE TABLE
            </span>
          </div>
          <p className="text-xs text-zinc-400 font-mono mt-1">
            Zero-copy table explorer with real-time JSON drawer, type filters, and cluster slot inspection.
          </p>
        </div>

        <button
          onClick={() => setIsModalOpen(true)}
          className="flex items-center gap-2 px-4 py-2.5 rounded-xl bg-cyan-400 text-black font-black font-mono text-xs hover:bg-cyan-300 transition-all shadow-[0_0_20px_-3px_rgba(0,242,254,0.4)] active:scale-95"
        >
          <Plus className="w-4 h-4" />
          <span>ADD NEW KEY</span>
        </button>
      </div>

      {/* Quick Stats Ribbon */}
      <div className="grid grid-cols-2 sm:grid-cols-4 gap-4 font-mono text-xs">
        <div className="p-3.5 rounded-xl glass-panel border border-white/[0.06]">
          <span className="text-[10px] text-zinc-400 uppercase block">Keys in Memory</span>
          <span className="text-lg font-black text-white">{keys.length.toLocaleString()}</span>
          <span className="text-[10px] text-cyan-400 ml-1">Cached in RAM</span>
        </div>

        <div className="p-3.5 rounded-xl glass-panel border border-white/[0.06]">
          <span className="text-[10px] text-zinc-400 uppercase block">Expiring Keys</span>
          <span className="text-lg font-black text-amber-400">
            {keys.filter((k) => k.ttlSeconds !== null).length}
          </span>
          <span className="text-[10px] text-zinc-400 ml-1">with TTL</span>
        </div>

        <div className="p-3.5 rounded-xl glass-panel border border-white/[0.06]">
          <span className="text-[10px] text-zinc-400 uppercase block">Memory Consumed</span>
          <span className="text-lg font-black text-emerald-400">
            {(keys.reduce((acc, k) => acc + k.sizeBytes, 0) / 1024).toFixed(1)} KB
          </span>
          <span className="text-[10px] text-zinc-400 ml-1">Payload raw</span>
        </div>

        <div className="p-3.5 rounded-xl glass-panel border border-white/[0.06]">
          <span className="text-[10px] text-zinc-400 uppercase block">Cluster Distribution</span>
          <span className="text-lg font-black text-purple-400">3 Nodes</span>
          <span className="text-[10px] text-zinc-400 ml-1">16,384 Slots</span>
        </div>
      </div>

      {/* Search & Filter Toolbar */}
      <div className="p-4 rounded-2xl glass-panel border border-white/[0.08] flex flex-col md:flex-row items-stretch md:items-center justify-between gap-4 font-mono text-xs">
        {/* Search input with focus glow */}
        <div className="relative flex-1">
          <Search className="w-4 h-4 absolute left-3.5 top-1/2 -translate-y-1/2 text-zinc-400" />
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Search by key pattern (e.g. {tenant:101}*, session:*, slot number)..."
            className="w-full pl-10 pr-4 py-2.5 bg-black/50 border border-white/[0.08] rounded-xl text-white placeholder:text-zinc-500 focus:outline-none focus:border-cyan-400 focus:ring-1 focus:ring-cyan-400/50 transition-all text-xs"
          />
        </div>

        {/* Dropdown Filters */}
        <div className="flex items-center gap-3">
          {/* Type filter */}
          <div className="flex items-center gap-1.5 bg-black/40 border border-white/[0.08] rounded-xl px-2.5 py-1.5">
            <span className="text-zinc-500 text-[11px] uppercase">Type:</span>
            <select
              value={selectedType}
              onChange={(e) => setSelectedType(e.target.value as KeyType | "all")}
              className="bg-transparent text-white focus:outline-none cursor-pointer text-xs"
            >
              <option value="all" className="bg-[#0d121c]">ALL TYPES</option>
              <option value="string" className="bg-[#0d121c]">STRING</option>
              <option value="hash" className="bg-[#0d121c]">HASH</option>
              <option value="list" className="bg-[#0d121c]">LIST</option>
              <option value="zset" className="bg-[#0d121c]">ZSET</option>
              <option value="stream" className="bg-[#0d121c]">STREAM</option>
              <option value="bitmap" className="bg-[#0d121c]">BITMAP</option>
            </select>
          </div>

          {/* Node filter */}
          <div className="flex items-center gap-1.5 bg-black/40 border border-white/[0.08] rounded-xl px-2.5 py-1.5">
            <span className="text-zinc-500 text-[11px] uppercase">Node:</span>
            <select
              value={selectedNode}
              onChange={(e) => setSelectedNode(e.target.value)}
              className="bg-transparent text-white focus:outline-none cursor-pointer text-xs"
            >
              <option value="all" className="bg-[#0d121c]">ALL NODES</option>
              <option value="6379" className="bg-[#0d121c]">NODE 1 (:6379)</option>
              <option value="6380" className="bg-[#0d121c]">NODE 2 (:6380)</option>
              <option value="6381" className="bg-[#0d121c]">NODE 3 (:6381)</option>
            </select>
          </div>
        </div>
      </div>

      {/* Advanced Excel-like Key Table */}
      <KeyTable
        keys={filteredKeys}
        onSelectKey={(k) => {
          setActiveDrawerKey(k);
          setIsDrawerOpen(true);
        }}
        onDeleteKey={handleDeleteKey}
        selectedKeyName={activeDrawerKey?.key}
      />

      {/* Slide-over Drawer for JSON & Value Editing */}
      <KeyDrawer
        keyRecord={activeDrawerKey}
        isOpen={isDrawerOpen}
        onClose={() => setIsDrawerOpen(false)}
        onUpdateValue={handleUpdateValue}
        onDeleteKey={handleDeleteKey}
      />

      {/* Add New Key Modal */}
      <NewKeyModal
        isOpen={isModalOpen}
        onClose={() => setIsModalOpen(false)}
        onCreateKey={handleCreateKey}
      />
    </div>
  );
}
