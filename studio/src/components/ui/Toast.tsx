"use client";

import React, { createContext, useContext, useState, useCallback, ReactNode } from "react";
import { CheckCircle2, AlertCircle, AlertTriangle, Info, X } from "lucide-react";
import { IApiError } from "@/types";

export type ToastType = "success" | "error" | "warning" | "info";

export interface ToastItem {
  id: string;
  type: ToastType;
  title: string;
  message?: string;
  statusCode?: number;
  durationMs?: number;
}

interface ToastContextType {
  toasts: ToastItem[];
  addToast: (toast: Omit<ToastItem, "id">) => string;
  removeToast: (id: string) => void;
  showError: (error: IApiError | string, title?: string) => void;
  showSuccess: (title: string, message?: string) => void;
}

const ToastContext = createContext<ToastContextType | undefined>(undefined);

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<ToastItem[]>([]);

  const removeToast = useCallback((id: string) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  const addToast = useCallback(
    (toast: Omit<ToastItem, "id">) => {
      const id = `toast-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
      const newToast: ToastItem = { ...toast, id };

      setToasts((prev) => [...prev, newToast]);

      const duration = toast.durationMs ?? (toast.type === "error" ? 5000 : 3500);
      setTimeout(() => {
        removeToast(id);
      }, duration);

      return id;
    },
    [removeToast]
  );

  const showError = useCallback(
    (error: IApiError | string, title = "Operation Failed") => {
      if (typeof error === "string") {
        addToast({
          type: "error",
          title,
          message: error,
        });
      } else {
        addToast({
          type: "error",
          title: `${title} [${error.status}]`,
          message: error.message || error.details || "An unexpected error occurred.",
          statusCode: error.status,
        });
      }
    },
    [addToast]
  );

  const showSuccess = useCallback(
    (title: string, message?: string) => {
      addToast({
        type: "success",
        title,
        message,
      });
    },
    [addToast]
  );

  return (
    <ToastContext.Provider value={{ toasts, addToast, removeToast, showError, showSuccess }}>
      {children}
      <ToastContainer toasts={toasts} onDismiss={removeToast} />
    </ToastContext.Provider>
  );
}

export function useToast(): ToastContextType {
  const context = useContext(ToastContext);
  if (!context) {
    throw new Error("useToast must be used within a ToastProvider");
  }
  return context;
}

interface ToastContainerProps {
  toasts: ToastItem[];
  onDismiss: (id: string) => void;
}

function ToastContainer({ toasts, onDismiss }: ToastContainerProps) {
  if (toasts.length === 0) return null;

  return (
    <div className="fixed bottom-6 right-6 z-50 flex flex-col gap-2.5 max-w-md w-full pointer-events-none">
      {toasts.map((toast) => {
        const isError = toast.type === "error";
        const isSuccess = toast.type === "success";
        const isWarning = toast.type === "warning";

        const borderClass = isError
          ? "border-rose-500/40 shadow-[0_0_20px_rgba(244,63,94,0.2)] bg-[#12080a]/95"
          : isSuccess
          ? "border-emerald-500/40 shadow-[0_0_20px_rgba(16,185,129,0.2)] bg-[#07130e]/95"
          : isWarning
          ? "border-amber-500/40 shadow-[0_0_20px_rgba(245,158,11,0.2)] bg-[#140e06]/95"
          : "border-cyan-500/40 shadow-[0_0_20px_rgba(0,242,254,0.2)] bg-[#061017]/95";

        const iconColor = isError
          ? "text-rose-400"
          : isSuccess
          ? "text-emerald-400"
          : isWarning
          ? "text-amber-400"
          : "text-cyan-400";

        return (
          <div
            key={toast.id}
            className={`pointer-events-auto p-4 rounded-xl border backdrop-blur-xl flex items-start gap-3 transition-all duration-200 animate-in slide-in-from-bottom-3 font-mono text-xs ${borderClass}`}
          >
            <div className={`mt-0.5 flex-shrink-0 ${iconColor}`}>
              {isError && <AlertCircle className="w-4 h-4" />}
              {isSuccess && <CheckCircle2 className="w-4 h-4" />}
              {isWarning && <AlertTriangle className="w-4 h-4" />}
              {toast.type === "info" && <Info className="w-4 h-4" />}
            </div>

            <div className="flex-1 space-y-0.5">
              <div className="font-bold text-white tracking-tight flex items-center justify-between">
                <span>{toast.title}</span>
                {toast.statusCode && (
                  <span className="text-[10px] px-1.5 py-0.2 rounded bg-black/40 border border-white/10 text-zinc-400">
                    HTTP {toast.statusCode}
                  </span>
                )}
              </div>
              {toast.message && (
                <p className="text-[11px] text-zinc-300 leading-relaxed break-words">
                  {toast.message}
                </p>
              )}
            </div>

            <button
              onClick={() => onDismiss(toast.id)}
              className="text-zinc-500 hover:text-white transition-colors p-0.5 rounded hover:bg-white/[0.05]"
              title="Dismiss"
            >
              <X className="w-3.5 h-3.5" />
            </button>
          </div>
        );
      })}
    </div>
  );
}