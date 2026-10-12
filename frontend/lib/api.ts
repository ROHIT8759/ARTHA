// Thin typed client for the ARTHA backend (Rust/Axum, see ../../backend).
//
// The frontend talks to whatever host the browser used to load the page —
// so a staff phone on the shop Wi-Fi that opened http://192.168.1.5:8080/
// keeps talking to 192.168.1.5, with no separate "server URL" to configure.
// NEXT_PUBLIC_API_BASE only overrides this for local dev (frontend on :3000,
// backend on :8080).

const API_BASE =
  process.env.NEXT_PUBLIC_API_BASE ??
  (typeof window !== "undefined" ? window.location.origin.replace(/:\d+$/, ":8080") : "http://localhost:8080");

export class ApiError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

const TOKEN_KEY = "artha_token";

export function getToken(): string | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage.getItem(TOKEN_KEY);
  } catch {
    return null;
  }
}

export function setToken(token: string | null) {
  if (typeof window === "undefined") return;
  try {
    if (token) window.localStorage.setItem(TOKEN_KEY, token);
    else window.localStorage.removeItem(TOKEN_KEY);
  } catch {
    // Storage can throw in private-mode browsers; losing the token just
    // means the user is prompted to log in again, which is an acceptable
    // degradation rather than a crash.
  }
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const token = getToken();
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (token) headers["Authorization"] = `Bearer ${token}`;

  const res = await fetch(`${API_BASE}${path}`, { ...init, headers: { ...headers, ...(init?.headers as Record<string, string>) } });
  const text = await res.text();
  const data = text ? JSON.parse(text) : null;

  if (!res.ok) {
    const message = (data && (data.message as string)) || res.statusText;
    throw new ApiError(res.status, message);
  }
  return data as T;
}

export type Role = "owner" | "staff";

export interface UserView {
  user_id: string;
  username: string;
  role: Role;
  status: string;
  created_at: string;
}

export interface LoginResponse {
  token: string;
  user: UserView;
  expires_at: string;
}

export interface ProductView {
  product_id: string;
  name: string;
  batch: string | null;
  hsn_code: string | null;
  gst_rate_bps: number;
  qr_code: string;
  price_paise: number;
  cost_paise: number;
  stock_qty_milli: number;
  unit: string;
  status: string;
  created_at: string;
  updated_at: string;
}

export const api = {
  health: () => request<{ status: string; version: string }>("/api/health"),

  setupStatus: () => request<{ exists: boolean }>("/api/setup"),

  setupBusiness: (body: { business_name: string; owner_username: string; owner_password: string }) =>
    request<LoginResponse>("/api/setup", { method: "POST", body: JSON.stringify(body) }),

  login: (body: { username: string; password: string }) =>
    request<LoginResponse>("/api/auth/login", { method: "POST", body: JSON.stringify(body) }),

  logout: () => request<{ status: string }>("/api/auth/logout", { method: "POST" }),

  me: () => request<UserView>("/api/auth/me"),

  listUsers: () => request<UserView[]>("/api/users"),

  createStaff: (body: { username: string; password: string }) =>
    request<UserView>("/api/users", { method: "POST", body: JSON.stringify(body) }),

  setUserStatus: (userId: string, status: "active" | "disabled") =>
    request<{ status: string }>(`/api/users/${userId}`, { method: "PATCH", body: JSON.stringify({ status }) }),

  listProducts: (search?: string) => 
    request<ProductView[]>(search ? `/api/products?search=${encodeURIComponent(search)}` : "/api/products"),

  createProduct: (body: {
    name: string;
    batch?: string;
    hsn_code?: string;
    gst_rate_bps: number;
    qr_code: string;
    price_paise: number;
    cost_paise: number;
    unit?: string;
  }) => request<ProductView>("/api/products", { method: "POST", body: JSON.stringify(body) }),

  getProduct: (productId: string) => request<ProductView>(`/api/products/${productId}`),

  updateProduct: (productId: string, body: Partial<{
    name: string;
    batch: string;
    hsn_code: string;
    gst_rate_bps: number;
    qr_code: string;
    price_paise: number;
    cost_paise: number;
    unit: string;
    status: "active" | "archived";
  }>) => request<ProductView>(`/api/products/${productId}`, { method: "PATCH", body: JSON.stringify(body) }),

  getProductByQr: (qrCode: string) => request<ProductView>(`/api/products/by-qr/${encodeURIComponent(qrCode)}`),

  // --- INVENTORY ---
  getInventoryStatus: (productId: string) => 
    request<{ product_id: string; current_stock_milli: number; status: string }>(`/api/inventory/${productId}`),
    
  getInventoryHistory: (productId: string) => 
    request<Array<{
      transaction_id: string;
      product_id: string;
      transaction_type: string;
      quantity_milli: number;
      reference_id?: string;
      reason?: string;
      created_by: string;
      created_at: string;
    }>>(`/api/inventory/${productId}/history`),
    
  postOpeningStock: (productId: string, quantityMilli: number) => 
    request<{ status: string }>("/api/inventory/opening", { method: "POST", body: JSON.stringify({ product_id: productId, quantity_milli: quantityMilli }) }),
    
  postAdjustment: (productId: string, quantityMilli: number, reason: string) => 
    request<{ status: string }>("/api/inventory/adjust", { method: "POST", body: JSON.stringify({ product_id: productId, quantity_milli: quantityMilli, reason }) }),

  // --- SUPPLIERS ---
  listSuppliers: () => request<Array<{ supplier_id: string; name: string; contact?: string; created_at: string }>>("/api/suppliers"),
  createSupplier: (name: string, contact?: string) => 
    request<{ supplier_id: string; name: string; contact?: string; created_at: string }>("/api/suppliers", { method: "POST", body: JSON.stringify({ name, contact }) }),

  // --- PURCHASES ---
  listPurchases: () => request<Array<{ purchase_id: string; supplier_id?: string; invoice_no?: string; invoice_date?: string; status: string; source: string; subtotal_paise: number; tax_total_paise: number; total_paise: number; created_by: string; created_at: string }>>("/api/purchases"),
  getPurchase: (purchaseId: string) => request<{
    purchase_id: string; supplier_id?: string; invoice_no?: string; invoice_date?: string; status: string; source: string; subtotal_paise: number; tax_total_paise: number; total_paise: number; created_by: string; created_at: string;
    items: Array<{
      purchase_item_id: string;
      product_id?: string;
      product_name_snapshot?: string;
      batch?: string;
      hsn_code?: string;
      gst_rate_bps: number;
      quantity_milli: number;
      price_paise: number;
      line_total_paise: number;
    }>;
  }>(`/api/purchases/${purchaseId}`),
  createPurchase: (source: "manual" | "ocr") => request<{ purchase_id: string }>("/api/purchases", { method: "POST", body: JSON.stringify({ source }) }),
  updatePurchase: (purchaseId: string, payload: unknown) => request<unknown>(`/api/purchases/${purchaseId}`, { method: "PATCH", body: JSON.stringify(payload) }),
  approvePurchase: (purchaseId: string) => request<{ status: string }>(`/api/purchases/${purchaseId}/approve`, { method: "POST" }),
  cancelPurchase: (purchaseId: string) => request<{ status: string }>(`/api/purchases/${purchaseId}/cancel`, { method: "POST" }),
  
  uploadBillImage: async (purchaseId: string, file: File) => {
    const formData = new FormData();
    formData.append("file", file);
    const token = localStorage.getItem("token");
    const res = await fetch(`/api/purchases/${purchaseId}/image`, {
      method: "POST",
      headers: { "Authorization": `Bearer ${token}` },
      body: formData
    });
    if (!res.ok) throw new Error("Upload failed");
    return res.json() as Promise<{ image_id: string }>;
  },
  
  processOcr: (purchaseId: string, imageId: string) => request<{ parsed?: { invoice_no?: string; items?: Array<Record<string, unknown>> }, status: string }>(`/api/purchases/${purchaseId}/ocr`, { method: "POST", body: JSON.stringify({ image_id: imageId }) }),

  // --- SALES & BILLING ---
  listSales: (query?: { status?: string; search?: string; limit?: number }) => {
    const params = new URLSearchParams();
    if (query?.status) params.set("status", query.status);
    if (query?.search) params.set("search", query.search);
    if (query?.limit) params.set("limit", query.limit.toString());
    const qs = params.toString();
    return request<SaleView[]>(qs ? `/api/sales?${qs}` : "/api/sales");
  },

  getSale: (saleId: string) => request<SaleView>(`/api/sales/${saleId}`),

  createSale: (body?: {
    customer_name?: string;
    customer_phone?: string;
    customer_email?: string;
    customer_address?: string;
    customer_gstin?: string;
    payment_method?: string;
    notes?: string;
  }) => request<SaleView>("/api/sales", { method: "POST", body: JSON.stringify(body || {}) }),

  updateSale: (
    saleId: string,
    body: {
      customer_name?: string;
      customer_phone?: string;
      customer_email?: string;
      customer_address?: string;
      customer_gstin?: string;
      payment_method?: string;
      discount_paise?: number;
      notes?: string;
      items?: Array<{
        sale_item_id?: string;
        product_id: string;
        quantity_milli: number;
        unit_price_paise?: number;
        discount_paise?: number;
      }>;
    }
  ) => request<SaleView>(`/api/sales/${saleId}`, { method: "PATCH", body: JSON.stringify(body) }),

  finalizeSale: (saleId: string, body?: { payment_method?: string; amount_paise?: number }) =>
    request<SaleView>(`/api/sales/${saleId}/finalize`, { method: "POST", body: JSON.stringify(body || {}) }),

  voidSale: (saleId: string, reason?: string) =>
    request<SaleView>(`/api/sales/${saleId}/void`, { method: "POST", body: JSON.stringify({ reason }) }),

  // --- BUSINESS SETTINGS & PROFILE ---
  getBusiness: () => request<BusinessView>("/api/business"),

  updateBusiness: (body: {
    name?: string;
    registered_name?: string;
    address?: string;
    city?: string;
    state?: string;
    pincode?: string;
    country?: string;
    phone?: string;
    email?: string;
    gstin?: string;
    invoice_footer?: string;
    terms_and_conditions?: string;
    default_layout?: "a4" | "thermal";
    show_hsn?: boolean;
    show_tax_breakdown?: boolean;
    show_logo?: boolean;
  }) => request<BusinessView>("/api/business", { method: "PATCH", body: JSON.stringify(body) }),

  uploadLogo: (logoDataUrl: string) =>
    request<BusinessView>("/api/business/logo", { method: "POST", body: JSON.stringify({ logo_data_url: logoDataUrl }) }),

  removeLogo: () =>
    request<BusinessView>("/api/business/logo", { method: "DELETE" }),
};

export interface SaleItemView {
  sale_item_id: string;
  product_id: string;
  product_name_snapshot: string;
  batch_snapshot: string | null;
  hsn_code_snapshot: string | null;
  gst_rate_bps: number;
  quantity_milli: number;
  unit_price_paise: number;
  discount_paise: number;
  tax_paise: number;
  line_total_paise: number;
}

export interface PaymentView {
  payment_id: string;
  payment_method: string;
  amount_paise: number;
  status: string;
  created_at: string;
}

export interface SaleView {
  sale_id: string;
  business_id: string;
  invoice_no: string | null;
  customer_name: string | null;
  customer_phone: string | null;
  customer_email: string | null;
  customer_address: string | null;
  customer_gstin: string | null;
  status: "draft" | "completed" | "void";
  subtotal_paise: number;
  discount_paise: number;
  tax_total_paise: number;
  grand_total_paise: number;
  payment_method: string;
  payment_status: string;
  notes: string | null;
  created_by: string;
  finalized_by: string | null;
  created_at: string;
  updated_at: string;
  finalized_at: string | null;
  items: SaleItemView[];
  payments: PaymentView[];
}

export interface BusinessSettingsView {
  registered_name?: string | null;
  address?: string | null;
  city?: string | null;
  state?: string | null;
  pincode?: string | null;
  country?: string | null;
  phone?: string | null;
  email?: string | null;
  gstin?: string | null;
  logo_data_url?: string | null;
  invoice_footer?: string | null;
  terms_and_conditions?: string | null;
  default_layout?: "a4" | "thermal" | null;
  show_hsn?: boolean | null;
  show_tax_breakdown?: boolean | null;
  show_logo?: boolean | null;
}

export interface BusinessView {
  business_id: string;
  name: string;
  settings: BusinessSettingsView;
  created_at: string;
  updated_at: string;
}

/** Formats integer paise with Indian numbering grouping (e.g. ₹1,23,456.78) */
export function formatPaise(paise: number): string {
  const isNegative = paise < 0;
  const absPaise = Math.abs(paise);
  const rupees = Math.floor(absPaise / 100);
  const remPaise = absPaise % 100;
  const paiseStr = remPaise.toString().padStart(2, "0");

  const s = rupees.toString();
  let result = "";
  if (s.length <= 3) {
    result = s;
  } else {
    const last3 = s.slice(-3);
    const rest = s.slice(0, -3);
    const parts = [];
    for (let i = rest.length; i > 0; i -= 2) {
      parts.unshift(rest.slice(Math.max(0, i - 2), i));
    }
    result = parts.join(",") + "," + last3;
  }

  return `${isNegative ? "-" : ""}₹${result}.${paiseStr}`;
}

const ONES = [
  "", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine",
  "Ten", "Eleven", "Twelve", "Thirteen", "Fourteen", "Fifteen", "Sixteen",
  "Seventeen", "Eighteen", "Nineteen",
];
const TENS = ["", "", "Twenty", "Thirty", "Forty", "Fifty", "Sixty", "Seventy", "Eighty", "Ninety"];

function numToWordsUnder1000(n: number): string {
  let str = "";
  if (n >= 100) {
    str += ONES[Math.floor(n / 100)] + " Hundred ";
    n %= 100;
  }
  if (n >= 20) {
    str += TENS[Math.floor(n / 10)] + " ";
    n %= 10;
  }
  if (n > 0) {
    str += ONES[n] + " ";
  }
  return str.trim();
}

/** Converts integer paise to Indian English words deterministically */
export function amountToWords(paise: number): string {
  if (paise === 0) return "Rupees Zero Only";
  const absPaise = Math.abs(paise);
  let rupees = Math.floor(absPaise / 100);
  const remPaise = absPaise % 100;

  let words = "";

  if (rupees > 0) {
    const crore = Math.floor(rupees / 10000000);
    rupees %= 10000000;
    const lakh = Math.floor(rupees / 100000);
    rupees %= 100000;
    const thousand = Math.floor(rupees / 1000);
    rupees %= 1000;
    const remainder = rupees;

    if (crore > 0) {
      words += numToWordsUnder1000(crore) + " Crore ";
    }
    if (lakh > 0) {
      words += numToWordsUnder1000(lakh) + " Lakh ";
    }
    if (thousand > 0) {
      words += numToWordsUnder1000(thousand) + " Thousand ";
    }
    if (remainder > 0) {
      words += numToWordsUnder1000(remainder) + " ";
    }
    words = "Rupees " + words.trim();
  }

  if (remPaise > 0) {
    const paiseWords = numToWordsUnder1000(remPaise) + " Paise";
    if (words.length > 0) {
      words += " and " + paiseWords;
    } else {
      words = paiseWords;
    }
  }

  return (paise < 0 ? "Minus " : "") + words.trim() + " Only";
}

export interface TaxSlabBreakdown {
  rateBps: number;
  taxablePaise: number;
  taxPaise: number;
}

/**
 * Derives tax slabs grouped by gst_rate_bps using authoritative integer arithmetic.
 * Derivation identity: taxable = line_total_paise - tax_paise (exact integer paise, no float rounding errors).
 */
export function computeTaxSlabs(
  items: Array<{ gst_rate_bps: number; line_total_paise: number; tax_paise: number }>
): Record<number, TaxSlabBreakdown> {
  const slabs: Record<number, TaxSlabBreakdown> = {};
  for (const item of items || []) {
    const lineTax = item.tax_paise ?? 0;
    const taxable = Math.max(0, (item.line_total_paise ?? 0) - lineTax);
    if (!slabs[item.gst_rate_bps]) {
      slabs[item.gst_rate_bps] = {
        rateBps: item.gst_rate_bps,
        taxablePaise: 0,
        taxPaise: 0,
      };
    }
    slabs[item.gst_rate_bps].taxablePaise += taxable;
    slabs[item.gst_rate_bps].taxPaise += lineTax;
  }
  return slabs;
}

/**
 * Calculates item subtotal in paise from quantity in milli-units and unit price in paise.
 * Uses integer rounding: Math.floor((quantityMilli * unitPricePaise + 500) / 1000).
 */
export function calculateItemSubtotalPaise(quantityMilli: number, unitPricePaise: number): number {
  return Math.floor((quantityMilli * unitPricePaise + 500) / 1000);
}

/**
 * Calculates line tax in paise from taxable amount and GST rate in basis points (bps).
 * Uses integer rounding: Math.floor((taxablePaise * gstRateBps + 5000) / 10000).
 */
export function calculateLineTaxPaise(taxablePaise: number, gstRateBps: number): number {
  return Math.floor((taxablePaise * gstRateBps + 5000) / 10000);
}

/**
 * Formats GST rate in basis points (bps) to percentage string.
 * Preserves integer percentages without decimal places (e.g., 1800 -> "18%"),
 * while correctly formatting fractional percentages (e.g., 825 -> "8.25%").
 */
export function formatGstRate(rateBps: number): string {
  const isWhole = rateBps % 100 === 0;
  return `${isWhole ? (rateBps / 100).toFixed(0) : (rateBps / 100).toFixed(2)}%`;
}

