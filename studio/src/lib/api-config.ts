"use client";

/**
 * VelocityDB Studio Network Routing Configuration.
 * 
 * Dynamically resolves the API and WebSocket host from the browser window's location.
 * This guarantees that whether loaded on localhost, AWS EC2 (e.g. 54.172.168.90),
 * or behind a reverse proxy / custom domain, all connections target the correct machine.
 */

export function getApiBaseUrl(): string {
  if (typeof window !== "undefined" && window.location?.hostname) {
    return `${window.location.protocol}//${window.location.hostname}:8080`;
  }
  return process.env.NEXT_PUBLIC_API_URL || "http://localhost:8080";
}

export function getWsBaseUrl(): string {
  if (typeof window !== "undefined" && window.location?.hostname) {
    const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
    return `${proto}//${window.location.hostname}:8080`;
  }
  return process.env.NEXT_PUBLIC_WS_URL || "ws://localhost:8080";
}

export const API_BASE_URL: string = getApiBaseUrl();
export const WS_BASE_URL: string = getWsBaseUrl();
