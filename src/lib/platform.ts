export function isMacPlatform(): boolean {
  if (typeof navigator === "undefined") return false;
  return navigator.platform.includes("Mac");
}

export function getPrimaryModifierLabel(): string {
  return isMacPlatform() ? "Cmd" : "Ctrl";
}
