-- Update Purchases Table
ALTER TABLE purchases ADD COLUMN source TEXT NOT NULL DEFAULT 'manual';
ALTER TABLE purchases ADD COLUMN subtotal_paise INTEGER NOT NULL DEFAULT 0;
ALTER TABLE purchases ADD COLUMN tax_total_paise INTEGER NOT NULL DEFAULT 0;

-- Recreate purchase_items to allow unmatched products during draft/OCR states
CREATE TABLE purchase_items_new (
    purchase_item_id TEXT PRIMARY KEY,
    purchase_id      TEXT NOT NULL REFERENCES purchases(purchase_id),
    product_id       TEXT REFERENCES products(product_id),
    product_name_snapshot TEXT,
    batch            TEXT,
    hsn_code         TEXT,
    gst_rate_bps     INTEGER NOT NULL DEFAULT 0,
    quantity_milli   INTEGER NOT NULL,
    price_paise      INTEGER NOT NULL,
    line_total_paise INTEGER NOT NULL
);

INSERT INTO purchase_items_new 
SELECT purchase_item_id, purchase_id, product_id, NULL, batch, hsn_code, gst_rate_bps, quantity_milli, price_paise, line_total_paise 
FROM purchase_items;

DROP TABLE purchase_items;
ALTER TABLE purchase_items_new RENAME TO purchase_items;
CREATE INDEX idx_purchase_items_purchase ON purchase_items(purchase_id);
