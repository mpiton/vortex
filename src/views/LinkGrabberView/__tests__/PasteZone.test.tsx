import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { PasteZone, extractUrls } from "../PasteZone";

describe("extractUrls", () => {
  it("should find http/https/ftp/magnet links", () => {
    const text = [
      "http://example.com/file.zip",
      "https://secure.example.com/path?q=1",
      "ftp://ftp.example.com/pub/file.iso",
      "magnet:?xt=urn:btih:abc123&dn=test",
      "not a url",
    ].join("\n");

    const urls = extractUrls(text);
    expect(urls).toHaveLength(4);
    expect(urls[0]).toBe("http://example.com/file.zip");
    expect(urls[1]).toBe("https://secure.example.com/path?q=1");
    expect(urls[2]).toBe("ftp://ftp.example.com/pub/file.iso");
    expect(urls[3]).toBe("magnet:?xt=urn:btih:abc123&dn=test");
  });

  it("should return empty array when no URLs found", () => {
    expect(extractUrls("no urls here")).toEqual([]);
    expect(extractUrls("")).toEqual([]);
  });
});

describe("PasteZone", () => {
  it("should call onPasteUrls when Analyze Links clicked", async () => {
    const user = userEvent.setup();
    const onPasteUrls = vi.fn();
    render(<PasteZone onPasteUrls={onPasteUrls} />);

    const textarea = screen.getByRole("textbox");
    await user.type(textarea, "https://example.com/file.zip\nhttps://other.com/other.zip");

    await user.click(screen.getByRole("button", { name: "Analyze Links" }));

    expect(onPasteUrls).toHaveBeenCalledOnce();
    const called = onPasteUrls.mock.calls[0][0] as string[];
    expect(called).toContain("https://example.com/file.zip");
    expect(called).toContain("https://other.com/other.zip");
  });

  it("should show Resolving text when loading", () => {
    render(<PasteZone onPasteUrls={vi.fn()} isLoading />);
    expect(screen.getByRole("button", { name: "Resolving…" })).toBeDisabled();
  });

  it("should clear textarea when Clear button clicked", async () => {
    const user = userEvent.setup();
    render(<PasteZone onPasteUrls={vi.fn()} />);

    const textarea = screen.getByRole("textbox");
    await user.type(textarea, "https://example.com");
    expect(textarea).toHaveValue("https://example.com");

    await user.click(screen.getByRole("button", { name: "Clear" }));
    expect(textarea).toHaveValue("");
  });

  it("should detect container files on drop", () => {
    const onPasteUrls = vi.fn();
    render(<PasteZone onPasteUrls={onPasteUrls} />);

    const dropZone = screen.getByTestId("paste-drop-zone");

    const dlcFile = new File(["content"], "links.dlc", {
      type: "application/octet-stream",
    });

    fireEvent.drop(dropZone, {
      dataTransfer: {
        files: [dlcFile],
        getData: () => "",
      },
    });

    expect(onPasteUrls).toHaveBeenCalledWith(["container:links.dlc"]);
  });
});
