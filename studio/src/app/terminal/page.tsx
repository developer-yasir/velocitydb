"use client";

import React from "react";
import { WebTerminal } from "@/components/terminal/WebTerminal";
import { Zap, Terminal, ShieldCheck, Cpu } from "lucide-react";

export default function TerminalPage() {
  return (
    <div className="space-y-6 animate-in fade-in duration-300">
      {/* Header */}
      <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4 pb-2 border-b border-white/[0.06]">
        <div>
          <div className="flex items-center gap-2.5">
            <h1 className="text-2xl font-black tracking-tight text-white font-mono flex items-center gap-2">
              IN-BROWSER TERMINAL
            </h1>
            <span className="px-2.5 py-0.5 text-[10px] font-mono font-bold uppercase rounded bg-orange-950/80 text-orange-300 border border-orange-800/60 shadow-[0_0_10px_rgba(249,115,22,0.3)]">
              RAW RESP SOCKET
            </span>
          </div>
          <p className="text-xs text-zinc-400 font-mono mt-1">
            Execute Redis-compatible commands directly with instantaneous microsecond feedback.
          </p>
        </div>

        {/* Engine Specs */}
        <div className="flex items-center gap-3 px-3.5 py-2 rounded-xl bg-[#0e131d] border border-white/[0.08] text-xs font-mono">
          <div className="flex items-center gap-2">
            <Zap className="w-3.5 h-3.5 text-cyan-400" />
            <span className="text-zinc-300">Socket Protocol:</span>
            <strong className="text-cyan-400">RESP2 / RESP3 Direct</strong>
          </div>
          <span className="text-zinc-600">|</span>
          <span className="text-emerald-400 font-bold">P99: 18.1 µs</span>
        </div>
      </div>

      {/* Linux Terminal Component */}
      <WebTerminal />
    </div>
  );
}
