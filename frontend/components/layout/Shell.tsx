"use client";

import { useState } from "react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import ConnectionStatus from "./ConnectionStatus";
import styles from "./Shell.module.css";

const NAV_LINKS = [
  { href: "/dashboard", label: "Dashboard" },
  { href: "/sales", label: "Sales" },
  { href: "/purchases", label: "Purchases" },
  { href: "/products", label: "Products" },
  { href: "/inventory", label: "Inventory" },
  { href: "/reports", label: "Reports" },
  { href: "/settings", label: "Settings" },
];

export default function Shell({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();
  const [isMobileMenuOpen, setMobileMenuOpen] = useState(false);

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
        </div>
      </header>
      
      <div className={styles.layout}>
        <nav className={`${styles.sidebar} ${isMobileMenuOpen ? styles.sidebarOpen : ""}`}>
          <ul className={styles.navList}>
            {NAV_LINKS.map(link => (
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
