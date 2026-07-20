import { useEffect, useState } from "react";
import { ChevronDown, ChevronUp } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useTauriMutation, useTauriQuery } from "@/api/hooks";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { useSettingsStore } from "@/stores/settingsStore";

const DEFAULT_TIMEOUT_SECONDS = 120;
const CAPTCHA_CREDENTIAL_QUERY = ["captcha_credential_status"] as const;
const SOLVERS = [
  { id: "vortex-mod-captcha-ocr", labelKey: "captcha.settings.solvers.ocr" },
  {
    id: "vortex-mod-captcha-anticaptcha",
    labelKey: "captcha.settings.solvers.antiCaptcha",
  },
  { id: "vortex-mod-captcha-browser", labelKey: "captcha.settings.solvers.browser" },
] as const;
const DEFAULT_SOLVER_ORDER = SOLVERS.map((solver) => solver.id);

interface CaptchaCredentialStatus {
  configured: boolean;
}

export function CaptchaSolverSettings() {
  const { t } = useTranslation();
  const configured = useSettingsStore(
    (state) => state.config?.captchaTimeoutSeconds ?? DEFAULT_TIMEOUT_SECONDS,
  );
  const updateConfig = useSettingsStore((state) => state.updateConfig);
  const [timeoutSeconds, setTimeoutSeconds] = useState(configured);
  const configuredOrder = useSettingsStore(
    (state) => state.config?.captchaSolverOrder ?? DEFAULT_SOLVER_ORDER,
  );
  const [solverOrder, setSolverOrder] = useState<readonly string[]>(configuredOrder);
  const [apiKey, setApiKey] = useState("");
  const credentialStatus = useTauriQuery<CaptchaCredentialStatus>("captcha_credential_status");
  const saveCredential = useTauriMutation<void, { apiKey: string }>("captcha_credential_set", {
    invalidateKeys: [CAPTCHA_CREDENTIAL_QUERY],
    onSuccess: () => setApiKey(""),
  });
  const deleteCredential = useTauriMutation<void, void>("captcha_credential_delete", {
    invalidateKeys: [CAPTCHA_CREDENTIAL_QUERY],
  });

  useEffect(() => setTimeoutSeconds(configured), [configured]);
  useEffect(() => setSolverOrder(configuredOrder), [configuredOrder]);

  const persistTimeout = () => {
    const normalized = Math.min(3_600, Math.max(10, timeoutSeconds || DEFAULT_TIMEOUT_SECONDS));
    setTimeoutSeconds(normalized);
    void updateConfig({ captchaTimeoutSeconds: normalized });
  };

  const persistSolverOrder = (nextOrder: readonly string[]) => {
    setSolverOrder(nextOrder);
    void updateConfig({ captchaSolverOrder: [...nextOrder] });
  };

  const toggleSolver = (id: string, enabled: boolean) => {
    persistSolverOrder(
      enabled ? [...solverOrder, id] : solverOrder.filter((solverId) => solverId !== id),
    );
  };

  const moveSolver = (id: string, offset: -1 | 1) => {
    const index = solverOrder.indexOf(id);
    const target = index + offset;
    if (index < 0 || target < 0 || target >= solverOrder.length) return;
    const nextOrder = [...solverOrder];
    [nextOrder[index], nextOrder[target]] = [nextOrder[target], nextOrder[index]];
    persistSolverOrder(nextOrder);
  };

  const visibleSolvers = [
    ...solverOrder
      .map((id) => SOLVERS.find((solver) => solver.id === id))
      .filter((solver): solver is (typeof SOLVERS)[number] => solver !== undefined),
    ...SOLVERS.filter((solver) => !solverOrder.includes(solver.id)),
  ];

  return (
    <Card>
      <CardHeader>
        <CardTitle>{t("captcha.settings.title")}</CardTitle>
        <CardDescription>{t("captcha.settings.description")}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-5">
        <div className="space-y-2">
          <p className="text-sm font-medium">{t("captcha.settings.automatic")}</p>
          <div className="space-y-2">
            {visibleSolvers.map((solver) => {
              const enabled = solverOrder.includes(solver.id);
              const index = solverOrder.indexOf(solver.id);
              const label = t(solver.labelKey);
              return (
                <div className="flex items-center gap-3 rounded-md border p-3" key={solver.id}>
                  <Checkbox
                    aria-label={label}
                    checked={enabled}
                    id={`captcha-solver-${solver.id}`}
                    onCheckedChange={(checked) => toggleSolver(solver.id, checked === true)}
                  />
                  <label
                    className="min-w-0 flex-1 text-sm font-medium"
                    htmlFor={`captcha-solver-${solver.id}`}
                  >
                    {label}
                  </label>
                  {enabled ? (
                    <div className="flex gap-1">
                      <Button
                        aria-label={t("captcha.settings.moveUp", { solver: label })}
                        disabled={index === 0}
                        onClick={() => moveSolver(solver.id, -1)}
                        size="icon"
                        type="button"
                        variant="ghost"
                      >
                        <ChevronUp />
                      </Button>
                      <Button
                        aria-label={t("captcha.settings.moveDown", { solver: label })}
                        disabled={index === solverOrder.length - 1}
                        onClick={() => moveSolver(solver.id, 1)}
                        size="icon"
                        type="button"
                        variant="ghost"
                      >
                        <ChevronDown />
                      </Button>
                    </div>
                  ) : null}
                </div>
              );
            })}
          </div>
          <p className="text-xs text-muted-foreground">
            {t("captcha.settings.tesseractDetection")}
          </p>
        </div>

        <div className="space-y-2 rounded-md border p-3">
          <label className="text-sm font-medium" htmlFor="captcha-anticaptcha-key">
            {t("captcha.settings.apiKey")}
          </label>
          <Input
            autoComplete="off"
            id="captcha-anticaptcha-key"
            maxLength={1_024}
            onChange={(event) => setApiKey(event.target.value)}
            type="password"
            value={apiKey}
          />
          <div className="flex flex-wrap items-center gap-2">
            <Button
              disabled={apiKey.trim().length === 0 || saveCredential.isPending}
              onClick={() => saveCredential.mutate({ apiKey: apiKey.trim() })}
              type="button"
            >
              {t("captcha.settings.saveApiKey")}
            </Button>
            {credentialStatus.data?.configured ? (
              <Button
                disabled={deleteCredential.isPending}
                onClick={() => deleteCredential.mutate()}
                type="button"
                variant="outline"
              >
                {t("captcha.settings.deleteApiKey")}
              </Button>
            ) : null}
            <span className="text-xs text-muted-foreground">
              {t(
                credentialStatus.data?.configured
                  ? "captcha.settings.apiKeyConfigured"
                  : "captcha.settings.apiKeyMissing",
              )}
            </span>
          </div>
        </div>

        <div className="grid gap-4 sm:grid-cols-2">
          <div>
            <p className="text-sm font-medium">{t("captcha.settings.manual")}</p>
            <p className="text-sm text-muted-foreground">
              {t("captcha.settings.manualDescription")}
            </p>
          </div>
          <div className="space-y-2">
            <label className="text-sm font-medium" htmlFor="captcha-timeout">
              {t("captcha.settings.timeout")}
            </label>
            <Input
              id="captcha-timeout"
              max={3_600}
              min={10}
              onBlur={persistTimeout}
              onChange={(event) => {
                if (!Number.isNaN(event.currentTarget.valueAsNumber)) {
                  setTimeoutSeconds(event.currentTarget.valueAsNumber);
                }
              }}
              type="number"
              value={timeoutSeconds}
            />
            <p className="text-xs text-muted-foreground">{t("captcha.settings.fallback")}</p>
          </div>
        </div>
      </CardContent>
    </Card>
  );
}
