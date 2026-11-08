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
  value?: string;
}

export interface IVelocityKey {
  key: string;
  key_type: string;
  slot: number;
  target_node: string;
  ttl_seconds: number | null;
  size_bytes: number;
  value: string;
  last_accessed: string;
}

export interface IApiError {
  status: number;
  message: string;
  details?: string;
}

export interface CoreTelemetryPayload {
  core_id: number;
  load_percent: number;
  ops_sec: number;
  sqe_depth: number;
  cqe_pending: number;
  keys_count: number;
}

export interface TelemetryPayload {
  timestamp_ms: number;
  rps: number;
  p50_latency_micros: number;
  p99_latency_micros: number;
  cluster_state: string;
  cluster_slots_assigned: number;
  used_memory_bytes: number;
  max_memory_bytes: number;
  eviction_count: number;
  active_connections: number;
  cpu_cores: CoreTelemetryPayload[];
}

export type TelemetryConnectionState =
  | "connected"
  | "connecting"
  | "reconnecting"
  | "disconnected"
  | "error";
