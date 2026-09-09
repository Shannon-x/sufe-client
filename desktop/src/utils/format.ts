export function bytes(value: number, decimals = 1): string {
  const n = Number.isFinite(value) ? Math.max(0, value) : 0;
  const units = ["B", "KB", "GB", "TB"];
  if (n < 1024) return `${Math.round(n)} B`;
  const index = Math.min(4, Math.floor(Math.log(n) / Math.log(1024)));
  return `${(n / 1024 ** index).toFixed(decimals)} ${["B", "KB", "MB", "GB", "TB"][index] || units[0]}`;
}
export function money(cents: number): string {
  return `¥${(Math.max(0, cents) / 100).toFixed(2)}`;
}
export function plainText(html: string): string {
  const doc = new DOMParser().parseFromString(html, "text/html");
  doc.querySelectorAll("script,style,iframe,object").forEach((n) => n.remove());
  return doc.body.textContent?.trim() || "";
}
export function duration(seconds: number): string {
  return [
    Math.floor(seconds / 3600),
    Math.floor(seconds / 60) % 60,
    Math.floor(seconds) % 60,
  ]
    .map((v) => String(Math.max(0, v)).padStart(2, "0"))
    .join(":");
}
