import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { PluginConfigField } from "../PluginConfigField";
import type { PluginConfigField as ConfigField } from "@/types/plugin-config";

function makeField(overrides: Partial<ConfigField> = {}): ConfigField {
  return {
    key: "test_key",
    fieldType: "string",
    default: null,
    description: null,
    options: [],
    min: null,
    max: null,
    regex: null,
    ...overrides,
  };
}

describe("PluginConfigField", () => {
  it("renders a text input for string fields", () => {
    const onChange = vi.fn();
    render(
      <PluginConfigField
        field={makeField({ fieldType: "string" })}
        value="hello"
        onChange={onChange}
      />,
    );
    const input = screen.getByLabelText("test_key") as HTMLInputElement;
    expect(input.tagName).toBe("INPUT");
    expect(input.type).toBe("text");
    expect(input.value).toBe("hello");
  });

  it("renders a number input with bounds for integer fields", () => {
    render(
      <PluginConfigField
        field={makeField({ fieldType: "integer", min: 0, max: 10 })}
        value="3"
        onChange={() => {}}
      />,
    );
    const input = screen.getByLabelText("test_key") as HTMLInputElement;
    expect(input.type).toBe("number");
    expect(input.min).toBe("0");
    expect(input.max).toBe("10");
  });

  it("renders a switch for boolean fields and propagates checked state", () => {
    const onChange = vi.fn();
    render(
      <PluginConfigField
        field={makeField({ fieldType: "boolean" })}
        value="false"
        onChange={onChange}
      />,
    );
    const sw = screen.getByLabelText("test_key");
    fireEvent.click(sw);
    expect(onChange).toHaveBeenCalledWith("true");
  });

  it("renders a select for enum fields with the given options", () => {
    render(
      <PluginConfigField
        field={makeField({
          fieldType: "enum",
          options: ["360p", "720p", "1080p"],
        })}
        value="720p"
        onChange={() => {}}
      />,
    );
    expect(screen.getByText("720p")).toBeInTheDocument();
  });

  it("renders a select when string field declares options (enum-like)", () => {
    render(
      <PluginConfigField
        field={makeField({
          fieldType: "string",
          options: ["fast", "slow"],
        })}
        value="fast"
        onChange={() => {}}
      />,
    );
    expect(screen.getByText("fast")).toBeInTheDocument();
  });

  it("renders a url input for url fields", () => {
    render(
      <PluginConfigField
        field={makeField({ fieldType: "url" })}
        value="https://example.com"
        onChange={() => {}}
      />,
    );
    const input = screen.getByLabelText("test_key") as HTMLInputElement;
    expect(input.type).toBe("url");
  });

  it("displays the description when provided", () => {
    render(
      <PluginConfigField
        field={makeField({ description: "Choose your preferred quality" })}
        value=""
        onChange={() => {}}
      />,
    );
    expect(screen.getByText("Choose your preferred quality")).toBeInTheDocument();
  });

  it("displays the error message when provided", () => {
    render(
      <PluginConfigField
        field={makeField()}
        value=""
        onChange={() => {}}
        errorMessage="Required"
      />,
    );
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("Required");
  });
});
