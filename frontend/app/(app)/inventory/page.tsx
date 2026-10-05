"use client";

import EmptyState from "@/components/ui/EmptyState";

export default function InventoryPage() {
  return (
    <div>
      <h1>Inventory</h1>
      <p className="muted" style={{ marginBottom: "2rem" }}>Track stock levels across the shop</p>
      
      <EmptyState 
        title="Inventory not implemented yet" 
        description="This section will show detailed stock levels and inventory history. It will be built in the Inventory phase."
      />
    </div>
  );
}
