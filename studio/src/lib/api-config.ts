"use client";

/**
 * VelocityDB Studio Network Routing Configuration.
 * 
 * Supports dynamic browser host detection and explicit environment variable overrides
 * for Docker Compose / Kubernetes container networks.
 */
export const API_BASE_URL: string =
  process.env.NEXT_PUBLIC_API_URL ||
  (typeof window !== "undefined"
    ? `${window.location.protocol}//${window.location.hostname}:8080`
    : "http://localhost:8080");

export const WS_BASE_URL: string =
  process.env.NEXT_PUBLIC_WS_URL ||
  (typeof window !== "undefined"
    ? `${window.location.protocol === "https:" ? "wss:" : "ws:"}//${window.location.hostname}:8080`
    : "ws://localhost:8080");
