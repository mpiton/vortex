import { useEffect } from "react";
import { Outlet, useNavigate } from "react-router";
import { Sidebar } from "./Sidebar";
import { StatusBar } from "./StatusBar";
import { ROUTES } from "@/types/layout";

export function AppLayout() {
  const navigate = useNavigate();

  useEffect(() => {
    function handleKeydown(event: KeyboardEvent) {
      const modifier = navigator.platform.includes("Mac") ? event.metaKey : event.ctrlKey;
      if (!modifier) return;

      const target = event.target as HTMLElement;
      if (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable) {
        return;
      }

      if (event.key === ",") {
        event.preventDefault();
        void navigate("/settings");
        return;
      }

      const index = parseInt(event.key, 10);
      if (index >= 1 && index <= 9 && ROUTES[index - 1]) {
        event.preventDefault();
        void navigate(ROUTES[index - 1].path);
      }
    }

    window.addEventListener("keydown", handleKeydown);
    return () => window.removeEventListener("keydown", handleKeydown);
  }, [navigate]);

  return (
    <div className="flex h-screen w-screen flex-col bg-background font-sans">
      <div className="flex flex-1 overflow-hidden">
        <Sidebar />
        <main className="flex-1 overflow-auto p-6">
          <Outlet />
        </main>
      </div>
      <StatusBar />
    </div>
  );
}
