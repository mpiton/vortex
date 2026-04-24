import { useTranslation } from "react-i18next";
import { getPrimaryModifierLabel } from "@/lib/platform";

interface ShortcutDefinition {
  keyCombo: string;
  descriptionKey: string;
}

export function ShortcutsSection() {
  const { t } = useTranslation();
  const mod = getPrimaryModifierLabel();
  const rows: ShortcutDefinition[] = [
    { keyCombo: `${mod}+V`, descriptionKey: "settings.shortcuts.rows.pasteUrls" },
    { keyCombo: `${mod}+A`, descriptionKey: "settings.shortcuts.rows.selectAll" },
    { keyCombo: "Space", descriptionKey: "settings.shortcuts.rows.pauseResume" },
    { keyCombo: "Delete", descriptionKey: "settings.shortcuts.rows.deleteSelection" },
    { keyCombo: `${mod}+Shift+P`, descriptionKey: "settings.shortcuts.rows.toggleClipboard" },
    { keyCombo: `${mod}+1…9`, descriptionKey: "settings.shortcuts.rows.navigateViews" },
    { keyCombo: `${mod}+F`, descriptionKey: "settings.shortcuts.rows.focusSearch" },
    { keyCombo: `${mod}+N`, descriptionKey: "settings.shortcuts.rows.addUrlsDialog" },
    { keyCombo: `${mod}+,`, descriptionKey: "settings.shortcuts.rows.openSettings" },
    { keyCombo: "Escape", descriptionKey: "settings.shortcuts.rows.closePanel" },
  ];

  return (
    <section className="space-y-4">
      <header>
        <h2 className="text-lg font-semibold">{t("settings.shortcuts.title")}</h2>
        <p className="text-sm text-muted-foreground">{t("settings.shortcuts.description")}</p>
      </header>
      <table className="w-full text-sm">
        <thead>
          <tr className="border-b text-left">
            <th className="w-48 py-2 pr-4 font-medium">
              {t("settings.shortcuts.columns.shortcut")}
            </th>
            <th className="py-2 font-medium">{t("settings.shortcuts.columns.action")}</th>
          </tr>
        </thead>
        <tbody>
          {rows.map(({ keyCombo, descriptionKey }) => (
            <tr key={keyCombo} className="border-b last:border-b-0">
              <td className="py-2 pr-4">
                <kbd className="rounded border bg-muted px-2 py-0.5 font-mono text-xs">
                  {keyCombo}
                </kbd>
              </td>
              <td className="py-2">{t(descriptionKey)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  );
}
