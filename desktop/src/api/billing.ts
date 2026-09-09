import { invoke } from "@/platform";
import type { CouponCheckResult, Order } from "@/types";

export interface OrderDetail extends Order {
  payment_id?: number | null;
  handling_amount?: number | null;
  plan?: { name?: string } | null;
}

export const billingApi = {
  detail: (tradeNo: string) => invoke<OrderDetail>("fetch_order", { tradeNo }),
  checkCoupon: (code: string, planId: number, period: string) =>
    invoke<CouponCheckResult>("check_coupon", { code, planId, period }),
  openPayment: (url: string, tradeNo: string) =>
    invoke<void>("open_payment_window", { url, tradeNo }),
};
