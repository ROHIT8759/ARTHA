"use client";

import EmptyState from "@/components/ui/EmptyState";

export default function DashboardPage() {
  return (
    <div>
      <h1>Dashboard</h1>
      <p className="muted" style={{ marginBottom: "2rem" }}>Overview of your business</p>
      
      <EmptyState 
        title="Dashboard not implemented yet" 
        description="This section will show business metrics, recent sales, and low stock alerts. It will be built in a future phase."
      />
    </div>
  );
}
