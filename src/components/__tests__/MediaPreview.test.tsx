import { describe, it, expect } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { MediaPreview } from "../MediaPreview";

describe("MediaPreview", () => {
  it("should display the title", () => {
    render(<MediaPreview title="Test Video" thumbnail="https://img.test/thumb.jpg" />);
    expect(screen.getByText("Test Video")).toBeInTheDocument();
  });

  it("should render thumbnail image", () => {
    render(<MediaPreview title="Test Video" thumbnail="https://img.test/thumb.jpg" />);
    const img = screen.getByRole("img", { name: "Test Video" });
    expect(img).toHaveAttribute("src", "https://img.test/thumb.jpg");
  });

  it("should show fallback when image fails to load", () => {
    render(<MediaPreview title="Test Video" thumbnail="https://broken-url/nope.jpg" />);
    const img = screen.getByRole("img", { name: "Test Video" });
    fireEvent.error(img);
    expect(screen.getByText("No preview available")).toBeInTheDocument();
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
  });
});
