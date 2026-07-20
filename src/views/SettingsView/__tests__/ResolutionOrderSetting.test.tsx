import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ResolutionOrderSetting } from "../ResolutionOrderSetting";
import type { ResolutionTier } from "@/types/settings";

const DEFAULT_ORDER: ResolutionTier[] = ["premium", "debrid", "free"];

function renderSetting(value: ResolutionTier[] = DEFAULT_ORDER) {
  const onChange = vi.fn();
  render(<ResolutionOrderSetting value={value} onChange={onChange} />);
  return { onChange };
}

function renderedTiers(): string[] {
  return screen.getAllByRole("listitem").map((item) => item.textContent ?? "");
}

describe("ResolutionOrderSetting", () => {
  it("should list the tiers in the configured order when rendered", () => {
    renderSetting(["debrid", "free", "premium"]);

    const tiers = renderedTiers();
    expect(tiers[0]).toContain("Debrid service");
    expect(tiers[1]).toContain("Free");
    expect(tiers[2]).toContain("Premium account");
  });

  it("should append missing tiers when the persisted order is incomplete", () => {
    renderSetting(["debrid"]);

    const tiers = renderedTiers();
    expect(tiers).toHaveLength(3);
    expect(tiers[0]).toContain("Debrid service");
    expect(tiers[1]).toContain("Premium account");
    expect(tiers[2]).toContain("Free");
  });

  it("should promote debrid above premium when its move-up button is pressed", async () => {
    const user = userEvent.setup();
    const { onChange } = renderSetting();

    await user.click(screen.getByRole("button", { name: "Move Debrid service up" }));

    expect(onChange).toHaveBeenCalledWith(["debrid", "premium", "free"]);
  });

  it("should demote premium below debrid when its move-down button is pressed", async () => {
    const user = userEvent.setup();
    const { onChange } = renderSetting();

    await user.click(screen.getByRole("button", { name: "Move Premium account down" }));

    expect(onChange).toHaveBeenCalledWith(["debrid", "premium", "free"]);
  });

  it("should disable the moves that would push a tier off the list", () => {
    renderSetting();

    expect(screen.getByRole("button", { name: "Move Premium account up" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Move Free down" })).toBeDisabled();
  });
});
