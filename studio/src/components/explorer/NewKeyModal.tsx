"use client";

import React, { useState } from "react";
import {
  X,
  Plus,
  Zap,
  Clock,
  Layers,
  Sparkles,
  AlertCircle,
} from "lucide-react";
import { KeyType } from "@/types";

interface NewKeyModalProps {
  isOpen: boolean;
  onClose: () => void;
  onCreateKey: (newKey: {
    key: string;
    type: KeyType;
    ttlSeconds: number | null;
    value: string;
  }) => void;
}

export function NewKeyModal({ isOpen, onClose, onCreateKey }: NewKeyModalProps) {
  const [keyName, setKeyName] = useState("");
  const [keyType, setKeyType] = useState<KeyType>("string");
  const [ttlOption, setTtlOption] = useState<"persistent" | "custom">("persistent");
  const [customTtl, setCustomTtl] = useState("3600");
  const [keyValue, setKeyValue] = useState(`{\n  "status": "active",\n  "tier": "enterprise"\n}`);
  const [error, setError] = useState<string | null>(null);

  if (!isOpen) return null;

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!keyName.trim()) {
      setError("Key name cannot be empty.");
      return;
    }

    const ttl = ttlOption === "custom" ? parseInt(customTtl, 10) || null : null;
    onCreateKey({
      key: keyName.trim(),
      type: keyType,
      ttlSeconds: ttl,
      value: keyValue,
    });

    // Reset and close
    setKeyName("");
    setKeyValue(`{\n  "status": "active",\n  "tier": "enterprise"\n}`);
    setError(null);
    onClose();
  };

  const handleApplyHashTag = (tag: string) => {
    setKeyName(`{${tag}}${keyName.replace(/^\{.*?\}/, "") || "key"}`);
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/75 backdrop-blur-md animate-in fade-in duration-200">
      <div className="relative w-full max-w-xl rounded-2xl bg-[#0d121c] border border-cyan-500/30 shadow-[0_0_50px_-10px_rgba(0,242,254,0.25)] p-6 font-mono text-xs overflow-hidden">
        {/* Modal Header */}
        <div className="flex items-center justify-between pb-4 border-b border-white/[0.08]">
          <div className="flex items-center gap-2.5">
            <div className="p-2 rounded-xl bg-cyan-500/10 border border-cyan-500/20 text-cyan-400">
              <Plus className="w-4 h-4" />
            </div>
            <div>
              <h2 className="text-sm font-black text-white uppercase tracking-tight flex items-center gap-2">
                <span>INJECT NEW DATABASE KEY</span>
              </h2>
              <p className="text-[11px] text-zinc-400 font-normal">
                Direct write into VelocityDB local cache across 16,384 hash slots.
              </p>
            </div>
          </div>

          <button
            onClick={onClose}
            className="p-1.5 rounded-lg text-zinc-400 hover:text-white hover:bg-white/[0.06] transition-colors"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {error && (
          <div className="mt-4 p-3 rounded-xl bg-red-950/40 border border-red-500/40 text-red-300 text-xs flex items-center gap-2">
            <AlertCircle className="w-4 h-4 shrink-0" />
            <span>{error}</span>
          </div>
        )}

        <form onSubmit={handleSubmit} className="mt-4 space-y-4">
          {/* Key Name Input */}
          <div>
            <div className="flex items-center justify-between mb-1.5">
              <label className="text-[11px] font-semibold uppercase text-zinc-300">
                Key Name
              </label>
              <div className="flex items-center gap-1.5 text-[10px] text-zinc-400">
                <span>Cluster Hash Tagging:</span>
                {["tenant:101", "user:42", "orders"].map((tag) => (
                  <button
                    type="button"
                    key={tag}
                    onClick={() => handleApplyHashTag(tag)}
                    className="text-cyan-400 hover:text-cyan-300 hover:underline"
                  >
                    {`{${tag}}`}
                  </button>
                ))}
              </div>
            </div>

            <input
              type="text"
              required
              value={keyName}
              onChange={(e) => {
                setKeyName(e.target.value);
                if (error) setError(null);
              }}
              placeholder="{tenant:101}session:user_token:9941"
              className="w-full px-3.5 py-2.5 bg-black/60 border border-white/[0.08] rounded-xl text-white placeholder:text-zinc-600 focus:outline-none focus:border-cyan-400 focus:ring-1 focus:ring-cyan-400/50 transition-all font-mono text-xs"
            />
          </div>

          {/* Key Type Selector */}
          <div>
            <label className="text-[11px] font-semibold uppercase text-zinc-300 block mb-1.5">
              Data Structure Type
            </label>
            <div className="grid grid-cols-3 sm:grid-cols-6 gap-2">
              {(["string", "hash", "list", "zset", "stream", "bitmap"] as const).map((t) => (
                <button
                  type="button"
                  key={t}
                  onClick={() => setKeyType(t)}
                  className={`py-2 px-1 rounded-xl uppercase font-bold text-[10px] border transition-all text-center ${
                    keyType === t
                      ? "bg-cyan-500/20 text-cyan-300 border-cyan-400 shadow-[0_0_10px_rgba(0,242,254,0.2)]"
                      : "bg-black/30 border-white/[0.06] text-zinc-400 hover:text-zinc-200 hover:bg-white/[0.04]"
                  }`}
                >
                  {t}
                </button>
              ))}
            </div>
          </div>

          {/* TTL Settings */}
          <div>
            <label className="text-[11px] font-semibold uppercase text-zinc-300 block mb-1.5">
              Expiration (Time-To-Live)
            </label>
            <div className="flex items-center gap-4">
              <label className="flex items-center gap-2 cursor-pointer text-zinc-300">
                <input
                  type="radio"
                  name="ttl"
                  checked={ttlOption === "persistent"}
                  onChange={() => setTtlOption("persistent")}
                  className="accent-cyan-400"
                />
                <span>Persistent (No TTL)</span>
              </label>

              <label className="flex items-center gap-2 cursor-pointer text-zinc-300">
                <input
                  type="radio"
                  name="ttl"
                  checked={ttlOption === "custom"}
                  onChange={() => setTtlOption("custom")}
                  className="accent-cyan-400"
                />
                <span>Custom Seconds:</span>
              </label>

              {ttlOption === "custom" && (
                <input
                  type="number"
                  min="1"
                  value={customTtl}
                  onChange={(e) => setCustomTtl(e.target.value)}
                  className="w-24 px-2.5 py-1 bg-black/60 border border-white/[0.08] rounded-lg text-white focus:outline-none focus:border-cyan-400 text-xs"
                />
              )}
            </div>
          </div>

          {/* Initial Value Payload */}
          <div>
            <div className="flex items-center justify-between mb-1.5">
              <label className="text-[11px] font-semibold uppercase text-zinc-300">
                Payload / Initial Value (JSON / UTF-8)
              </label>
              <span className="text-[10px] text-zinc-500 font-normal">Binary Safe</span>
            </div>

            <textarea
              rows={5}
              value={keyValue}
              onChange={(e) => setKeyValue(e.target.value)}
              className="w-full p-3 bg-black/70 border border-white/[0.08] rounded-xl text-zinc-200 placeholder:text-zinc-600 focus:outline-none focus:border-cyan-400 focus:ring-1 focus:ring-cyan-400/50 transition-all font-mono text-xs leading-relaxed"
            />
          </div>

          {/* Footer Buttons */}
          <div className="flex items-center justify-end gap-3 pt-4 border-t border-white/[0.08]">
            <button
              type="button"
              onClick={onClose}
              className="px-4 py-2 rounded-xl text-zinc-400 hover:text-white hover:bg-white/[0.06] transition-colors"
            >
              Cancel
            </button>

            <button
              type="submit"
              className="flex items-center gap-2 px-5 py-2.5 rounded-xl bg-cyan-400 text-black font-black uppercase text-xs hover:bg-cyan-300 transition-all shadow-[0_0_20px_-3px_rgba(0,242,254,0.5)] active:scale-95"
            >
              <Zap className="w-3.5 h-3.5 fill-current" />
              <span>COMMIT TO VELOCITYDB</span>
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
