export function getHostname(url: string): string {
  try {
    return new URL(url).hostname;
  } catch {
    return url;
  }
}

export function getProtocol(url: string): string {
  try {
    return new URL(url).protocol.replace(":", "").toUpperCase();
  } catch {
    const parts = url.split("://");
    return parts.length > 1 ? parts[0].toUpperCase() : "UNKNOWN";
  }
}
