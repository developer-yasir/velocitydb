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
  Layers,
  ChevronRight,
} from "lucide-react";

interface TerminalLine {
  id: string;
  type: "command" | "response" | "integer" | "null" | "error" | "system" | "array";
  content: string | string[];
  latency?: string;
  timestamp: string;
}

const INITIAL_GREETING: TerminalLine[] = [
  {
    id: "g-1",
    type: "system",
    content: "VelocityDB v1.0.0-enterprise (x86_64-unknown-linux-gnu / io_uring)",
    timestamp: "12:00:00",
  },
  {
    id: "g-2",
    type: "system",
    content: "Cluster mode active: 16,384 consistent hash slots across 3 nodes.",
    timestamp: "12:00:00",
  },
  {
    id: "g-3",
    type: "system",
    content: "Connected to local socket 127.0.0.1:6379 via raw non-blocking RESP3 protocol.",
    timestamp: "12:00:00",
  },
  {
    id: "g-4",
    type: "system",
    content: "Type 'HELP' for a list of commands, or test with: SET {user:1}name 'Yasir', GET {user:1}name",
    timestamp: "12:00:00",
  },
];

export function WebTerminal() {
  const [lines, setLines] = useState<TerminalLine[]>(INITIAL_GREETING);
  const [input, setInput] = useState("");
  const [history, setHistory] = useState<string[]>([]);
  const [historyIndex, setHistoryIndex] = useState(-1);
  const [isCopied, setIsCopied] = useState(false);

  // In-memory simulated storage for realistic interactive responses!
  const storeRef = useRef<
    Map<string, { value: string | number | boolean | Record<string, unknown>; type: string; ttl: number | null }>
  >(
    new Map([
      ["{tenant:101}session:user_token:9941", { value: "eyJhbGciOi...", type: "string", ttl: 3412 }],
      ["{user:8912}name", { value: "Yasir Saleem", type: "string", ttl: null }],
      ["{cluster:master}status", { value: "online", type: "string", ttl: null }],
    ])
  );

  const terminalViewportRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  // Auto-scroll on new output
  useEffect(() => {
    terminalViewportRef.current?.scrollTo({
      top: terminalViewportRef.current.scrollHeight,
      behavior: "smooth",
    });
  }, [lines]);

  const handleExecute = (rawCommand?: string) => {
    const cmd = (rawCommand || input).trim();
    if (!cmd) return;

    const now = new Date();
    const timeStr = now.toTimeString().split(" ")[0];

    // Add user command line
    const cmdLine: TerminalLine = {
      id: `cmd-${Date.now()}`,
      type: "command",
      content: cmd,
      timestamp: timeStr,
    };

    setLines((prev) => [...prev, cmdLine]);
    setHistory((prev) => [...prev, cmd]);
    setHistoryIndex(-1);
    setInput("");

    // Process command against in-memory store
    const tokens = cmd.match(/(?:[^\s"]+|"[^"]*")+/g) || [];
    const commandName = tokens[0]?.toUpperCase();
    const cleanTokens = tokens.map((t) => t.replace(/^"|"$/g, ""));

    const latency = `${(0.8 + Math.random() * 2.2).toFixed(1)} µs`;

    let responseLine: TerminalLine;

    switch (commandName) {
      case "CLEAR":
        setLines([]);
        return;

      case "PING": {
        const reply = cleanTokens[1] ? `"${cleanTokens[1]}"` : "+PONG";
        responseLine = {
          id: `res-${Date.now()}`,
          type: "response",
          content: reply,
          latency,
          timestamp: timeStr,
        };
        break;
      }

      case "SET": {
        const key = cleanTokens[1];
        const val = cleanTokens[2];
        if (!key || val === undefined) {
          responseLine = {
            id: `res-${Date.now()}`,
            type: "error",
            content: "-ERR wrong number of arguments for 'set' command",
            latency,
            timestamp: timeStr,
          };
        } else {
          // Check for EX <seconds>
          let ttl = null;
          if (cleanTokens[3]?.toUpperCase() === "EX" && cleanTokens[4]) {
            ttl = parseInt(cleanTokens[4], 10) || null;
          }
          storeRef.current.set(key, { value: val, type: "string", ttl });
          responseLine = {
            id: `res-${Date.now()}`,
            type: "response",
            content: "+OK",
            latency,
            timestamp: timeStr,
          };
        }
        break;
      }

      case "GET": {
        const key = cleanTokens[1];
        if (!key) {
          responseLine = {
            id: `res-${Date.now()}`,
            type: "error",
            content: "-ERR wrong number of arguments for 'get' command",
            latency,
            timestamp: timeStr,
          };
        } else {
          const entry = storeRef.current.get(key);
          if (!entry) {
            responseLine = {
              id: `res-${Date.now()}`,
              type: "null",
              content: "(nil)",
              latency,
              timestamp: timeStr,
            };
          } else {
            responseLine = {
              id: `res-${Date.now()}`,
              type: "response",
              content: `"${entry.value}"`,
              latency,
              timestamp: timeStr,
            };
          }
        }
        break;
      }

      case "DEL": {
        const key = cleanTokens[1];
        const deleted = storeRef.current.delete(key);
        responseLine = {
          id: `res-${Date.now()}`,
          type: "integer",
          content: `(integer) ${deleted ? 1 : 0}`,
          latency,
          timestamp: timeStr,
        };
        break;
      }

      case "TTL": {
        const key = cleanTokens[1];
        const entry = storeRef.current.get(key);
        let ttlVal = -2; // does not exist
        if (entry) {
          ttlVal = entry.ttl !== null ? entry.ttl : -1; // -1 = persist
        }
        responseLine = {
          id: `res-${Date.now()}`,
          type: "integer",
          content: `(integer) ${ttlVal}`,
          latency,
          timestamp: timeStr,
        };
        break;
      }

      case "KEYS": {
        const pattern = cleanTokens[1] || "*";
        const allKeys = Array.from(storeRef.current.keys());
        const matching =
          pattern === "*"
            ? allKeys
            : allKeys.filter((k) => k.includes(pattern.replace(/\*/g, "")));

        responseLine = {
          id: `res-${Date.now()}`,
          type: "array",
          content: matching.map((k, i) => `${i + 1}) "${k}"`),
          latency,
          timestamp: timeStr,
        };
        break;
      }

      case "CLUSTER": {
        const sub = cleanTokens[1]?.toUpperCase();
        if (sub === "INFO") {
          responseLine = {
            id: `res-${Date.now()}`,
            type: "response",
            content: `cluster_state:ok\r\ncluster_slots_assigned:16384\r\ncluster_slots_ok:16384\r\ncluster_known_nodes:3\r\ncluster_size:3\r\ncluster_current_epoch:1\r\ncluster_my_epoch:1\r\ncluster_stats_messages_sent:149201\r\ncluster_stats_messages_received:149198`,
            latency,
            timestamp: timeStr,
          };
        } else if (sub === "SLOTS") {
          responseLine = {
            id: `res-${Date.now()}`,
            type: "array",
            content: [
              `1) 1) (integer) 0`,
              `   2) (integer) 5461`,
              `   3) 1) "127.0.0.1"`,
              `      2) (integer) 6379`,
              `2) 1) (integer) 5462`,
              `   2) (integer) 10922`,
              `   3) 1) "127.0.0.1"`,
              `      2) (integer) 6380`,
              `3) 1) (integer) 10923`,
              `   2) (integer) 16383`,
              `   3) 1) "127.0.0.1"`,
              `      2) (integer) 6381`,
            ],
            latency,
            timestamp: timeStr,
          };
        } else if (sub === "NODES") {
          responseLine = {
            id: `res-${Date.now()}`,
            type: "response",
            content: `07c37dfeb2352134135 127.0.0.1:6379@16379 myself,master - 0 1731057400000 1 connected 0-5461\r\n6750d1c05873e3518a9 127.0.0.1:6380@16380 master - 0 1731057400000 2 connected 5462-10922\r\n39a489110a26e84992b 127.0.0.1:6381@16381 master - 0 1731057400000 3 connected 10923-16383`,
            latency,
            timestamp: timeStr,
          };
        } else {
          responseLine = {
            id: `res-${Date.now()}`,
            type: "error",
            content: `-ERR unknown cluster subcommand '${cleanTokens[1]}'`,
            latency,
            timestamp: timeStr,
          };
        }
        break;
      }

      case "INFO": {
        responseLine = {
          id: `res-${Date.now()}`,
          type: "response",
          content: `# Server\r\nvelocitydb_version:1.0.0-enterprise\r\nos:Linux 6.8.0-45-generic x86_64\r\narch_bits:64\r\nmultiplexing_api:io_uring\r\nprocess_id:4819\r\ntcp_port:6379\r\nuptime_in_seconds:86400\r\n\r\n# Memory\r\nused_memory:2097152\r\nused_memory_human:2.00M\r\nmaxmemory:2097152\r\nmaxmemory_policy:allkeys-lru\r\nmem_allocator:mimalloc\r\n\r\n# Replication\r\nrole:master\r\nconnected_slaves:2\r\nmaster_repl_offset:10485760`,
          latency,
          timestamp: timeStr,
        };
        break;
      }

      case "HELP": {
        responseLine = {
          id: `res-${Date.now()}`,
          type: "system",
          content: `VelocityDB Commands Guide:\r\n- SET key value [EX sec]    : Set key with optional TTL\r\n- GET key                   : Get key value\r\n- DEL key                   : Delete key\r\n- TTL key                   : Check expiration remaining\r\n- KEYS pattern              : Find keys matching pattern\r\n- PING [msg]                : Probe connection latency\r\n- CLUSTER INFO              : Show cluster topology state\r\n- CLUSTER SLOTS             : Show 16384 slot allocation\r\n- CLUSTER NODES             : List all nodes in gossip bus\r\n- INFO                      : Display engine memory & replication stats\r\n- CLEAR                     : Clear the terminal console`,
          latency,
          timestamp: timeStr,
        };
        break;
      }

      default:
        responseLine = {
          id: `res-${Date.now()}`,
          type: "error",
          content: `-ERR unknown command '${commandName}'`,
          latency,
          timestamp: timeStr,
        };
    }

    setLines((prev) => [...prev, responseLine]);
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      handleExecute();
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      if (history.length > 0) {
        const nextIdx = historyIndex === -1 ? history.length - 1 : Math.max(0, historyIndex - 1);
        setHistoryIndex(nextIdx);
        setInput(history[nextIdx]);
      }
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      if (historyIndex !== -1) {
        const nextIdx = historyIndex + 1;
        if (nextIdx >= history.length) {
          setHistoryIndex(-1);
          setInput("");
        } else {
          setHistoryIndex(nextIdx);
          setInput(history[nextIdx]);
        }
      }
    } else if (e.ctrlKey && e.key === "l") {
      e.preventDefault();
      setLines([]);
    }
  };

  const handleCopyLogs = () => {
    const text = lines
      .map((l) => (Array.isArray(l.content) ? l.content.join("\n") : l.content))
      .join("\n");
    navigator.clipboard.writeText(text);
    setIsCopied(true);
    setTimeout(() => setIsCopied(false), 1500);
  };

  return (
    <div className="flex flex-col h-[calc(100vh-210px)] rounded-2xl bg-[#06080e] border border-white/[0.1] shadow-[0_0_50px_-10px_rgba(0,0,0,0.8)] overflow-hidden font-mono text-xs select-text">
      {/* Linux Titlebar */}
      <div className="flex items-center justify-between px-4 py-3 bg-[#0d121c] border-b border-white/[0.08] select-none">
        <div className="flex items-center gap-3">
          {/* Mac/Linux Window Controls */}
          <div className="flex items-center gap-1.5">
            <span className="w-3 h-3 rounded-full bg-[#ff5f56] inline-block shadow-sm" />
            <span className="w-3 h-3 rounded-full bg-[#ffbd2e] inline-block shadow-sm" />
            <span className="w-3 h-3 rounded-full bg-[#27c93f] inline-block shadow-sm" />
          </div>

          <div className="flex items-center gap-2 pl-2 text-zinc-400 text-xs">
            <TerminalIcon className="w-3.5 h-3.5 text-cyan-400" />
            <span className="font-semibold text-zinc-300">
              ubuntu@velocitydb-aws-node-01: ~ (io_uring raw socket session • port 6379)
            </span>
          </div>
        </div>

        <div className="flex items-center gap-3">
          {/* Connection Pill */}
          <div className="flex items-center gap-2 text-[11px] text-emerald-400">
            <span className="relative flex h-2 w-2">
              <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75" />
              <span className="relative inline-flex rounded-full h-2 w-2 bg-emerald-500 shadow-[0_0_8px_#10b981]" />
            </span>
            <span className="font-bold">CONNECTED (0.1ms RTT)</span>
          </div>

          {/* Action buttons */}
          <div className="flex items-center gap-1 pl-2 border-l border-white/[0.08]">
            <button
              onClick={handleCopyLogs}
              className="p-1.5 rounded-lg text-zinc-400 hover:text-white hover:bg-white/[0.06] transition-colors"
              title="Copy Terminal Logs"
            >
              {isCopied ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : <Copy className="w-3.5 h-3.5" />}
            </button>
            <button
              onClick={() => setLines([])}
              className="p-1.5 rounded-lg text-zinc-400 hover:text-white hover:bg-white/[0.06] transition-colors"
              title="Clear Console (Ctrl+L)"
            >
              <RotateCcw className="w-3.5 h-3.5" />
            </button>
          </div>
        </div>
      </div>

      {/* Terminal Output Log Area */}
      <div
        ref={terminalViewportRef}
        className="flex-1 p-6 overflow-y-auto space-y-3 bg-[#06080e] leading-relaxed selection:bg-cyan-500/40 selection:text-white"
        onClick={() => inputRef.current?.focus()}
      >
        {lines.map((l) => (
          <div key={l.id} className="space-y-1">
            {l.type === "command" && (
              <div className="flex items-center gap-2 text-white font-bold">
                <span className="text-cyan-400 select-none">velocitydb:6379&gt;</span>
                <span className="text-white">{l.content}</span>
              </div>
            )}

            {l.type === "response" && (
              <div className="flex items-start justify-between gap-4 pl-4 border-l-2 border-emerald-500/30 text-emerald-400 whitespace-pre-wrap">
                <span>{l.content}</span>
                {l.latency && (
                  <span className="text-[10px] text-zinc-600 select-none font-normal shrink-0">
                    [{l.latency}]
                  </span>
                )}
              </div>
            )}

            {l.type === "integer" && (
              <div className="flex items-start justify-between gap-4 pl-4 border-l-2 border-cyan-500/30 text-cyan-300 font-bold">
                <span>{l.content}</span>
                {l.latency && (
                  <span className="text-[10px] text-zinc-600 select-none font-normal shrink-0">
                    [{l.latency}]
                  </span>
                )}
              </div>
            )}

            {l.type === "null" && (
              <div className="flex items-start justify-between gap-4 pl-4 border-l-2 border-zinc-700 text-zinc-500 italic">
                <span>{l.content}</span>
                {l.latency && (
                  <span className="text-[10px] text-zinc-600 select-none font-normal shrink-0">
                    [{l.latency}]
                  </span>
                )}
              </div>
            )}

            {l.type === "array" && Array.isArray(l.content) && (
              <div className="pl-4 border-l-2 border-purple-500/30 text-purple-300 space-y-0.5">
                {l.content.map((item, idx) => (
                  <div key={idx}>{item}</div>
                ))}
              </div>
            )}

            {l.type === "error" && (
              <div className="pl-4 border-l-2 border-red-500/50 text-red-400">
                {l.content}
              </div>
            )}

            {l.type === "system" && (
              <div className="text-zinc-500 flex items-center gap-2 italic">
                <Info className="w-3.5 h-3.5 text-cyan-500/70 shrink-0" />
                <span className="whitespace-pre-wrap">{l.content}</span>
              </div>
            )}
          </div>
        ))}
      </div>

      {/* Terminal Command Input Prompt */}
      <div className="p-3.5 bg-[#0a0d16] border-t border-white/[0.08] flex items-center gap-3 select-none">
        <span className="text-cyan-400 font-bold pl-2 flex items-center gap-1 shrink-0">
          <span>velocitydb:6379</span>
          <ChevronRight className="w-4 h-4 text-cyan-400" />
        </span>

        <input
          ref={inputRef}
          type="text"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="Type command (e.g. SET mykey 'hello', GET mykey, CLUSTER SLOTS, INFO)..."
          className="flex-1 bg-transparent text-white font-mono text-xs placeholder:text-zinc-600 focus:outline-none"
          autoFocus
        />

        <button
          onClick={() => handleExecute()}
          className="flex items-center gap-1.5 px-3.5 py-1.5 rounded-xl bg-cyan-400 text-black font-black text-xs hover:bg-cyan-300 transition-colors shadow-sm active:scale-95 shrink-0"
        >
          <Play className="w-3 h-3 fill-current" />
          <span>EXEC</span>
        </button>
      </div>

      {/* Quick Preset Buttons Bar */}
      <div className="flex items-center gap-2 px-4 py-2.5 bg-[#07090f] border-t border-white/[0.05] overflow-x-auto text-xs font-mono scrollbar-none">
        <span className="text-[10px] text-zinc-500 uppercase tracking-wider shrink-0">
          Live Probe Presets:
        </span>
        {[
          "PING",
          "SET {user:1}name 'Yasir Saleem'",
          "GET {user:1}name",
          "TTL {user:1}name",
          "KEYS *",
          "CLUSTER INFO",
          "CLUSTER SLOTS",
          "INFO",
        ].map((cmd) => (
          <button
            key={cmd}
            onClick={() => handleExecute(cmd)}
            className="px-2.5 py-1 rounded-lg bg-white/[0.04] border border-white/[0.06] text-zinc-300 hover:text-cyan-300 hover:border-cyan-500/30 hover:bg-white/[0.08] transition-colors shrink-0 text-[11px]"
          >
            {cmd}
          </button>
        ))}
      </div>
    </div>
  );
}
