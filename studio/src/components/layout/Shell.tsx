"use client";

import React from "react";
import { Sidebar } from "./Sidebar";
import { Navbar } from "./Navbar";

export function Shell({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex h-screen w-screen overflow-hidden bg-[#080a0f] text-foreground bg-grid-pattern">
      {/* Collapsible Left Navigation Sidebar */}
      <Sidebar />

      {/* Main Content Area */}
      <div className="flex flex-col flex-1 h-screen overflow-hidden">
        {/* Top Navbar */}
        <Navbar />

        {/* Dynamic Scrollable Page Content */}
        <main className="flex-1 overflow-y-auto overflow-x-hidden p-6 md:p-8 space-y-8">
          {children}
        </main>
      </div>
    </div>
  );
}
