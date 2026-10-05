# Offline Billing & Inventory Platform (ARTHA)

Full Product, Architecture & Versioned Phase Plan — working specification, V0 → V1 → V2 → V3.
Local-first • Offline-capable • Multi-user • QR billing • OCR-assisted purchase entry

## 1. Product Vision

Build a free, local-first billing and inventory system for small businesses. The owner's PC acts as the central local server and database. Staff connect through a browser over the shop's local network, log in with individual accounts, manage products, record purchases, scan product QR codes during sales, and generate bills. Internet access is not required for core operations.

Core principles:

- **Offline-first:** billing and inventory must continue without internet.
- **Local ownership:** business data remains on the owner's PC.
- **Simple UX:** staff should be able to scan, review, approve, and bill with minimal clicks.
- **Reliability first:** autosave, crash recovery, backups, restore, and safe updates are core features.
- **Human verification:** OCR pre-fills data; staff approves or edits before committing it.
- **Version everything:** application, database schema, OCR/model package, and update package.
- **No mandatory third-party cloud/API dependency.**

## 2. Target Users & Roles

- **Owner/Admin:** creates the business, manages staff, products, settings, backups, updates, and reports.
- **Staff:** logs in, adds/edits permitted products, records purchases, scans QR codes, creates sales, and completes billing.
- Future roles can be added without redesigning the authorization model.

## 3. High-Level Architecture

- **Owner PC:** runs the local application server, database, OCR service, backup manager, and update manager.
- **Staff devices:** phones/PCs/tablets use a browser over the shop LAN.
- **Frontend:** Next.js + TypeScript.
- **Backend:** Rust REST API.
- **Database:** SQLite as the primary embedded database; Excel is export-only, not the system of record.
- **OCR:** PaddleOCR running locally, isolated from billing/business logic.
- **Packaging:** local application bundle/containerized services where useful; Docker may package the OCR service.
- **Storage:** structured application data in SQLite; uploaded bill images in controlled local storage; backups as versioned snapshots.
- **Network:** local Wi-Fi/LAN only for normal operations; internet is optional for updates.

## 4. Data Model — Core Entities

| Entity | Purpose | Important fields |
|---|---|---|
| Business | Business/tenant boundary | business_id, name, settings |
| User | Owner/staff account | user_id, username, password_hash, role, status |
| Product | Catalog + inventory identity | product_id, name, batch, HSN, GST, QR code, pricing |
| Supplier | Purchase source | supplier_id, name, contact |
| Purchase | Inbound transaction | purchase_id, supplier, invoice_no, date, totals, status |
| PurchaseItem | Line item | product, quantity, batch, GST, HSN, price, QR |
| Sale | Outbound transaction | sale_id, customer, invoice_no, date, totals |
| SaleItem | Sold product line | product, quantity, rate, tax, discount |
| BillImage | Original photographed bill | image_id, purchase_id, path, hash, timestamp |
| OCRResult | Extracted OCR data | ocr_id, image_id, text, boxes, confidence |
| Backup | Recovery point | backup_id, version, timestamp, checksum, schema_version |
| AppVersion/ModelVersion | Safe updates | version, package_hash, release metadata |

## 5. Version Roadmap

### V0 — Foundation / Internal Prototype

- Architecture and requirements freeze
- Rust API skeleton and Next.js/TypeScript UI
- SQLite schema and migrations
- Local server startup/shutdown
- Owner account creation
- Basic staff authentication and roles
- Product CRUD
- Unique internal product ID + QR code field
- Local LAN browser access
- Basic purchase and sales data structures
- Audit-friendly transaction IDs
- Basic logging and error handling

### V0.1 — Core Billing

- Fast sales screen
- QR scan → product lookup
- Cart/line-item editing
- Quantity, price, GST/tax, discount and totals
- Bill generation/printing-ready layout
- Stock decrement on approved sale
- Purchase entry and stock increment
- Manual product selection/search
- Validation and duplicate transaction protection

### V0.2 — Purchase Photo/OCR

- Take/upload purchase-bill photo from staff device
- Store original image locally
- Local PaddleOCR pipeline
- OCR text + bounding boxes + confidence
- Bill parser maps OCR output to structured fields
- Purchase table prefilled from OCR
- Low-confidence/unknown fields visibly flagged
- Staff edits all extracted values
- Approve → validate → save
- No automatic training from user corrections

### V0.3 — Reliability

- Autosave of in-progress billing
- Crash/power-loss recovery
- Resume unfinished bill after restart
- Atomic database transactions
- Safe file writes
- Hourly local backups
- Separate backup storage location
- Timestamped/versioned backups
- Checksum/integrity validation
- Restore and rollback flow
- Configurable retention, e.g. last N backups

### V1 — Production Small-Business Release

- Complete owner/admin console
- Staff management and permissions
- Products, suppliers, purchases, sales and inventory
- QR-first billing workflow
- Purchase OCR workflow
- Reports and dashboard
- Search/filter/export
- Excel/CSV export only
- Backup/restore UI
- Operational logs
- LAN discovery/help screen
- Production error handling
- Security hardening
- Database migration system
- Installer/packaging
- Documentation and recovery guide

### V1.1 — Updates

- One-click Update button
- Pre-update backup
- Package signature/hash verification
- Version compatibility checks
- Database migration execution
- Safe install with rollback
- Separate OCR/model package version
- Optional internet update check
- Offline update package via USB/local file
- Update status and failure recovery

### V2 — OCR Improvement

- Correction dataset capture with explicit business/user consent
- Store OCR errors and corrected target fields
- Dataset cleaning and deduplication
- Bill layout/category grouping
- Evaluation dataset and accuracy metrics
- Train candidate parser/model offline
- Compare candidate vs current version
- Promote only after validation
- Versioned model releases
- Rollback to previous OCR/model version

### V3 — Advanced Operations

- More detailed reporting
- Advanced permissions/audit trails
- Improved backup rotation
- Optional controlled multi-location/sync architecture
- Optional centralized model improvement pipeline
- Optional encrypted backup/export
- Operational diagnostics and support bundle

## 6. Detailed Functional Workflows

### 6.1 Staff Login

Staff opens the local address in a browser → enters ID/password → backend validates credentials → role permissions are loaded → dashboard opens.

### 6.2 Add Product

Staff/owner enters product name, batch, HSN, GST, price and other required information → assigns/scans product QR → system validates uniqueness/identity → product is saved.

### 6.3 Sale / Billing

1. Open New Sale.
2. Scan product QR code.
3. Backend resolves QR to product.
4. Product is added to bill automatically.
5. Repeat for all products.
6. Edit quantity/price/discount/tax if permitted.
7. Validate stock and totals.
8. Approve bill.
9. Create immutable sale transaction.
10. Decrease stock atomically.
11. Generate/print bill.

### 6.4 Purchase — Manual

Open Purchase → Manual Entry → add supplier/invoice metadata → add product rows → validate → approve → stock increases atomically.

### 6.5 Purchase — Photo/OCR

1. Open Purchase → Take Photo.
2. Capture/select bill image.
3. Save original image locally.
4. Run local PaddleOCR.
5. Receive text, coordinates and confidence.
6. Parser attempts to identify invoice number, date, supplier, product rows, quantities, prices, GST/HSN and totals.
7. Show prefilled purchase table.
8. Highlight uncertain or missing values.
9. Staff checks and edits.
10. Staff approves.
11. Backend validates all required fields.
12. Commit purchase and inventory transaction atomically.

## 7. OCR Architecture

PaddleOCR is the initial local OCR engine. Do not train a custom OCR model in V0. Separate OCR from the Rust business backend so the OCR engine can be replaced or upgraded independently.

Image capture → preprocessing → PaddleOCR → OCR text/boxes/confidence → bill parser → validation → human review → approval → database.

- Preprocessing can include rotation correction, cropping, resolution normalization, contrast/thresholding and noise reduction.
- The parser should use deterministic rules first: labels, table geometry, numeric patterns, GST/HSN patterns, totals and invoice-number patterns.
- OCR confidence is not business validation. A high-confidence OCR value can still be wrong.
- Never let OCR directly commit a purchase without staff approval.

## 8. OCR Learning / Training Strategy

The product must not automatically retrain production OCR/model weights from every correction. Corrections may be collected as training/evaluation data, but a new version is trained offline, evaluated, versioned, and deliberately promoted.

1. Capture corrected fields and the original OCR result.
2. Keep original images where legally and operationally appropriate.
3. Create a clean labeled dataset.
4. Split data into training/validation/test sets.
5. Measure field-level accuracy and full-bill success rate.
6. Compare candidate model/parser against the current production version.
7. Release only if it passes predefined thresholds.
8. Keep rollback to the previous model.

## 9. Reliability, Autosave & Recovery

- Persist important transaction state to SQLite rather than relying on browser memory.
- Use atomic transactions for stock + bill changes.
- Use an explicit draft state for unfinished bills.
- Autosave drafts frequently and on important actions.
- On restart, show recoverable drafts.
- Use durable file/database writes and integrity checks.
- Create scheduled backups, initially hourly.
- Never overwrite the only backup.
- Use timestamped backups and configurable retention.
- Provide Restore Backup and Verify Backup actions.

## 10. Backup & Restore Design

SQLite is the source of truth. Excel is an interoperability/export format, not the primary storage format.

- **Primary DB:** live SQLite database.
- **Backup:** consistent SQLite backup/snapshot plus required local document/image assets.
- **Backup metadata:** timestamp, application version, schema version, backup ID and checksum.
- **Recommended retention:** daily/weekly rotation in addition to hourly backups as the product matures.
- Restore must require confirmation and create a safety backup of the current state first.
- After restore, run integrity checks and schema compatibility checks.

## 11. Security Architecture

- Password hashes only; never store plaintext passwords.
- Role-based authorization enforced by the Rust backend, not only the UI.
- Owner-only access to staff administration, backups, restores and updates.
- Validate every API input server-side.
- Prevent unauthorized LAN access where possible with OS firewall guidance and configurable bind address.
- Use session/token expiry and revocation.
- Audit sensitive actions.
- Protect backup and application directories using OS permissions.
- Do not expose the local server to the public internet by default.
- Security hardening is a V1 production gate.

## 12. Local Network Architecture

Owner PC hosts the service. Staff devices connect over the shop's private Wi-Fi/LAN. The application should display the local URL and connection status. Core billing must continue without WAN/internet access.

## 13. Update Manager

User sees Update Now.

1. System checks compatibility and available package.
2. Create verified backup.
3. Verify package signature/hash.
4. Stop/coordinate services safely.
5. Apply application and/or OCR package update.
6. Run database migrations.
7. Run health checks.
8. Restart services.
9. If validation fails, roll back.
10. Record update result.

Two modes: optional online update check/download, and fully offline update using a local package/USB.

## 14. API / Module Boundaries

- **Auth module** — users, sessions, roles.
- **Products module** — catalog and QR identity.
- **Inventory module** — stock movements, not merely a mutable stock number.
- **Purchases module** — purchase headers/items and approval.
- **Sales module** — bills, line items and stock deduction.
- **OCR module** — image processing and OCR job lifecycle.
- **Parser module** — OCR-to-business-field mapping.
- **Backup module** — create, verify, list, restore.
- **Update module** — package verification, migration, rollback.
- **Reporting module** — read-only aggregation/export.
- **Audit module** — sensitive operation history.

## 15. Testing Strategy

- Unit tests for pricing, GST/tax calculations, stock calculations and parsers.
- API tests for authorization and validation.
- Database migration tests.
- OCR parser fixture tests using representative bills.
- End-to-end tests for purchase, approval, sale and stock movement.
- Crash-recovery tests during draft billing.
- Backup/restore tests.
- Update/rollback tests.
- LAN connectivity tests.
- Load tests with many staff requests on a realistic shop PC.

## 16. Production Acceptance Criteria

- No internet required for core login, inventory, purchase and sales workflows.
- A QR scan reliably identifies the intended product.
- A completed sale changes inventory exactly once.
- A purchase changes inventory exactly once.
- OCR never silently commits unverified data.
- An unexpected restart does not lose an approved transaction.
- Unfinished drafts can be recovered.
- Backups can be verified and restored.
- Updates preserve data through tested migrations.
- Owner and staff permissions are enforced server-side.
- Application can be operated by a small-business user without technical file/database knowledge.

## 17. Explicit Non-Goals for V0

- No mandatory cloud backend.
- No mandatory external OCR API.
- No automatic production-model retraining.
- No dependence on Excel as the primary database.
- No public internet exposure by default.
- No complex multi-branch synchronization until the local single-business product is stable.
- No unnecessary technology additions merely to make the stack look impressive.

## 18. Recommended Build Order

1. Freeze requirements and data model
2. Build Rust + SQLite foundation
3. Build Next.js/TypeScript shell
4. Authentication and RBAC
5. Product + QR management
6. Inventory transaction engine
7. Sales/billing workflow
8. Purchase/manual workflow
9. Autosave and crash recovery
10. Backup/restore
11. Local OCR + parser + review UI
12. Reports/export
13. Security hardening
14. Packaging/installer
15. Update manager
16. OCR correction/evaluation pipeline
17. Production testing and release

## 19. Key Product Decisions

| Decision | Choice |
|---|---|
| Primary storage | SQLite |
| Primary backend | Rust |
| Frontend | Next.js + TypeScript |
| Local OCR | PaddleOCR |
| Primary data exchange | REST APIs |
| Business data location | Owner PC |
| Staff access | Browser over private LAN |
| Primary export | Excel/CSV |
| Core operation | Offline |
| OCR approval | Human-in-the-loop |
| Model improvement | Controlled/versioned, not automatic production retraining |
| Backups | Automatic local versioned backups |
| Updates | One-click, verified, backup-first, rollback-capable |

## 20. Final Product Concept

A small business installs the application on the owner's PC. The PC becomes the local source of truth. Staff connect through a browser on the same network, log in individually, scan QR codes to sell products, and use a camera-based purchase workflow to capture invoices. Local OCR pre-fills purchase information, staff verifies it, and the system commits inventory safely. Autosave and recovery protect active work, scheduled backups protect historical data, and a verified one-click updater keeps the application and OCR components current. The architecture stays local-first and dependency-conscious while leaving room for future OCR improvements and optional online update distribution.

## 21. Development Rule

Do not add technology just because it is available. Every component must have a clear reason, remain replaceable where practical, and support the core goals: offline operation, reliability, simplicity, security, maintainability, and zero mandatory cloud/API cost.

