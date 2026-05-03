import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SearchBar } from "../SearchBar";

describe("SearchBar", () => {
  it("should render input with placeholder", () => {
    render(<SearchBar value="" onChange={vi.fn()} />);
    expect(screen.getByPlaceholderText("Search downloads...")).toBeInTheDocument();
  });

  it("should display current value", () => {
    render(<SearchBar value="test query" onChange={vi.fn()} />);
    expect(screen.getByDisplayValue("test query")).toBeInTheDocument();
  });

  it("should call onChange when typing", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<SearchBar value="" onChange={onChange} />);
    const input = screen.getByPlaceholderText("Search downloads...");
    await user.type(input, "a");
    expect(onChange).toHaveBeenCalledWith("a");
  });
});
