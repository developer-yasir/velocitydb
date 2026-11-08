import { LucideIcon } from "lucide-react";

export type NavItem = {
  title: string;
  href: string;
  icon: LucideIcon;
  badge?: string;
  badgeVariant?: "cyan" | "orange" | "green" | "purple" | "default";
};

export type ConnectionStatus = "connected" | "connecting" | "disconnected" | "error";

export interface VelocityNode {
  id: string;
  name: string;
  host: string;
  clientPort: number;
  busPort: number;
  role: "master" | "replica";
  status: "online" | "degraded" | "offline";
  slotsRange: string;
  assignedSlotsCount: number;
  rssMemoryMB: number;
  coresCount: number;
}

export interface EngineStats {
  rps: number;
  p50LatencyMicros: number;
  p90LatencyMicros: number;
  p99LatencyMicros: number;
  totalKeys: number;
  usedMemoryBytes: number;
  maxMemoryBytes: number;
  evictionCount: number;
  hitRate: number;
  ioUringRingEntries: number;
  activeClients: number;
}

export interface CoreMetric {
  coreId: number;
  cpuPercent: number;
  opsPerSec: number;
  sqeDepth: number;
  cqePending: number;
  cacheKeysCount: number;
}

export type KeyType = "string" | "hash" | "list" | "zset" | "stream" | "bitmap";

export interface KeyRecord {
  key: string;
  type: KeyType;
  slot: number;
  targetNode: string;
  ttlSeconds: number | null; // null = persistent
  sizeBytes: number;
  lastAccessed: string;
}
