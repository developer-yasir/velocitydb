"use client";

import React, { useState, useEffect, useCallback, useMemo } from "react";
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
  AlertCircle,
  Inbox,
} from "lucide-react";
import { KeyRecord, KeyType, IVelocityKey, IApiError } from "@/types";
import { KeyTable } from "@/components/explorer/KeyTable";
import { KeyDrawer } from "@/components/explorer/KeyDrawer";
import { NewKeyModal } from "@/components/explorer/NewKeyModal";
import { useToast } from "@/components/ui/Toast";
import { API_BASE_URL as API_ROOT } from "@/lib/api-config";

const API_BASE_URL = `${API_ROOT}/api/keys`;

function mapVelocityKeyToRecord(dto: IVelocityKey): KeyRecord {
  return {
    key: dto.key,
    type: (dto.key_type.toLowerCase() || "string") as KeyType,
    slot: dto.slot,
    targetNode: dto.target_node,
    ttlSeconds: dto.ttl_seconds,
    sizeBytes: dto.size_bytes,
    value: dto.value,
    lastAccessed: dto.last_accessed || "Just now",
  };
}

export default function ExplorerPage() {
  const [keys, setKeys] = useState<KeyRecord[]>([]);
  const [isLoading, setIsLoading] = useState<boolean>(true);
  const [isRefreshing, setIsRefreshing] = useState<boolean>(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [selectedType, setSelectedType] = useState<KeyType | "all">("all");
  const [selectedNode, setSelectedNode] = useState<string>("all");
  const [activeDrawerKey, setActiveDrawerKey] = useState<KeyRecord | null>(null);
  const [isDrawerOpen, setIsDrawerOpen] = useState(false);
  const [isModalOpen, setIsModalOpen] = useState(false);

  const { showError, showSuccess } = useToast();

  // Pull live keys from the Rust Axum REST API (GET /api/keys)
  const fetchKeys = useCallback(
    async (showFeedback = false) => {
      if (showFeedback) setIsRefreshing(true);
      try {
        const res = await fetch(API_BASE_URL, {
          headers: { Accept: "application/json" },
        });

        if (!res.ok) {
          const err: IApiError = {
            status: res.status,
            message: `Engine responded with status ${res.statusText}`,
          };
          showError(err, "Failed to Load Keys");
          return;
        }

        const data: IVelocityKey[] = await res.json();
        const mapped = data.map(mapVelocityKeyToRecord);
        setKeys(mapped);

        if (showFeedback) {
          showSuccess(
            "Keys Synchronized",
            `Loaded ${mapped.length} records from VelocityDB engine.`
          );
        }
      } catch (err: unknown) {
        const msg =
          err instanceof Error
            ? err.message
            : "Cannot establish connection to VelocityDB REST plane (port 8080)";
        showError(
          {
            status: 503,
            message: msg,
          },
          "Backend Unreachable"
        );
      } finally {
        setIsLoading(false);
        setIsRefreshing(false);
      }
    },
    [showError, showSuccess]
  );

  // Initial fetch on mount
  useEffect(() => {
    fetchKeys(false);
  }, [fetchKeys]);

  // Filter keys based on search and dropdown filters
  const filteredKeys = useMemo(() => {
    return keys.filter((k) => {
      const matchesSearch =
        k.key.toLowerCase().includes(searchQuery.toLowerCase()) ||
        String(k.slot).includes(searchQuery);

      const matchesType = selectedType === "all" || k.type === selectedType;
      const matchesNode =
        selectedNode === "all" || k.targetNode.includes(selectedNode);

      return matchesSearch && matchesType && matchesNode;
    });
  }, [keys, searchQuery, selectedType, selectedNode]);

  // CREATE: POST /api/keys
  const handleCreateKey = async (newKey: {
    key: string;
    type: KeyType;
    ttlSeconds: number | null;
    value: string;
  }) => {
    try {
      const res = await fetch(API_BASE_URL, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          key: newKey.key,
          key_type: newKey.type,
          ttl_seconds: newKey.ttlSeconds,
          value: newKey.value,
        }),
      });

      if (!res.ok) {
        const err: IApiError = {
          status: res.status,
          message: `Engine failed to allocate slot for "${newKey.key}". Status: ${res.statusText}`,
        };
        showError(err, "Key Creation Rejected");
        return;
      }

      const created: IVelocityKey = await res.json();
      const createdRecord = mapVelocityKeyToRecord(created);

      // Prepend to keys table
      setKeys((prev) => [
        createdRecord,
        ...prev.filter((k) => k.key !== createdRecord.key),
      ]);
      setActiveDrawerKey(createdRecord);
      setIsDrawerOpen(true);

      showSuccess(
        "Key Created",
        `Assigned to Slot ${created.slot} on ${created.target_node} (${created.size_bytes} bytes).`
      );
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : "Network error contacting port 8080";
      showError({ status: 503, message: msg }, "Backend Unreachable");
    }
  };

  // EDIT/SAVE: PUT /api/keys/:key
  const handleUpdateValue = async (
    keyName: string,
    newValue: string,
    newTtl: number | null
  ): Promise<boolean> => {
    const previousKeys = [...keys];

    // Optimistically update table row
    setKeys((prev) =>
      prev.map((k) => {
        if (k.key === keyName) {
          return {
            ...k,
            ttlSeconds: newTtl,
            sizeBytes: new TextEncoder().encode(newValue).length,
            lastAccessed: "Just now",
            value: newValue,
          };
        }
        return k;
      })
    );

    try {
      const res = await fetch(`${API_BASE_URL}/${encodeURIComponent(keyName)}`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          value: newValue,
          ttl_seconds: newTtl,
        }),
      });

      if (!res.ok) {
        if (res.status === 404) {
          const err: IApiError = {
            status: 404,
            message: `Key "${keyName}" not found on node. It may have expired or been evicted.`,
          };
          showError(err, "Key Not Found");
        } else {
          const err: IApiError = {
            status: res.status,
            message: `Failed to commit updates for "${keyName}". Status: ${res.statusText}`,
          };
          showError(err, "Update Rejected");
        }
        // Rollback optimistic changes
        setKeys(previousKeys);
        return false;
      }

      const updated: IVelocityKey = await res.json();
      const updatedRecord = mapVelocityKeyToRecord(updated);

      // Harmonize with server response
      setKeys((prev) =>
        prev.map((k) => (k.key === updated.key ? updatedRecord : k))
      );
      if (activeDrawerKey?.key === updated.key) {
        setActiveDrawerKey(updatedRecord);
      }

      showSuccess(
        "Key Updated",
        `Committed ${updated.size_bytes} bytes to memory on ${updated.target_node}.`
      );
      return true;
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : "Network error contacting port 8080";
      showError({ status: 503, message: msg }, "Backend Unreachable");
      setKeys(previousKeys);
      return false;
    }
  };

  // DELETE: DELETE /api/keys/:key (Optimistic)
  const handleDeleteKey = async (keyName: string): Promise<boolean> => {
    const previousKeys = [...keys];

    // Optimistically remove from state immediately
    setKeys((prev) => prev.filter((k) => k.key !== keyName));
    if (activeDrawerKey?.key === keyName) {
      setIsDrawerOpen(false);
      setActiveDrawerKey(null);
    }

    try {
      const res = await fetch(`${API_BASE_URL}/${encodeURIComponent(keyName)}`, {
        method: "DELETE",
      });

      if (!res.ok) {
        if (res.status === 404) {
          const err: IApiError = {
            status: 404,
            message: `Key "${keyName}" not found on node. It may have already expired or been evicted.`,
          };
          showError(err, "Eviction Warning");
        } else {
          const err: IApiError = {
            status: res.status,
            message: `Failed to evict "${keyName}". Status: ${res.statusText}`,
          };
          showError(err, "Eviction Failed");
        }
        // Rollback optimistic deletion
        setKeys(previousKeys);
        return false;
      }

      showSuccess("Key Evicted", `Key "${keyName}" removed from database RAM.`);
      return true;
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : "Network error contacting port 8080";
      showError({ status: 503, message: msg }, "Backend Unreachable");
      setKeys(previousKeys);
      return false;
    }
  };

  // Stats calculation
  const totalMemoryBytes = useMemo(
    () => keys.reduce((acc, k) => acc + k.sizeBytes, 0),
    [keys]
  );
  const expiringCount = useMemo(
    () => keys.filter((k) => k.ttlSeconds !== null).length,
    [keys]
  );

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
              LIVE REST PLANE
            </span>
          </div>
          <p className="text-xs text-zinc-400 font-mono mt-1">
            Real-time key inspector communicating directly with VelocityDB management endpoints on port 8080.
          </p>
        </div>

        <div className="flex items-center gap-2.5">
          {/* Refresh from Backend button */}
          <button
            onClick={() => fetchKeys(true)}
            disabled={isRefreshing}
            className="flex items-center gap-2 px-3.5 py-2.5 rounded-xl bg-white/[0.05] border border-white/[0.08] text-xs font-mono font-bold text-zinc-300 hover:text-white hover:bg-white/[0.1] hover:border-cyan-500/40 transition-all active:scale-95 disabled:opacity-50"
            title="Reload keys from Rust engine"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${isRefreshing ? "animate-spin text-cyan-400" : ""}`} />
            <span>{isRefreshing ? "SYNCING..." : "SYNC"}</span>
          </button>

          {/* Add Key Button */}
          <button
            onClick={() => setIsModalOpen(true)}
            className="flex items-center gap-2 px-4 py-2.5 rounded-xl bg-cyan-400 text-black font-black font-mono text-xs hover:bg-cyan-300 transition-all shadow-[0_0_20px_-3px_rgba(0,242,254,0.4)] active:scale-95"
          >
            <Plus className="w-4 h-4" />
            <span>ADD NEW KEY</span>
          </button>
        </div>
      </div>

      {/* Quick Stats Ribbon */}
      <div className="grid grid-cols-2 sm:grid-cols-4 gap-4 font-mono text-xs">
        <div className="p-3.5 rounded-xl glass-panel border border-white/[0.06]">
          <span className="text-[10px] text-zinc-400 uppercase block">Keys in Memory</span>
          <span className="text-lg font-black text-white">
            {isLoading ? "--" : keys.length.toLocaleString()}
          </span>
          <span className="text-[10px] text-cyan-400 ml-1">Live Engine State</span>
        </div>

        <div className="p-3.5 rounded-xl glass-panel border border-white/[0.06]">
          <span className="text-[10px] text-zinc-400 uppercase block">Expiring Keys</span>
          <span className="text-lg font-black text-amber-400">
            {isLoading ? "--" : expiringCount}
          </span>
          <span className="text-[10px] text-zinc-400 ml-1">Active TTL</span>
        </div>

        <div className="p-3.5 rounded-xl glass-panel border border-white/[0.06]">
          <span className="text-[10px] text-zinc-400 uppercase block">Memory Consumed</span>
          <span className="text-lg font-black text-emerald-400">
            {isLoading ? "--" : (totalMemoryBytes / 1024).toFixed(1)} KB
          </span>
          <span className="text-[10px] text-zinc-400 ml-1">Payload Size</span>
        </div>

        <div className="p-3.5 rounded-xl glass-panel border border-white/[0.06]">
          <span className="text-[10px] text-zinc-400 uppercase block">Cluster Topology</span>
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

      {/* Loading Skeleton / Empty State / Key Table */}
      {isLoading ? (
        <div className="glass-panel p-16 rounded-2xl flex flex-col items-center justify-center space-y-3 font-mono text-xs">
          <RefreshCw className="w-6 h-6 animate-spin text-cyan-400" />
          <span className="text-zinc-300 font-bold">Querying VelocityDB Key Space...</span>
          <span className="text-zinc-500 text-[11px]">Connecting to http://localhost:8080/api/keys</span>
        </div>
      ) : keys.length === 0 ? (
        <div className="glass-panel p-16 rounded-2xl flex flex-col items-center justify-center space-y-4 font-mono text-center">
          <div className="p-3 rounded-2xl bg-cyan-500/10 border border-cyan-500/20 text-cyan-400">
            <Inbox className="w-8 h-8" />
          </div>
          <div>
            <h3 className="text-base font-bold text-white">No Keys in Database</h3>
            <p className="text-xs text-zinc-400 mt-1 max-w-sm">
              The database key space is currently empty. Use the in-browser terminal or click &quot;ADD NEW KEY&quot; to write your first key.
            </p>
          </div>
          <button
            onClick={() => setIsModalOpen(true)}
            className="flex items-center gap-2 px-4 py-2 rounded-xl bg-cyan-400 text-black font-bold text-xs hover:bg-cyan-300 transition-colors"
          >
            <Plus className="w-3.5 h-3.5" />
            <span>Create First Key</span>
          </button>
        </div>
      ) : (
        <KeyTable
          keys={filteredKeys}
          onSelectKey={(k) => {
            setActiveDrawerKey(k);
            setIsDrawerOpen(true);
          }}
          onDeleteKey={handleDeleteKey}
          selectedKeyName={activeDrawerKey?.key}
        />
      )}

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