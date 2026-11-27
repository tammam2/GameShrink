export function formatBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const idx = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const val = bytes / Math.pow(1024, idx);
  return `${val.toFixed(idx >= 3 ? 2 : 1)} ${units[idx]}`;
}

export function formatModeLabel(mode: string): string {
  switch (mode) {
    case "max_crush":
      return "MAX CRUSH (LZX)";
    case "brute_max":
      return "BRUTE MAX (ALL LZX)";
    case "adaptive_thermal":
      return "ADAPTIVE THERMAL";
    default:
      return mode.toUpperCase();
  }
}

export function formatDate(iso: string): string {
  try {
    return new Date(iso).toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return iso;
  }
}
