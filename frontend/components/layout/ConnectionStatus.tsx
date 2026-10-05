"use client";

import { useEffect, useState } from "react";
import { api } from "@/lib/api";
import styles from "./ConnectionStatus.module.css";

export default function ConnectionStatus() {
  const [status, setStatus] = useState<"CONNECTING" | "CONNECTED" | "DISCONNECTED" | "ERROR">("CONNECTING");

  useEffect(() => {
    let mounted = true;
    
    async function checkHealth() {
      if (!mounted) return;
      try {
        await api.health();
        if (mounted) setStatus("CONNECTED");
      } catch {
        if (mounted) setStatus("DISCONNECTED");
      }
    }
    
    checkHealth();
    
    // Poll every 15 seconds so we don't spam the server but stay relatively up to date
    const interval = setInterval(checkHealth, 15000);
    return () => {
      mounted = false;
      clearInterval(interval);
    };
  }, []);

  return (
    <div className={`${styles.statusBadge} ${styles[status.toLowerCase()]}`}>
      <span className={styles.indicator}></span>
      Server: {status}
    </div>
  );
}
