"use client";

import React, { useState } from "react";
import {
  Settings as SettingsIcon,
  ShieldCheck,
  Lock,
  HardDrive,
  Network,
  Users,
  Key,
  Mail,
  Phone,
  Save,
  Check,
} from "lucide-react";

export default function SettingsPage() {
  const [saved, setSaved] = useState(false);
  const [meetIp, setMeetIp] = useState("172.28.0.12");
  const [meetPort, setMeetPort] = useState("6380");

  const handleSave = () => {
    setSaved(true);
    setTimeout(() => setSaved(false), 2000);
  };

  return (
    <div className="space-y-8 animate-in fade-in duration-300 max-w-5xl">
      {/* Page Header */}
      <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4 pb-2 border-b border-white/[0.06]">
        <div>
          <div className="flex items-center gap-2.5">
            <h1 className="text-2xl font-black tracking-tight text-white font-mono flex items-center gap-2">
              CLUSTER &amp; SECURITY SETTINGS
            </h1>
            <span className="px-2 py-0.5 text-[10px] font-mono font-bold uppercase rounded bg-zinc-800 text-zinc-300 border border-zinc-700">
              ENTERPRISE ADMIN
            </span>
          </div>
          <p className="text-xs text-zinc-400 font-mono mt-1">
            Configure Gossip Bus parameters, zero-allocation ACL bitmasks, TLS 1.3 crypto, and direct I/O AOF engines.
          </p>
        </div>

        <button
          onClick={handleSave}
          className="flex items-center gap-2 px-4 py-2 rounded-xl bg-cyan-500 text-black font-mono font-bold text-xs hover:bg-cyan-400 transition-colors shadow-sm"
        >
          {saved ? <Check className="w-4 h-4" /> : <Save className="w-4 h-4" />}
          <span>{saved ? "CONFIG SAVED" : "SAVE CHANGES"}</span>
        </button>
      </div>

      {/* Section 1: Cluster Gossip Bus & Node Discovery */}
      <div className="glass-panel p-6 rounded-2xl space-y-4">
        <div className="flex items-center gap-2 pb-3 border-b border-white/[0.06]">
          <Network className="w-4 h-4 text-cyan-400" />
          <h3 className="text-sm font-black font-mono tracking-tight text-white uppercase">
            Cluster Gossip Bus Configuration
          </h3>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-4 font-mono text-xs">
          <div>
            <label className="text-zinc-400 block text-[11px] mb-1.5 uppercase font-semibold">
              Gossip Bus Magic Header
            </label>
            <input
              type="text"
              readOnly
              value="RCmb (0x52436d62)"
              className="w-full px-3 py-2 bg-black/40 border border-white/[0.08] rounded-xl text-zinc-300"
            />
          </div>

          <div>
            <label className="text-zinc-400 block text-[11px] mb-1.5 uppercase font-semibold">
              Topology Cache Strategy
            </label>
            <input
              type="text"
              readOnly
              value="ThreadLocal (Rc<RefCell<ClusterTopology>>) - Lock-Free"
              className="w-full px-3 py-2 bg-black/40 border border-white/[0.08] rounded-xl text-emerald-400 font-bold"
            />
          </div>
        </div>

        <div className="pt-2">
          <label className="text-zinc-400 block text-[11px] mb-1.5 uppercase font-semibold">
            Join Remote Node (CLUSTER MEET)
          </label>
          <div className="flex gap-3">
            <input
              type="text"
              value={meetIp}
              onChange={(e) => setMeetIp(e.target.value)}
              placeholder="IP Address (e.g. 172.28.0.12)"
              className="flex-1 px-3 py-2 bg-black/40 border border-white/[0.08] rounded-xl text-xs font-mono text-white"
            />
            <input
              type="text"
              value={meetPort}
              onChange={(e) => setMeetPort(e.target.value)}
              placeholder="Port (6380)"
              className="w-28 px-3 py-2 bg-black/40 border border-white/[0.08] rounded-xl text-xs font-mono text-white"
            />
            <button className="px-4 py-2 rounded-xl bg-white/[0.08] border border-white/10 text-xs font-mono font-bold text-white hover:bg-white/[0.12] transition-colors">
              MEET NODE
            </button>
          </div>
        </div>
      </div>

      {/* Section 2: TLS 1.3 & Cryptography Engine */}
      <div className="glass-panel p-6 rounded-2xl space-y-4">
        <div className="flex items-center gap-2 pb-3 border-b border-white/[0.06]">
          <Lock className="w-4 h-4 text-emerald-400" />
          <h3 className="text-sm font-black font-mono tracking-tight text-white uppercase">
            Native Non-Blocking TLS 1.3 (Rustls)
          </h3>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-3 gap-4 font-mono text-xs">
          <div className="p-3 rounded-xl bg-[#0d121c] border border-white/[0.06]">
            <span className="text-[10px] text-zinc-400 uppercase block">Crypto Provider</span>
            <span className="text-cyan-400 font-bold text-sm">Ring (BoringSSL Fork)</span>
          </div>
          <div className="p-3 rounded-xl bg-[#0d121c] border border-white/[0.06]">
            <span className="text-[10px] text-zinc-400 uppercase block">Cipher Suites</span>
            <span className="text-white font-bold text-sm">TLS13_AES_256_GCM_SHA384</span>
          </div>
          <div className="p-3 rounded-xl bg-[#0d121c] border border-white/[0.06]">
            <span className="text-[10px] text-zinc-400 uppercase block">TLS P99 Latency SLA</span>
            <span className="text-emerald-400 font-bold text-sm">18.1 µs (&lt; 5.0 ms SLA)</span>
          </div>
        </div>
      </div>

      {/* Section 3: Commercial License & Legal Support Inquiries */}
      <div className="glass-panel p-6 rounded-2xl space-y-4 border-cyan-500/30 shadow-[0_0_20px_-5px_rgba(0,242,254,0.1)]">
        <div className="flex items-center justify-between pb-3 border-b border-white/[0.06]">
          <div className="flex items-center gap-2">
            <ShieldCheck className="w-4 h-4 text-cyan-400" />
            <h3 className="text-sm font-black font-mono tracking-tight text-white uppercase">
              Dual-License &amp; Commercial Enterprise Inquiries
            </h3>
          </div>
          <span className="text-[10px] font-mono px-2 py-0.5 rounded uppercase font-bold bg-cyan-950 text-cyan-300 border border-cyan-800">
            AGPL v3 / Commercial
          </span>
        </div>

        <p className="text-xs font-mono text-zinc-300 leading-relaxed">
          VelocityDB is dual-licensed. Commercial entities, banks, and closed-source startups requiring an exemption from AGPL v3 copyleft disclosure or 24/7 SLA enterprise support must acquire a Commercial License.
        </p>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-4 pt-2 font-mono text-xs">
          <div className="flex items-center gap-3 p-3.5 rounded-xl bg-[#0d121c] border border-white/[0.06]">
            <div className="p-2 rounded-lg bg-cyan-500/10 text-cyan-400">
              <Mail className="w-4 h-4" />
            </div>
            <div>
              <span className="text-[10px] text-zinc-400 uppercase block">Commercial Inquiries</span>
              <a
                href="mailto:engyasirsaleem@gmail.com"
                className="text-white font-bold hover:text-cyan-400 transition-colors"
              >
                engyasirsaleem@gmail.com
              </a>
            </div>
          </div>

          <div className="flex items-center gap-3 p-3.5 rounded-xl bg-[#0d121c] border border-white/[0.06]">
            <div className="p-2 rounded-lg bg-emerald-500/10 text-emerald-400">
              <Phone className="w-4 h-4" />
            </div>
            <div>
              <span className="text-[10px] text-zinc-400 uppercase block">Direct WhatsApp</span>
              <a
                href="https://wa.me/923298594228"
                target="_blank"
                rel="noreferrer"
                className="text-white font-bold hover:text-emerald-400 transition-colors"
              >
                03298594228 (+92 329 8594228)
              </a>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
