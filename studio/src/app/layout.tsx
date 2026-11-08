import type { Metadata } from "next";
import "./globals.css";
import { Shell } from "@/components/layout/Shell";
import { ToastProvider } from "@/components/ui/Toast";

export const metadata: Metadata = {
  title: "VelocityDB Studio — Enterprise Thread-per-Core Cockpit",
  description:
    "Ultra-high-throughput real-time management console for VelocityDB. Powered by Linux io_uring, 100% lock-free shared-nothing architecture.",
  icons: {
    icon: "/favicon.ico",
  },
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="en" className="dark">
      <body className="bg-[#080a0f] text-zinc-100 antialiased selection:bg-cyan-500/30 selection:text-cyan-200">
        <ToastProvider>
          <Shell>{children}</Shell>
        </ToastProvider>
      </body>
    </html>
  );
}
