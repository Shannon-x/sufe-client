/** Money is always integer cents, matching Xboard's PHP checkout. */
export function paymentFee(total: number, fixed = 0, percent = 0): number {
  if (total <= 0) return 0;
  return Math.round(
    (total * Math.max(0, Math.min(100, percent))) / 100 + Math.max(0, fixed),
  );
}

export function couponDiscount(
  base: number,
  type: number,
  value: number,
): number {
  if (type === 1) return Math.min(base, Math.max(0, Math.round(value)));
  if (type === 2)
    return Math.round((base * Math.max(0, Math.min(100, value))) / 100);
  return 0;
}

export function paymentState(
  status: number,
): "pending" | "activating" | "cancelled" | "complete" {
  if (status === 1) return "activating";
  if (status === 2) return "cancelled";
  if (status === 3 || status === 4) return "complete";
  return "pending";
}

export const periodNames: Record<string, string> = {
  month_price: "月付",
  quarter_price: "季付",
  half_year_price: "半年付",
  year_price: "年付",
  two_year_price: "两年付",
  three_year_price: "三年付",
  onetime_price: "一次性",
  reset_price: "流量重置",
  monthly: "月付",
  quarterly: "季付",
  half_yearly: "半年付",
  yearly: "年付",
  two_yearly: "两年付",
  three_yearly: "三年付",
  onetime: "一次性",
  reset: "流量重置",
};

export function money(cents: number): string {
  return (Number(cents || 0) / 100).toFixed(2);
}
