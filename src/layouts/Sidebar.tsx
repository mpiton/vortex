import { NavLink } from "react-router";
import { ROUTES } from "@/types/layout";
import { cn } from "@/lib/utils";

/** Route indices for visual grouping with dividers */
const GROUP_BREAKS = new Set([3, 6]);

export function Sidebar() {
  return (
    <aside className="flex h-full w-[58px] flex-col items-center bg-sidebar-bg py-3.5 select-none shrink-0">
      {/* Logo */}
      <div className="mb-5 flex h-9 w-9 items-center justify-center rounded-lg bg-accent text-base font-semibold text-white tracking-tighter">
        Vx
      </div>

      {/* Navigation */}
      <nav className="flex flex-1 flex-col items-center gap-1">
        {ROUTES.map((route, index) => (
          <div key={route.path} className="flex flex-col items-center">
            {GROUP_BREAKS.has(index) && (
              <div className="my-2 h-px w-6 bg-sidebar-divider" />
            )}
            <NavLink
              to={route.path}
              title={route.label}
              className={({ isActive }) =>
                cn(
                  "flex h-10 w-10 items-center justify-center rounded-lg transition-colors",
                  "hover:bg-sidebar-hover",
                  isActive
                    ? "bg-accent/20 [&>svg]:stroke-accent-medium"
                    : "[&>svg]:stroke-sidebar-icon",
                )
              }
            >
              <route.icon className="h-[18px] w-[18px]" strokeWidth={1.8} aria-hidden="true" />
              <span className="sr-only">{route.label}</span>
            </NavLink>
          </div>
        ))}
      </nav>
    </aside>
  );
}
