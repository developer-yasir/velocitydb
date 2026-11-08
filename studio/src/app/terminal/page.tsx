"use client";

import React, { useState, useRef, useEffect } from "react";
import {
  Terminal as TerminalIcon,
  Play,
  RotateCcw,
  Copy,
  Check,
  Zap,
  Info,
} from "lucide-react";

interface ConsoleLog {
  id: string;
  type: "command" | "response" | "error" | "system";
  content: string;
  latency?: string;
}

const INITIAL_LOGS: ConsoleLog[] = [
  {
    id: "log-1",
    type: "system",
    content: "Connected to VelocityDB Cluster Engine v1.0.0-enterprise (io_uring runtime).",
  },
  {
    id: "log-2",
    type: "system",
    content: "Type 'HELP' or select quick commands below to test high-throughput RESP primitives.",
  },
  {
    id: "log-3",
    type: "command",
    content: "PING",
  },
  {
    id: "log-4",
    type: "response",
    content: "+PONG",
    latency: "1.2 µs",
  },
  {
    id: "log-5",
    type: "command",
    content: "CLUSTER INFO",
  },
  {
    id: "log-6",
    type: "response",
    content: `cluster_state:ok\r\ncluster_slots_assigned:16384\r\ncluster_slots_ok:16384\r\ncluster_known_nodes:3\r\ncluster_size:3\r\ncluster_current_epoch:1\r\ncluster_stats_messages_sent:89421\r\ncluster_stats_messages_received:89419`,
    latency: "3.4 µs",
  },
];

export default function TerminalPage() {
  const [logs, setLogs] = useState<ConsoleLog[]>(INITIAL_LOGS);
  const [inputVal, setInputVal] = useState("");
  const [history, setHistory] = useState<string[]>(["PING", "CLUSTER INFO"]);
  const [historyIndex, setHistoryIndex] = useState(-1);
  const terminalEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    terminalEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [logs]);

  const handleExecute = (cmdToRun?: string) => {
    const rawCmd = (cmdToRun || inputVal).trim();
    if (!rawCmd) return;

    const newCmdLog: ConsoleLog = {
      id: `cmd-${Date.now()}`,
      type: "command",
      content: rawCmd,
    };

    setLogs((prev) => [...prev, newCmdLog]);
    setHistory((prev) => [...prev, rawCmd]);
    setHistoryIndex(-1);
    setInputVal("");

    // Simulate instant RESP response based on command
    setTimeout(() => {
      const parts = rawCmd.split(/\s+/);
      const mainCmd = parts[0].toUpperCase();
      let resContent = "";
      let resType: "response" | "error" = "response";
      let latency = `${(1.2 + Math.random() * 2.5).toFixed(1)} µs`;

      switch (mainCmd) {
        case "PING":
          resContent = "+PONG";
          break;
        case "SET":
          resContent = "+OK";
          break;
        case "GET":
          resContent = parts[1]?.includes("user") ? `"Yasir Saleem"` : `"online"`;
          break;
        case "DEL":
          resContent = ":1";
          break;
        case "CLUSTER":
          if (parts[1]?.toUpperCase() === "SLOTS") {
            resContent = `1) 1) (integer) 0\r\n   2) (integer) 5461\r\n   3) 1) "127.0.0.1"\r\n      2) (integer) 6379\r\n2) 1) (integer) 5462\r\n   2) (integer) 10922\r\n   3) 1) "127.0.0.1"\r\n      2) (integer) 6380\r\n3) 1) (integer) 10923\r\n   2) (integer) 16383\r\n   3) 1) "127.0.0.1"\r\n      2) (integer) 6381`;
          } else {
            resContent = `cluster_state:ok\r\ncluster_slots_assigned:16384\r\ncluster_size:3`;
          }
          break;
        case "INFO":
          resContent = `# Replication\r\nrole:master\r\nconnected_slaves:2\r\nmaster_replid:8a4f1092eacdb871109927b\r\nmaster_repl_offset:10485760\r\nsecond_repl_offset:-1`;
          break;
        case "HELP":
          resContent = `VelocityDB Supported Commands:\r\n- Strings: SET, GET, DEL, EXPIRE, TTL, PING\r\n- Hashes: HSET, HGET, HDEL, HLEN, HGETALL\r\n- Lists: LPUSH, RPUSH, LPOP, RPOP, LLEN, LRANGE\r\n- Streams: XADD, XREAD, XRANGE, XLEN\r\n- Bitmaps: SETBIT, GETBIT, BITCOUNT\r\n- Cluster: CLUSTER INFO, CLUSTER NODES, CLUSTER SLOTS`;
          break;
        case "CLEAR":
          setLogs([]);
          return;
        default:
          resContent = `-ERR unknown or unsupported command '${rawCmd}'`;
          resType = "error";
      }

      setLogs((prev) => [
        ...prev,
        {
          id: `res-${Date.now()}`,
          type: resType,
          content: resContent,
          latency,
        },
      ]);
    }, 40);
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      handleExecute();
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      if (history.length > 0) {
        const nextIdx = historyIndex === -1 ? history.length - 1 : Math.max(0, historyIndex - 1);
        setHistoryIndex(nextIdx);
        setInputVal(history[nextIdx]);
      }
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      if (historyIndex !== -1) {
        const nextIdx = historyIndex + 1;
        if (nextIdx >= history.length) {
          setHistoryIndex(-1);
          setInputVal("");
        } else {
          setHistoryIndex(nextIdx);
          setInputVal(history[nextIdx]);
        }
      }
    }
  };

  return (
    <div className="space-y-6 animate-in fade-in duration-300">
      {/* Page Header */}
      <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4 pb-2 border-b border-white/[0.06]">
        <div>
          <div className="flex items-center gap-2.5">
            <h1 className="text-2xl font-black tracking-tight text-white font-mono flex items-center gap-2">
              RESP TERMINAL CONSOLE
            </h1>
            <span className="px-2 py-0.5 text-[10px] font-mono font-bold uppercase rounded bg-orange-950/80 text-orange-300 border border-orange-800/60">
              DIRECT PROTOCOL SOCKET
            </span>
          </div>
          <p className="text-xs text-zinc-400 font-mono mt-1">
            Zero-overhead TCP connection testing against VelocityDB via raw RESP2 / RESP3 frames.
          </p>
        </div>

        <button
          onClick={() => setLogs([])}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl bg-white/[0.05] border border-white/[0.08] text-xs font-mono text-zinc-400 hover:text-white hover:bg-white/[0.08] transition-colors"
        >
          <RotateCcw className="w-3.5 h-3.5" />
          <span>Clear Screen</span>
        </button>
      </div>

      {/* Terminal Window Box */}
      <div className="glass-panel rounded-2xl overflow-hidden border border-white/10 shadow-2xl flex flex-col h-[calc(100vh-250px)]">
        {/* Title Bar */}
        <div className="flex items-center justify-between px-4 py-3 bg-[#0d121c] border-b border-white/[0.08]">
          <div className="flex items-center gap-2">
            <span className="w-3 h-3 rounded-full bg-red-500/80 inline-block" />
            <span className="w-3 h-3 rounded-full bg-yellow-500/80 inline-block" />
            <span className="w-3 h-3 rounded-full bg-emerald-500/80 inline-block" />
            <span className="ml-2 text-xs font-mono text-zinc-400">
              io_uring@velocitydb-aws-node-01:6379 (Client RESP Session)
            </span>
          </div>

          <div className="flex items-center gap-2 text-[11px] font-mono text-emerald-400">
            <Zap className="w-3.5 h-3.5" />
            <span>0 ms Network RTT</span>
          </div>
        </div>

        {/* Terminal Logs Output Viewport */}
        <div className="flex-1 p-6 font-mono text-xs overflow-y-auto space-y-3 bg-[#080b11]">
          {logs.map((log) => (
            <div key={log.id} className="leading-relaxed">
              {log.type === "system" && (
                <div className="text-zinc-500 italic flex items-center gap-2">
                  <Info className="w-3.5 h-3.5 text-cyan-500" />
                  <span>{log.content}</span>
                </div>
              )}

              {log.type === "command" && (
                <div className="flex items-center gap-2 text-white font-bold">
                  <span className="text-cyan-400">velocitydb:6379&gt;</span>
                  <span>{log.content}</span>
                </div>
              )}

              {log.type === "response" && (
                <div className="pl-4 border-l-2 border-emerald-500/30 text-emerald-400 whitespace-pre-wrap flex justify-between items-start">
                  <span>{log.content}</span>
                  {log.latency && (
                    <span className="text-[10px] text-zinc-500 ml-4 font-normal shrink-0">
                      [{log.latency}]
                    </span>
                  )}
                </div>
              )}

              {log.type === "error" && (
                <div className="pl-4 border-l-2 border-red-500/50 text-red-400 whitespace-pre-wrap">
                  {log.content}
                </div>
              )}
            </div>
          ))}
          <div ref={terminalEndRef} />
        </div>

        {/* Input Bar */}
        <div className="p-3 bg-[#0d121c] border-t border-white/[0.08] flex items-center gap-3">
          <span className="text-cyan-400 font-mono font-bold text-xs pl-2">
            velocitydb:6379&gt;
          </span>
          <input
            type="text"
            value={inputVal}
            onChange={(e) => setInputVal(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="Type RESP command (e.g. SET {user:1}tag 'active', PING, CLUSTER SLOTS)..."
            className="flex-1 bg-transparent font-mono text-xs text-white placeholder:text-zinc-600 focus:outline-none"
            autoFocus
          />
          <button
            onClick={() => handleExecute()}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-cyan-500 text-black font-mono font-bold text-xs hover:bg-cyan-400 transition-colors shadow-sm"
          >
            <Play className="w-3 h-3 fill-current" />
            <span>SEND</span>
          </button>
        </div>
      </div>

      {/* Quick Snippet Chips */}
      <div className="flex items-center gap-2 overflow-x-auto text-xs font-mono">
        <span className="text-zinc-500 shrink-0 text-[11px] uppercase">Quick Probes:</span>
        {[
          "PING",
          "SET {tenant:1}name 'VelocityDB'",
          "GET {tenant:1}name",
          "CLUSTER SLOTS",
          "CLUSTER INFO",
          "INFO replication",
        ].map((cmd) => (
          <button
            key={cmd}
            onClick={() => handleExecute(cmd)}
            className="px-2.5 py-1 rounded-lg bg-surface border border-white/[0.06] text-zinc-300 hover:text-cyan-300 hover:border-cyan-500/40 hover:bg-surface-hover transition-colors shrink-0"
          >
            {cmd}
          </button>
        ))}
      </div>
    </div>
  );
}
