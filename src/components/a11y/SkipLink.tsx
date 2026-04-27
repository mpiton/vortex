import type { MouseEvent } from "react";
import { useTranslation } from "react-i18next";

const TARGET_ID = "main-content";

export function SkipLink() {
  const { t } = useTranslation();

  function handleActivate(event: MouseEvent<HTMLAnchorElement>) {
    const target = document.getElementById(TARGET_ID);
    if (!target) return;
    event.preventDefault();
    target.focus();
  }

  return (
    <a
      href={`#${TARGET_ID}`}
      onClick={handleActivate}
      className="sr-only focus:not-sr-only focus:absolute focus:top-2 focus:left-2 focus:z-50 focus:rounded-md focus:bg-accent focus:px-3 focus:py-2 focus:text-sm focus:font-medium focus:text-white focus:shadow-lg focus:outline-none"
    >
      {t("a11y.skipToMain")}
    </a>
  );
}
