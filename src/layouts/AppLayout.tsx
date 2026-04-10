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
    <div className="flex h-screen w-screen overflow-hidden font-mono text-[13px] leading-normal text-text">
      <Sidebar />
      <div className="flex min-w-0 flex-1 flex-col">
        <main className="flex-1 overflow-y-auto bg-surface-alt">
          <Outlet />
        </main>
        <StatusBar />
      </div>
    </div>
  );
}
