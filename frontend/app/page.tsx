"use client";

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { api } from "@/lib/api";
import { useAuth } from "@/lib/auth-context";

/** Pure traffic director: first-run setup, then login, then dashboard.
 *  No UI of its own, so it never flashes the wrong screen while deciding. */
export default function Home() {
  const router = useRouter();
  const { user, loading } = useAuth();
  const [checking, setChecking] = useState(true);

  useEffect(() => {
    if (loading) return;
    if (user) {
      router.replace("/dashboard");
      return;
    }
    (async () => {
      try {
        const { exists } = await api.setupStatus();
        router.replace(exists ? "/login" : "/setup");
      } catch {
        // Backend unreachable (e.g. server not started yet) — land on login,
        // which will surface a clear connection error instead of a blank page.
        router.replace("/login");
      } finally {
        setChecking(false);
      }
    })();
  }, [loading, user, router]);

  return (
    <main style={{ display: "flex", minHeight: "100vh", alignItems: "center", justifyContent: "center" }}>
      <p>{checking || loading ? "Loading…" : null}</p>
    </main>
  );
}
