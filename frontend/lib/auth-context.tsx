"use client";

import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import { api, getToken, setToken, type UserView } from "./api";

interface AuthState {
  user: UserView | null;
  loading: boolean;
  /** True once we've checked the server for first-run setup status. */
  businessExists: boolean | null;
  login: (username: string, password: string) => Promise<void>;
  setupBusiness: (businessName: string, ownerUsername: string, ownerPassword: string) => Promise<void>;
  logout: () => Promise<void>;
}

const AuthContext = createContext<AuthState | null>(null);

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<UserView | null>(null);
  const [loading, setLoading] = useState(true);
  const [businessExists, setBusinessExists] = useState<boolean | null>(null);

  useEffect(() => {
    // We don't persist the user object, only the token; on reload, the
    // simplest "is this token still good" check is a real API call. V0 has
    // no cheap /me endpoint, so we probe with listUsers (owner) or
    // listProducts (any role) and fall back to "logged out" on failure.
    (async () => {
      const token = getToken();
      if (!token) {
        setLoading(false);
        return;
      }
      try {
        const me = await api.me();
        setUser(me);
      } catch {
        setToken(null);
      } finally {
        setLoading(false);
      }
    })();
  }, []);

  const login = useCallback(async (username: string, password: string) => {
    const res = await api.login({ username, password });
    setToken(res.token);
    setUser(res.user);
  }, []);

  const setupBusiness = useCallback(async (businessName: string, ownerUsername: string, ownerPassword: string) => {
    const res = await api.setupBusiness({ business_name: businessName, owner_username: ownerUsername, owner_password: ownerPassword });
    setToken(res.token);
    setUser(res.user);
    setBusinessExists(true);
  }, []);

  const logout = useCallback(async () => {
    try {
      await api.logout();
    } catch {
      // Already invalid/expired — fine, we're clearing it locally anyway.
    }
    setToken(null);
    setUser(null);
  }, []);

  return (
    <AuthContext.Provider value={{ user, loading, businessExists, login, setupBusiness, logout }}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth(): AuthState {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error("useAuth must be used within an AuthProvider");
  return ctx;
}
