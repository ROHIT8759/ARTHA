"use client";

import EmptyState from "@/components/ui/EmptyState";

export default function SalesPage() {
  return (
    <div>
      <h1>Sales</h1>
      <p className="muted" style={{ marginBottom: "2rem" }}>Manage customer billing and sales</p>
      
      <EmptyState 
        title="Sales not implemented yet" 
        description="This section will allow you to create new bills, scan products, and view past sales. It will be built in the Billing phase."
      />
    </div>
  );
}
