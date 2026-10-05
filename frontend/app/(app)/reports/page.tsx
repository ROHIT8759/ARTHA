"use client";

import EmptyState from "@/components/ui/EmptyState";

export default function ReportsPage() {
  return (
    <div>
      <h1>Reports</h1>
      <p className="muted" style={{ marginBottom: "2rem" }}>View business analytics and tax summaries</p>
      
      <EmptyState 
        title="Reports not implemented yet" 
        description="This section will generate sales reports and GST summaries. It will be built in a future phase."
      />
    </div>
  );
}
