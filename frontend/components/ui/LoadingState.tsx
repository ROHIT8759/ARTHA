"use client";

import styles from "./LoadingState.module.css";

interface LoadingStateProps {
  message?: string;
  fullPage?: boolean;
}

export default function LoadingState({ message = "Loading...", fullPage = false }: LoadingStateProps) {
  return (
    <div className={`${styles.container} ${fullPage ? styles.fullPage : ""}`}>
      <div className={styles.spinner}></div>
      {message && <p className={styles.message}>{message}</p>}
    </div>
  );
}
