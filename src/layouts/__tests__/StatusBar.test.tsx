import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { StatusBar } from "../StatusBar";

describe("StatusBar", () => {
  it("should render download speed", () => {
    render(<StatusBar />);
    expect(screen.getByText(/0\.0 MB\/s/)).toBeInTheDocument();
  });

  it("should render speed limit", () => {
    render(<StatusBar />);
    expect(screen.getByText(/Limit: unlimited/)).toBeInTheDocument();
  });

  it("should render free space", () => {
    render(<StatusBar />);
    expect(screen.getByText(/-- GB/)).toBeInTheDocument();
  });

  it("should render connection count", () => {
    render(<StatusBar />);
    expect(screen.getByText(/0 connections/)).toBeInTheDocument();
  });

  it("should render app version", () => {
    render(<StatusBar />);
    expect(screen.getByText(/vortex v0\.1\.0/)).toBeInTheDocument();
  });
});
