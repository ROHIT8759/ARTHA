"use client";

import EmptyState from "@/components/ui/EmptyState";

export default function ProductsPage() {
  return (
    <div>
      <h1>Products</h1>
      <p className="muted" style={{ marginBottom: "2rem" }}>Manage product catalog and pricing</p>
      
      <EmptyState 
        title="Products not implemented yet" 
        description="This section will manage your product catalog, prices, and GST rates. It will be built in the Inventory phase."
      />
    </div>
  );
}
