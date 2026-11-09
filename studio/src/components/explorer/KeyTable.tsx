"use client";

import React, { useState } from "react";
import {
  ArrowUpDown,
  ArrowUp,
  ArrowDown,
  Clock,
  HardDrive,
  Copy,
  Check,
  Trash2,
  ExternalLink,
  ChevronLeft,
  ChevronRight,
} from "lucide-react";
import { KeyRecord, KeyType } from "@/types";

interface KeyTableProps {
  keys: KeyRecord[];
  onSelectKey: (key: KeyRecord) => void;
  onDeleteKey: (key: string) => void | Promise<boolean>;
  selectedKeyName?: string;
}

type SortField = "key" | "type" | "slot" | "ttlSeconds" | "sizeBytes";
type SortDirection = "asc" | "desc";

export function KeyTable({
  keys,
  onSelectKey,
  onDeleteKey,
  selectedKeyName,
}: KeyTableProps) {
  const [sortField, setSortField] = useState<SortField>("key");
  const [sortDirection, setSortDirection] = useState<SortDirection>("asc");
  const [selectedKeys, setSelectedKeys] = useState<Set<string>>(new Set());
  const [copiedKey, setCopiedKey] = useState<string | null>(null);
  const [currentPage, setCurrentPage] = useState(1);
  const [rowsPerPage, setRowsPerPage] = useState(10);

  const handleSort = (field: SortField) => {
    if (sortField === field) {
      setSortDirection(sortDirection === "asc" ? "desc" : "asc");
    } else {
      setSortField(field);
      setSortDirection("asc");
    }
  };

  const sortedKeys = [...keys].sort((a, b) => {
    let aVal = a[sortField];
    let bVal = b[sortField];

    if (aVal === null) aVal = -1;
    if (bVal === null) bVal = -1;

    if (aVal < bVal) return sortDirection === "asc" ? -1 : 1;
    if (aVal > bVal) return sortDirection === "asc" ? 1 : -1;
    return 0;
  });

  // Pagination slice
  const totalPages = Math.ceil(sortedKeys.length / rowsPerPage);
  const paginatedKeys = sortedKeys.slice(
    (currentPage - 1) * rowsPerPage,
    currentPage * rowsPerPage
  );

  const toggleSelectAll = () => {
    if (selectedKeys.size === paginatedKeys.length) {
      setSelectedKeys(new Set());
    } else {
      setSelectedKeys(new Set(paginatedKeys.map((k) => k.key)));
    }
  };

  const toggleSelectOne = (key: string, e: React.MouseEvent) => {
    e.stopPropagation();
    const next = new Set(selectedKeys);
    if (next.has(key)) {
      next.delete(key);
    } else {
      next.add(key);
    }
    setSelectedKeys(next);
  };

  const handleCopy = (key: string, e: React.MouseEvent) => {
    e.stopPropagation();
    navigator.clipboard.writeText(key);
    setCopiedKey(key);
    setTimeout(() => setCopiedKey(null), 1500);
  };

  const getTypeBadge = (type: KeyType) => {
    switch (type) {
      case "string":
        return "bg-cyan-950/80 text-cyan-300 border-cyan-800/60 shadow-[0_0_8px_rgba(0,242,254,0.15)]";
      case "hash":
        return "bg-purple-950/80 text-purple-300 border-purple-800/60";
      case "list":
        return "bg-amber-950/80 text-amber-300 border-amber-800/60";
      case "zset":
        return "bg-emerald-950/80 text-emerald-300 border-emerald-800/60";
      case "stream":
        return "bg-orange-950/80 text-orange-300 border-orange-800/60";
      case "bitmap":
        return "bg-blue-950/80 text-blue-300 border-blue-800/60";
      default:
        return "bg-zinc-800 text-zinc-300 border-zinc-700";
    }
  };

  return (
    <div className="glass-panel rounded-2xl overflow-hidden flex flex-col font-mono text-xs border border-white/[0.08] shadow-2xl">
      {/* Table Viewport */}
      <div className="overflow-x-auto">
        <table className="w-full text-left border-collapse">
          <thead>
            <tr className="border-b border-white/[0.08] bg-[#0c1018] text-zinc-400 text-[11px] uppercase tracking-wider select-none">
              {/* Checkbox column */}
              <th className="py-3.5 px-4 w-10">
                <input
                  type="checkbox"
                  checked={paginatedKeys.length > 0 && selectedKeys.size === paginatedKeys.length}
                  onChange={toggleSelectAll}
                  className="rounded border-zinc-700 bg-black/40 text-cyan-400 focus:ring-0 focus:ring-offset-0 cursor-pointer"
                />
              </th>

              {/* Key Name */}
              <th
                onClick={() => handleSort("key")}
                className="py-3.5 px-4 cursor-pointer hover:text-white transition-colors"
              >
                <div className="flex items-center gap-1.5">
                  <span>Database Key</span>
                  {sortField === "key" ? (
                    sortDirection === "asc" ? <ArrowUp className="w-3.5 h-3.5 text-cyan-400" /> : <ArrowDown className="w-3.5 h-3.5 text-cyan-400" />
                  ) : (
                    <ArrowUpDown className="w-3.5 h-3.5 opacity-40" />
                  )}
                </div>
              </th>

              {/* Data Type */}
              <th
                onClick={() => handleSort("type")}
                className="py-3.5 px-4 cursor-pointer hover:text-white transition-colors"
              >
                <div className="flex items-center gap-1.5">
                  <span>Data Type</span>
                  {sortField === "type" ? (
                    sortDirection === "asc" ? <ArrowUp className="w-3.5 h-3.5 text-cyan-400" /> : <ArrowDown className="w-3.5 h-3.5 text-cyan-400" />
                  ) : (
                    <ArrowUpDown className="w-3.5 h-3.5 opacity-40" />
                  )}
                </div>
              </th>

              {/* Hash Slot */}
              <th
                onClick={() => handleSort("slot")}
                className="py-3.5 px-4 cursor-pointer hover:text-white transition-colors"
              >
                <div className="flex items-center gap-1.5">
                  <span>Slot #</span>
                  {sortField === "slot" ? (
                    sortDirection === "asc" ? <ArrowUp className="w-3.5 h-3.5 text-purple-400" /> : <ArrowDown className="w-3.5 h-3.5 text-purple-400" />
                  ) : (
                    <ArrowUpDown className="w-3.5 h-3.5 opacity-40" />
                  )}
                </div>
              </th>

              {/* Target Node */}
              <th className="py-3.5 px-4">Cluster Node</th>

              {/* Time to Live (TTL) */}
              <th
                onClick={() => handleSort("ttlSeconds")}
                className="py-3.5 px-4 cursor-pointer hover:text-white transition-colors"
              >
                <div className="flex items-center gap-1.5">
                  <span>Time-To-Live (TTL)</span>
                  {sortField === "ttlSeconds" ? (
                    sortDirection === "asc" ? <ArrowUp className="w-3.5 h-3.5 text-amber-400" /> : <ArrowDown className="w-3.5 h-3.5 text-amber-400" />
                  ) : (
                    <ArrowUpDown className="w-3.5 h-3.5 opacity-40" />
                  )}
                </div>
              </th>

              {/* Size */}
              <th
                onClick={() => handleSort("sizeBytes")}
                className="py-3.5 px-4 cursor-pointer hover:text-white transition-colors"
              >
                <div className="flex items-center gap-1.5">
                  <span>Memory Size</span>
                  {sortField === "sizeBytes" ? (
                    sortDirection === "asc" ? <ArrowUp className="w-3.5 h-3.5 text-emerald-400" /> : <ArrowDown className="w-3.5 h-3.5 text-emerald-400" />
                  ) : (
                    <ArrowUpDown className="w-3.5 h-3.5 opacity-40" />
                  )}
                </div>
              </th>

              {/* Action Column */}
              <th className="py-3.5 px-4 text-right">Actions</th>
            </tr>
          </thead>

          <tbody className="divide-y divide-white/[0.04]">
            {paginatedKeys.map((k) => {
              const isSelectedRow = selectedKeyName === k.key;
              const isChecked = selectedKeys.has(k.key);

              return (
                <tr
                  key={k.key}
                  onClick={() => onSelectKey(k)}
                  className={`group cursor-pointer transition-colors duration-150 ${
                    isSelectedRow
                      ? "bg-cyan-500/10 border-l-2 border-l-cyan-400"
                      : isChecked
                      ? "bg-white/[0.04]"
                      : "hover:bg-white/[0.03]"
                  }`}
                >
                  {/* Checkbox */}
                  <td className="py-3 px-4" onClick={(e) => toggleSelectOne(k.key, e)}>
                    <input
                      type="checkbox"
                      checked={isChecked}
                      onChange={() => {}}
                      className="rounded border-zinc-700 bg-black/40 text-cyan-400 focus:ring-0 focus:ring-offset-0 cursor-pointer"
                    />
                  </td>

                  {/* Key Name with Hash Tag Highlights */}
                  <td className="py-3 px-4 font-semibold text-white group-hover:text-cyan-300 transition-colors">
                    <div className="flex items-center gap-2">
                      <span className="truncate max-w-xs">{k.key}</span>
                      <button
                        onClick={(e) => handleCopy(k.key, e)}
                        className="opacity-0 group-hover:opacity-100 p-1 rounded text-zinc-400 hover:text-cyan-400 transition-all"
                        title="Copy Key"
                      >
                        {copiedKey === k.key ? (
                          <Check className="w-3.5 h-3.5 text-emerald-400" />
                        ) : (
                          <Copy className="w-3.5 h-3.5" />
                        )}
                      </button>
                    </div>
                  </td>

                  {/* Type Badge */}
                  <td className="py-3 px-4">
                    <span
                      className={`px-2 py-0.5 rounded text-[10px] font-bold uppercase border ${getTypeBadge(
                        k.type
                      )}`}
                    >
                      {k.type}
                    </span>
                  </td>

                  {/* Hash Slot */}
                  <td className="py-3 px-4">
                    <span className="text-purple-400 font-bold bg-purple-950/60 px-2 py-0.5 rounded border border-purple-800/40">
                      {k.slot}
                    </span>
                  </td>

                  {/* Target Node */}
                  <td className="py-3 px-4 text-zinc-400">{k.targetNode}</td>

                  {/* TTL */}
                  <td className="py-3 px-4">
                    {typeof k.ttlSeconds === "number" && !isNaN(k.ttlSeconds) ? (
                      <span className="inline-flex items-center gap-1.5 text-amber-300">
                        <Clock className="w-3 h-3 text-amber-400" />
                        <span>{k.ttlSeconds.toLocaleString()}s</span>
                      </span>
                    ) : (
                      <span className="text-zinc-500 font-normal">Persistent</span>
                    )}
                  </td>

                  {/* Size */}
                  <td className="py-3 px-4 font-semibold text-zinc-300">
                    {k.sizeBytes > 1024
                      ? `${(k.sizeBytes / 1024).toFixed(1)} KB`
                      : `${k.sizeBytes} B`}
                  </td>

                  {/* Row Actions */}
                  <td className="py-3 px-4 text-right" onClick={(e) => e.stopPropagation()}>
                    <div className="flex items-center justify-end gap-1.5 opacity-80 group-hover:opacity-100">
                      <button
                        onClick={() => onSelectKey(k)}
                        className="p-1.5 rounded-lg text-zinc-400 hover:text-white hover:bg-white/[0.06] transition-colors"
                        title="Inspect & Edit"
                      >
                        <ExternalLink className="w-3.5 h-3.5" />
                      </button>
                      <button
                        onClick={() => {
                          if (confirm(`Delete key "${k.key}"?`)) {
                            onDeleteKey(k.key);
                          }
                        }}
                        className="p-1.5 rounded-lg text-zinc-400 hover:text-red-400 hover:bg-red-950/40 transition-colors"
                        title="Delete Key"
                      >
                        <Trash2 className="w-3.5 h-3.5" />
                      </button>
                    </div>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>

      {/* Pagination & Footer Controls */}
      <div className="flex flex-col sm:flex-row items-center justify-between p-4 border-t border-white/[0.06] bg-[#0c1018] gap-4 text-zinc-400 text-xs">
        <div className="flex items-center gap-3">
          <span>
            Showing{" "}
            <strong className="text-white">
              {Math.min(sortedKeys.length, (currentPage - 1) * rowsPerPage + 1)}
            </strong>{" "}
            to{" "}
            <strong className="text-white">
              {Math.min(sortedKeys.length, currentPage * rowsPerPage)}
            </strong>{" "}
            of <strong className="text-white">{sortedKeys.length}</strong> keys
          </span>

          {selectedKeys.size > 0 && (
            <span className="px-2 py-0.5 rounded bg-cyan-950 text-cyan-300 border border-cyan-800 text-[10px] font-bold">
              {selectedKeys.size} Selected
            </span>
          )}
        </div>

        <div className="flex items-center gap-4">
          {/* Rows per page selector */}
          <div className="flex items-center gap-2">
            <span>Rows:</span>
            <select
              value={rowsPerPage}
              onChange={(e) => {
                setRowsPerPage(Number(e.target.value));
                setCurrentPage(1);
              }}
              className="bg-black/60 border border-white/[0.08] rounded-lg px-2 py-1 text-white text-xs focus:outline-none"
            >
              {[10, 25, 50, 100].map((num) => (
                <option key={num} value={num}>
                  {num}
                </option>
              ))}
            </select>
          </div>

          {/* Page buttons */}
          <div className="flex items-center gap-1">
            <button
              disabled={currentPage <= 1}
              onClick={() => setCurrentPage((p) => Math.max(1, p - 1))}
              className="p-1.5 rounded-lg border border-white/[0.08] disabled:opacity-30 hover:bg-white/[0.05] text-zinc-300 disabled:hover:bg-transparent"
            >
              <ChevronLeft className="w-4 h-4" />
            </button>
            <span className="px-2 font-bold text-white">
              {currentPage} / {totalPages || 1}
            </span>
            <button
              disabled={currentPage >= totalPages}
              onClick={() => setCurrentPage((p) => Math.min(totalPages, p + 1))}
              className="p-1.5 rounded-lg border border-white/[0.08] disabled:opacity-30 hover:bg-white/[0.05] text-zinc-300 disabled:hover:bg-transparent"
            >
              <ChevronRight className="w-4 h-4" />
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
