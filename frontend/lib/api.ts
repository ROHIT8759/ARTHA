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
};

/** Money is stored/transported as integer paise; this is the only place
 *  that formats it for display, so the rupee symbol and rounding rule
 *  live in exactly one spot. */
export function formatPaise(paise: number): string {
  return `₹${(paise / 100).toFixed(2)}`;
}
