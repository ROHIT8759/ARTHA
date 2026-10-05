"use client";

import { useState } from "react";
import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { useAuth } from "@/lib/auth-context";
import ConnectionStatus from "./ConnectionStatus";
import styles from "./Shell.module.css";

const NAV_LINKS = [
  { href: "/dashboard", label: "Dashboard", roles: ["owner", "staff"] },
  { href: "/sales", label: "Sales", roles: ["owner", "staff"] },
  { href: "/purchases", label: "Purchases", roles: ["owner", "staff"] },
  { href: "/products", label: "Products", roles: ["owner", "staff"] },
  { href: "/inventory", label: "Inventory", roles: ["owner", "staff"] },
  { href: "/reports", label: "Reports", roles: ["owner", "staff"] },
  { href: "/settings", label: "Settings", roles: ["owner"] },
];

export default function Shell({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();
  const router = useRouter();
  const { user, logout } = useAuth();
  const [isMobileMenuOpen, setMobileMenuOpen] = useState(false);

  const handleLogout = async () => {
    await logout();
    router.replace("/login");
  };

  return (
    <div className={styles.shell}>
      <header className={styles.header}>
        <div className={styles.headerLeft}>
          <button 
            className={styles.mobileToggle} 
            onClick={() => setMobileMenuOpen(!isMobileMenuOpen)}
            aria-label="Toggle Navigation"
          >
            ☰
          </button>
          <div className={styles.brand}>ARTHA</div>
        </div>
        <div className={styles.headerRight}>
          <ConnectionStatus />
          {user && (
            <div className={styles.userInfo}>
              <span className={styles.userName}>{user.username} <span className={styles.userRole}>({user.role})</span></span>
              <button className={styles.logoutBtn} onClick={handleLogout} aria-label="Sign out">
                Sign Out
              </button>
            </div>
          )}
        </div>
      </header>
      
      <div className={styles.layout}>
        <nav className={`${styles.sidebar} ${isMobileMenuOpen ? styles.sidebarOpen : ""}`}>
          <ul className={styles.navList}>
            {NAV_LINKS.filter(link => !user || link.roles.includes(user.role)).map(link => (
              <li key={link.href}>
                <Link 
                  href={link.href} 
                  className={`${styles.navLink} ${pathname?.startsWith(link.href) ? styles.active : ""}`} 
                  onClick={() => setMobileMenuOpen(false)}
                >
                  {link.label}
                </Link>
              </li>
            ))}
          </ul>
        </nav>
        
        {/* Overlay for mobile when sidebar is open */}
        {isMobileMenuOpen && (
          <div 
            className={styles.mobileOverlay} 
            onClick={() => setMobileMenuOpen(false)}
            aria-hidden="true"
          />
        )}
        
        <main className={styles.mainContent}>
          {children}
        </main>
      </div>
    </div>
  );
}
