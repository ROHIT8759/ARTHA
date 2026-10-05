"use client";

import EmptyState from "@/components/ui/EmptyState";
import { useAuth } from "@/lib/auth-context";
import ErrorState from "@/components/ui/ErrorState";

export default function SettingsPage() {
  const { user } = useAuth();
  
  if (user?.role !== "owner") {
    return <ErrorState title="Access Denied" message="You do not have permission to view this page. Owner access is required." />;
  }

  return (
    <div>
      <h1>Settings</h1>
      <p className="muted" style={{ marginBottom: "2rem" }}>Manage shop details and staff accounts</p>
      
      <EmptyState 
        title="Settings not implemented yet" 
        description="This section will manage your shop details, staff roles, and backup settings. It will be built in the Settings phase."
      />
    </div>
  );
}
