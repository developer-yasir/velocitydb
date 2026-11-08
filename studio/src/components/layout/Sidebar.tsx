"use client";

import React, { useState } from "react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import {
  LayoutDashboard,
  Database,
  Terminal,
  Cpu,
  Settings,
  ChevronLeft,
  ChevronRight,
  Zap,
  Server,
  Activity,
  Layers,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { NavItem } from "@/types";

const NAV_ITEMS: NavItem[] = [
  {
    title: "Dashboard",
    href: "/",
    icon: LayoutDashboard,
    badge: "LIVE",
    badgeVariant: "green",
  },
  {
    title: "Data Explorer",
    href: "/explorer",
    icon: Database,
    badge: "16K SLOTS",
    badgeVariant: "cyan",
  },
  {
    title: "Terminal (RESP)",
    href: "/terminal",
    icon: Terminal,
    badge: "DIRECT",
    badgeVariant: "orange",
  },
  {
    title: "CPU / RAM Monitor",
    href: "/metrics",
    icon: Cpu,
    badge: "io_uring",
    badgeVariant: "purple",
  },
  {
    title: "Settings",
    href: "/settings",
    icon: Settings,
  },
];

export function Sidebar() {
  const [isCollapsed, setIsCollapsed] = useState(false);
  const pathname = usePathname();

  return (
    <aside
      className={cn(
        "relative flex flex-col h-screen select-none border-r border-white/[0.08] bg-[#0b0e14]/95 backdrop-blur-2xl transition-all duration-300 ease-in-out z-30",
        isCollapsed ? "w-16" : "w-64"
      )}
    >
      {/* Brand Header */}
      <div className="flex items-center justify-between h-16 px-4 border-b border-white/[0.08]">
        <Link href="/" className="flex items-center gap-3 overflow-hidden group">
          <div className="relative flex items-center justify-center w-9 h-9 rounded-xl bg-gradient-to-br from-cyan-500/20 via-blue-600/20 to-orange-500/10 border border-cyan-500/30 shadow-[0_0_15px_-3px_rgba(0,242,254,0.3)] group-hover:border-cyan-400/60 transition-colors">
            <Zap className="w-5 h-5 text-cyan-400 group-hover:scale-110 transition-transform duration-200" />
            <span className="absolute -top-0.5 -right-0.5 w-2 h-2 rounded-full bg-emerald-400 ring-2 ring-[#0b0e14] animate-pulse" />
          </div>

          {!isCollapsed && (
            <div className="flex flex-col">
              <div className="flex items-center gap-1.5">
                <span className="font-extrabold tracking-wider text-white text-sm">
                  VELOCITY<span className="text-cyan-400 font-black">DB</span>
                </span>
                <span className="px-1.5 py-0.2 text-[9px] font-mono font-bold uppercase rounded bg-cyan-950/80 text-cyan-300 border border-cyan-800/60">
                  STUDIO
                </span>
              </div>
              <span className="text-[10px] text-zinc-400 font-mono tracking-tight">
                v1.0.0-enterprise
              </span>
            </div>
          )}
        </Link>
      </div>

      {/* Navigation Links */}
      <div className="flex-1 py-4 px-2 space-y-1 overflow-y-auto">
        <div className={cn("px-2 pb-2 text-[10px] font-mono font-semibold uppercase tracking-wider text-zinc-400", isCollapsed && "sr-only")}>
          Core Navigation
        </div>

        {NAV_ITEMS.map((item) => {
          const isActive = pathname === item.href;
          const Icon = item.icon;

          return (
            <Link
              key={item.href}
              href={item.href}
              className={cn(
                "group relative flex items-center gap-3 px-3 py-2.5 rounded-lg text-xs font-medium transition-all duration-150",
                isActive
                  ? "bg-cyan-500/10 text-cyan-300 border border-cyan-500/30 shadow-[inset_0_1px_1px_rgba(255,255,255,0.05),0_0_15px_-4px_rgba(0,242,254,0.2)]"
                  : "text-zinc-400 hover:text-zinc-100 hover:bg-white/[0.04] border border-transparent"
              )}
              title={isCollapsed ? item.title : undefined}
            >
              <Icon
                className={cn(
                  "w-4 h-4 shrink-0 transition-transform duration-150 group-hover:scale-110",
                  isActive ? "text-cyan-400" : "text-zinc-400 group-hover:text-zinc-200"
                )}
              />

              {!isCollapsed && (
                <div className="flex items-center justify-between w-full">
                  <span className="truncate">{item.title}</span>

                  {item.badge && (
                    <span
                      className={cn(
                        "text-[9px] font-mono font-bold px-1.5 py-0.5 rounded uppercase tracking-wider",
                        item.badgeVariant === "green" && "bg-emerald-950/80 text-emerald-400 border border-emerald-800/60",
                        item.badgeVariant === "cyan" && "bg-cyan-950/80 text-cyan-400 border border-cyan-800/60",
                        item.badgeVariant === "orange" && "bg-orange-950/80 text-orange-400 border border-orange-800/60",
                        item.badgeVariant === "purple" && "bg-purple-950/80 text-purple-400 border border-purple-800/60",
                        !item.badgeVariant && "bg-zinc-800/80 text-zinc-300 border border-zinc-700/60"
                      )}
                    >
                      {item.badge}
                    </span>
                  )}
                </div>
              )}

              {/* Active Route Indicator bar */}
              {isActive && (
                <span className="absolute left-0 top-1/2 -translate-y-1/2 w-1 h-5 rounded-r-full bg-cyan-400 shadow-[0_0_8px_#00f2fe]" />
              )}
            </Link>
          );
        })}
      </div>

      {/* Cluster Node Summary Footer (Expanded Only) */}
      {!isCollapsed && (
        <div className="p-3 mx-2 mb-3 rounded-xl bg-gradient-to-b from-[#131924] to-[#0d121b] border border-white/[0.07] shadow-inner">
          <div className="flex items-center justify-between pb-2 border-b border-white/[0.05]">
            <div className="flex items-center gap-1.5">
              <Server className="w-3.5 h-3.5 text-cyan-400" />
              <span className="text-[11px] font-semibold text-zinc-200 font-mono">NODE-01 (LEADER)</span>
            </div>
            <span className="inline-flex items-center gap-1 text-[10px] font-mono text-emerald-400 font-bold">
              <span className="w-1.5 h-1.5 rounded-full bg-emerald-400 animate-pulse" />
              6379
            </span>
          </div>

          <div className="grid grid-cols-2 gap-2 pt-2 text-[10px] font-mono">
            <div>
              <span className="text-zinc-400 block text-[9px] uppercase">Engine Arch</span>
              <span className="text-zinc-300 font-bold flex items-center gap-1">
                <Layers className="w-3 h-3 text-orange-400" />
                io_uring 64C
              </span>
            </div>
            <div>
              <span className="text-zinc-400 block text-[9px] uppercase">Memory Mode</span>
              <span className="text-emerald-400 font-bold flex items-center gap-1">
                <Activity className="w-3 h-3 text-emerald-400" />
                LRU Ceiling
              </span>
            </div>
          </div>
        </div>
      )}

      {/* Collapse Toggle Footer */}
      <div className="p-2 border-t border-white/[0.08] flex items-center justify-end">
        <button
          onClick={() => setIsCollapsed(!isCollapsed)}
          className="flex items-center justify-center w-full h-8 rounded-lg text-zinc-400 hover:text-white hover:bg-white/[0.06] transition-colors"
          title={isCollapsed ? "Expand Sidebar" : "Collapse Sidebar"}
        >
          {isCollapsed ? (
            <ChevronRight className="w-4 h-4 text-cyan-400" />
          ) : (
            <div className="flex items-center gap-2 text-xs font-mono text-zinc-400">
              <ChevronLeft className="w-4 h-4 text-zinc-400" />
              <span>COLLAPSE NAV</span>
            </div>
          )}
        </button>
      </div>
    </aside>
  );
}
