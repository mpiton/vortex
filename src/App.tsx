import "./i18n/i18n";
import { BrowserRouter, Routes, Route, Navigate } from "react-router";
import { QueryClientProvider } from "@tanstack/react-query";
import { Toaster } from "sonner";
import { TooltipProvider } from "@/components/ui/tooltip";
import { AppLayout } from "@/layouts/AppLayout";
import {
  DownloadsView,
  LinkGrabberView,
  PackagesView,
  AccountsView,
  CaptchaView,
  PluginsView,
  SchedulerView,
  HistoryView,
  StatisticsView,
  SettingsView,
} from "@/views";
import { queryClient } from "@/api/client";
import { CaptchaBrowserWindow } from "@/views/CaptchaBrowserWindow";

export function App() {
  const captchaWindowId = new URLSearchParams(window.location.search).get("captchaWindow");
  return (
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>
        {captchaWindowId ? (
          <CaptchaBrowserWindow challengeId={captchaWindowId} />
        ) : (
          <BrowserRouter>
            <Routes>
              <Route element={<AppLayout />}>
                <Route index element={<Navigate to="/downloads" replace />} />
                <Route path="downloads" element={<DownloadsView />} />
                <Route path="link-grabber" element={<LinkGrabberView />} />
                <Route path="packages" element={<PackagesView />} />
                <Route path="accounts" element={<AccountsView />} />
                <Route path="captcha" element={<CaptchaView />} />
                <Route path="plugins" element={<PluginsView />} />
                <Route path="scheduler" element={<SchedulerView />} />
                <Route path="history" element={<HistoryView />} />
                <Route path="statistics" element={<StatisticsView />} />
                <Route path="settings" element={<SettingsView />} />
                <Route path="*" element={<Navigate to="/downloads" replace />} />
              </Route>
            </Routes>
          </BrowserRouter>
        )}
        <Toaster richColors position="bottom-right" closeButton />
      </TooltipProvider>
    </QueryClientProvider>
  );
}
