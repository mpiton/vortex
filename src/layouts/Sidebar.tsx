import { NavLink } from "react-router";
import { ROUTES } from "@/types/layout";
import { useLayoutStore } from "@/stores/layout-store";
import { cn } from "@/lib/utils";

export function Sidebar() {
  const sidebarCollapsed = useLayoutStore((state) => state.sidebarCollapsed);

  return (
    <aside
      className={cn(
        "flex h-full flex-col bg-indigo-950 text-white transition-all",
        sidebarCollapsed ? "w-16" : "w-64",
      )}
    >
      <div className="p-4">
        {!sidebarCollapsed && (
          <h1 className="text-xl font-bold tracking-tight">Vortex</h1>
        )}
      </div>
      <nav className="flex-1 space-y-1 px-2">
        {ROUTES.map((route) => (
          <NavLink
            key={route.path}
            to={route.path}
            className={({ isActive }) =>
              cn(
                "flex items-center gap-3 rounded-lg px-4 py-3 text-sm transition-colors hover:bg-indigo-900",
                isActive && "bg-indigo-600 font-semibold",
              )
            }
          >
            <route.icon className="h-5 w-5 shrink-0" aria-hidden="true" />
            {sidebarCollapsed
              ? <span className="sr-only">{route.label}</span>
              : <span>{route.label}</span>
            }
          </NavLink>
        ))}
      </nav>
    </aside>
  );
}
