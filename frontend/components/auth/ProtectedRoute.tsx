"use client";

import { useEffect } from "react";
import { useRouter } from "next/navigation";
import { useAuth } from "@/lib/auth-context";
import LoadingState from "@/components/ui/LoadingState";

export default function ProtectedRoute({ children }: { children: React.ReactNode }) {
  const { user, loading, businessExists } = useAuth();
  const router = useRouter();

  useEffect(() => {
    if (loading) return;
    
    // If not authenticated, we need to decide where to send them
    if (!user) {
      if (businessExists === false) {
        router.replace("/setup");
      } else {
        router.replace("/login");
      }
    }
  }, [user, loading, businessExists, router]);

  // While loading or if unauthenticated (before the redirect happens), show a loading state
  if (loading || !user) {
    return <LoadingState fullPage message="Authenticating..." />;
  }

  return <>{children}</>;
}
