import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { StatusBar } from "../StatusBar";

describe("StatusBar", () => {
  it("should render active count", () => {
    render(<StatusBar />);
    expect(screen.getByText("0 active")).toBeInTheDocument();
  });

  it("should render download speed", () => {
    render(<StatusBar />);
    expect(screen.getByText(/0\.0 MB\/s/)).toBeInTheDocument();
  });

  it("should render free space", () => {
    render(<StatusBar />);
    expect(screen.getByText("-- GB free")).toBeInTheDocument();
  });

  it("should render connection count", () => {
    render(<StatusBar />);
    expect(screen.getByText("0 conn.")).toBeInTheDocument();
  });

  it("should render app version", () => {
    render(<StatusBar />);
    expect(screen.getByText("v0.1.0")).toBeInTheDocument();
  });
});
