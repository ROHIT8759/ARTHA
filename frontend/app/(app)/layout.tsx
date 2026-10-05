import Shell from "@/components/layout/Shell";
import ProtectedRoute from "@/components/auth/ProtectedRoute";

export default function AppLayout({ children }: { children: React.ReactNode }) {
  return (
    <ProtectedRoute>
      <Shell>{children}</Shell>
    </ProtectedRoute>
  );
}
