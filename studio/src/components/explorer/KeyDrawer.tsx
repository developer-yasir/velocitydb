"use client";

import React, { useState, useEffect } from "react";
import {
  X,
  Copy,
  Check,
  Save,
  Trash2,
  Clock,
  HardDrive,
  Network,
  Zap,
  Code2,
  FileCode,
  Sparkles,
  AlertTriangle,
  RefreshCw,
} from "lucide-react";
import { KeyRecord, IVelocityKey } from "@/types";
import { API_BASE_URL } from "@/lib/api-config";

interface KeyDrawerProps {
  keyRecord: KeyRecord | null;
  isOpen: boolean;
  onClose: () => void;
  onUpdateValue: (key: string, newValue: string, newTtl: number | null) => Promise<boolean> | void;
  onDeleteKey: (key: string) => Promise<boolean> | void;
}

export function KeyDrawer({
  keyRecord,
  isOpen,
  onClose,
  onUpdateValue,
  onDeleteKey,
}: KeyDrawerProps) {
  const [value, setValue] = useState("");
  const [ttlInput, setTtlInput] = useState<string>("-1");
  const [copiedKey, setCopiedKey] = useState(false);
  const [copiedVal, setCopiedVal] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [isSaved, setIsSaved] = useState(false);
  const [isLoadingValue, setIsLoadingValue] = useState(false);
  const [activeTab, setActiveTab] = useState<"editor" | "metadata">("editor");

  // Load live fresh value from the Rust Axum REST API (GET /api/keys/:key)
  useEffect(() => {
    if (!keyRecord) return;

    setTtlInput(keyRecord.ttlSeconds !== null ? String(keyRecord.ttlSeconds) : "-1");

    // Initialize with cached value if present
    if (keyRecord.value !== undefined) {
      tryFormatInitial(keyRecord.value);
    } else {
      setValue("");
    }

    let isCancelled = false;
    setIsLoadingValue(true);

    fetch(`${API_BASE_URL}/api/keys/${encodeURIComponent(keyRecord.key)}`)
      .then((res) => {
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        return res.json();
      })
      .then((data: IVelocityKey) => {
        if (!isCancelled) {
          tryFormatInitial(data.value);
          setTtlInput(data.ttl_seconds !== null ? String(data.ttl_seconds) : "-1");
          setIsLoadingValue(false);
        }
      })
      .catch(() => {
        if (!isCancelled) {
          setIsLoadingValue(false);
        }
      });

    return () => {
      isCancelled = true;
    };
  }, [keyRecord]);

  function tryFormatInitial(raw: string) {
    try {
      const parsed = JSON.parse(raw);
      setValue(JSON.stringify(parsed, null, 2));
    } catch {
      setValue(raw);
    }
  }

  if (!isOpen || !keyRecord) return null;

  const lineCount = value.split("\n").length;
  const lines = Array.from({ length: Math.max(1, lineCount) }, (_, i) => i + 1);

  const handleCopyKey = () => {
    navigator.clipboard.writeText(keyRecord.key);
    setCopiedKey(true);
    setTimeout(() => setCopiedKey(false), 1500);
  };

  const handleCopyVal = () => {
    navigator.clipboard.writeText(value);
    setCopiedVal(true);
    setTimeout(() => setCopiedVal(false), 1500);
  };

  const handleFormatJson = () => {
    try {
      const parsed = JSON.parse(value);
      setValue(JSON.stringify(parsed, null, 2));
    } catch {
      // not json, leave as is
    }
  };

  const handleMinifyJson = () => {
    try {
      const parsed = JSON.parse(value);
      setValue(JSON.stringify(parsed));
    } catch {
      // not json, leave as is
    }
  };

  const handleSave = async () => {
    if (!keyRecord) return;
    setIsSaving(true);
    const ttl = parseInt(ttlInput, 10);
    const result = await onUpdateValue(keyRecord.key, value, isNaN(ttl) || ttl < 0 ? null : ttl);
    setIsSaving(false);
    if (result !== false) {
      setIsSaved(true);
      setTimeout(() => setIsSaved(false), 1500);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex justify-end bg-black/60 backdrop-blur-sm animate-in fade-in duration-200">
      {/* Sliding Drawer Body */}
      <div className="relative w-full max-w-2xl h-full bg-[#0a0d14] border-l border-white/[0.1] shadow-2xl flex flex-col font-mono text-xs animate-in slide-in-from-right duration-300">
        {/* Drawer Header */}
        <div className="flex items-start justify-between p-6 border-b border-white/[0.08] bg-[#0d121c]/90">
          <div className="space-y-1.5 flex-1 pr-4">
            <div className="flex items-center gap-2">
              <span className="text-[10px] font-bold px-2 py-0.5 rounded uppercase bg-cyan-950 text-cyan-400 border border-cyan-800">
                {keyRecord.type.toUpperCase()}
              </span>
              <span className="text-[10px] font-bold text-purple-400 bg-purple-950/80 px-2 py-0.5 rounded border border-purple-800/60">
                SLOT {keyRecord.slot}
              </span>
              <span className="text-[10px] text-zinc-400">
                Host: <strong className="text-zinc-200">{keyRecord.targetNode}</strong>
              </span>
            </div>

            <h2 className="text-sm font-bold text-white break-all flex items-center gap-2">
              <span>{keyRecord.key}</span>
              <button
                onClick={handleCopyKey}
                className="p-1 rounded text-zinc-400 hover:text-cyan-400 transition-colors"
                title="Copy Key"
              >
                {copiedKey ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : <Copy className="w-3.5 h-3.5" />}
              </button>
            </h2>
          </div>

          <button
            onClick={onClose}
            className="p-2 rounded-xl text-zinc-400 hover:text-white hover:bg-white/[0.06] transition-colors"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Navigation Tabs */}
        <div className="flex items-center px-6 border-b border-white/[0.06] bg-[#0c1018]">
          <button
            onClick={() => setActiveTab("editor")}
            className={`flex items-center gap-2 py-3 px-3 font-semibold text-xs border-b-2 transition-colors ${
              activeTab === "editor"
                ? "border-cyan-400 text-cyan-300"
                : "border-transparent text-zinc-400 hover:text-zinc-200"
            }`}
          >
            <Code2 className="w-4 h-4" />
            <span>VALUE (CODE EDITOR)</span>
          </button>

          <button
            onClick={() => setActiveTab("metadata")}
            className={`flex items-center gap-2 py-3 px-3 font-semibold text-xs border-b-2 transition-colors ${
              activeTab === "metadata"
                ? "border-cyan-400 text-cyan-300"
                : "border-transparent text-zinc-400 hover:text-zinc-200"
            }`}
          >
            <Network className="w-4 h-4" />
            <span>METADATA &amp; ROUTING</span>
          </button>
        </div>

        {/* Tab 1: Code Editor Body */}
        {activeTab === "editor" && (
          <div className="flex-1 flex flex-col p-6 space-y-4 overflow-hidden">
            {/* Action Bar Above Editor */}
            <div className="flex items-center justify-between text-xs">
              <div className="flex items-center gap-2">
                <button
                  onClick={handleFormatJson}
                  className="px-2.5 py-1 rounded-lg bg-white/[0.05] border border-white/[0.08] text-zinc-300 hover:text-white hover:bg-white/[0.1] transition-colors"
                >
                  Prettify JSON
                </button>
                <button
                  onClick={handleMinifyJson}
                  className="px-2.5 py-1 rounded-lg bg-white/[0.05] border border-white/[0.08] text-zinc-300 hover:text-white hover:bg-white/[0.1] transition-colors"
                >
                  Minify
                </button>
                <button
                  onClick={handleCopyVal}
                  className="px-2.5 py-1 rounded-lg bg-white/[0.05] border border-white/[0.08] text-zinc-300 hover:text-cyan-400 transition-colors flex items-center gap-1.5"
                >
                  {copiedVal ? <Check className="w-3 h-3 text-emerald-400" /> : <Copy className="w-3 h-3" />}
                  <span>{copiedVal ? "Copied!" : "Copy Value"}</span>
                </button>
              </div>

              <div className="text-[11px] text-zinc-500">
                <span>{new TextEncoder().encode(value).length} Bytes</span>
              </div>
            </div>

            {/* Code Editor Container with Gutter Line Numbers */}
            <div className="flex-1 rounded-xl bg-[#06080e] border border-white/[0.08] overflow-hidden flex relative shadow-inner">
              {/* Left Line Number Gutter */}
              <div className="w-10 py-3 select-none text-right pr-2.5 border-r border-white/[0.05] bg-[#080b12] text-zinc-600 font-mono text-[11px] leading-relaxed overflow-hidden">
                {lines.map((l) => (
                  <div key={l}>{l}</div>
                ))}
              </div>

              {/* Text Area Code Body */}
              <textarea
                value={value}
                onChange={(e) => setValue(e.target.value)}
                spellCheck={false}
                className="flex-1 p-3 bg-transparent text-zinc-200 font-mono text-xs leading-relaxed focus:outline-none resize-none selection:bg-cyan-500/30 selection:text-cyan-200"
              />
            </div>

            {/* TTL Modifier Bar */}
            <div className="p-3.5 rounded-xl bg-[#0d121c] border border-white/[0.06] flex items-center justify-between">
              <div className="flex items-center gap-2 text-zinc-300">
                <Clock className="w-4 h-4 text-amber-400" />
                <span className="font-semibold text-xs">TTL (Seconds):</span>
              </div>

              <div className="flex items-center gap-2">
                <input
                  type="number"
                  value={ttlInput}
                  onChange={(e) => setTtlInput(e.target.value)}
                  placeholder="-1 (No expiration)"
                  className="w-32 px-3 py-1 bg-black/60 border border-white/[0.08] rounded-lg text-white font-mono text-xs focus:outline-none focus:border-cyan-400"
                />
                <span className="text-[10px] text-zinc-500 font-normal">(-1 = Persist)</span>
              </div>
            </div>
          </div>
        )}

        {/* Tab 2: Metadata & Hash Slot Routing */}
        {activeTab === "metadata" && (
          <div className="flex-1 p-6 space-y-6 overflow-y-auto">
            <div className="grid grid-cols-2 gap-4">
              <div className="p-4 rounded-xl bg-[#0d121c] border border-white/[0.06] space-y-1">
                <span className="text-[10px] text-zinc-400 uppercase">Consistent Hash Slot</span>
                <span className="text-xl font-black text-purple-400 block">{keyRecord.slot}</span>
                <span className="text-[10px] text-zinc-500">Algorithm: CRC16(key) mod 16384</span>
              </div>

              <div className="p-4 rounded-xl bg-[#0d121c] border border-white/[0.06] space-y-1">
                <span className="text-[10px] text-zinc-400 uppercase">Local Memory Overhead</span>
                <span className="text-xl font-black text-emerald-400 block">{keyRecord.sizeBytes} B</span>
                <span className="text-[10px] text-zinc-500">24B Intrusive Doubly-Linked Node</span>
              </div>
            </div>

            <div className="p-4 rounded-xl bg-[#0d121c] border border-white/[0.06] space-y-3">
              <span className="text-[11px] font-bold uppercase text-zinc-300 block">
                Cluster Routing &amp; Redirection Engine
              </span>

              <div className="space-y-2 text-xs text-zinc-400">
                <div className="flex justify-between">
                  <span>Owning Node:</span>
                  <span className="text-cyan-400 font-bold">{keyRecord.targetNode}</span>
                </div>
                <div className="flex justify-between">
                  <span>Routing Strategy:</span>
                  <span className="text-zinc-200">Lock-Free ThreadLocal Topology Cache</span>
                </div>
                <div className="flex justify-between">
                  <span>Non-blocking Redirect:</span>
                  <span className="text-emerald-400 font-bold">-MOVED {keyRecord.slot} {keyRecord.targetNode}</span>
                </div>
              </div>
            </div>
          </div>
        )}

        {/* Drawer Footer Actions */}
        <div className="p-6 border-t border-white/[0.08] bg-[#0d121c] flex items-center justify-between">
          <button
            onClick={() => {
              if (confirm(`Are you sure you want to delete key "${keyRecord.key}" from VelocityDB?`)) {
                onDeleteKey(keyRecord.key);
                onClose();
              }
            }}
            className="flex items-center gap-2 px-3.5 py-2 rounded-xl bg-red-950/40 border border-red-500/40 text-red-400 text-xs font-bold hover:bg-red-950/70 transition-colors"
          >
            <Trash2 className="w-4 h-4" />
            <span>DELETE KEY</span>
          </button>

          <div className="flex items-center gap-3">
            <button
              onClick={onClose}
              className="px-4 py-2 rounded-xl text-zinc-400 hover:text-white hover:bg-white/[0.06] transition-colors"
            >
              Cancel
            </button>

            <button
              onClick={handleSave}
              disabled={isSaving}
              className="flex items-center gap-2 px-5 py-2.5 rounded-xl bg-cyan-400 text-black font-black uppercase text-xs hover:bg-cyan-300 transition-all shadow-[0_0_20px_-3px_rgba(0,242,254,0.5)] active:scale-95 disabled:opacity-50"
            >
              {isSaving ? (
                <RefreshCw className="w-4 h-4 animate-spin" />
              ) : isSaved ? (
                <Check className="w-4 h-4" />
              ) : (
                <Save className="w-4 h-4" />
              )}
              <span>{isSaving ? "SAVING..." : isSaved ? "COMMITTED" : "COMMIT UPDATE"}</span>
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
