"use client";

import EmptyState from "@/components/ui/EmptyState";

export default function SettingsPage() {
  return (
    <div>
      <h1>Settings</h1>
      <p className="muted" style={{ marginBottom: "2rem" }}>Manage shop details and staff accounts</p>
      
      <EmptyState 
        title="Settings not implemented yet" 
        description="This section will manage your shop details, staff roles, and backup settings. It will be built in the Authentication and Settings phases."
      />
    </div>
  );
}
