import test from "node:test";
import assert from "node:assert/strict";
import {
  paymentFee,
  couponDiscount,
  paymentState,
} from "../src/utils/billing.ts";

test("fees match Xboard rounding and never charge a fully credited order", () => {
  assert.equal(paymentFee(999, 25, 2.5), 50);
  assert.equal(paymentFee(1001, 25, 2.5), 50);
  assert.equal(paymentFee(0, 100, 5), 0);
  assert.equal(paymentFee(5000, -50, -3), 0);
  assert.equal(paymentFee(500, 5, 150), 505);
});
test("fixed and percentage coupons cannot make a negative payment", () => {
  assert.equal(couponDiscount(1000, 1, 1200), 1000);
  assert.equal(couponDiscount(999, 2, 100), 999);
  assert.equal(couponDiscount(999, 2, 50), 500);
  assert.equal(couponDiscount(999, 2, -10), 0);
  assert.equal(couponDiscount(999, 8, 100), 0);
});
test("paid but activating must not announce usable service", () => {
  assert.equal(paymentState(0), "pending");
  assert.equal(paymentState(1), "activating");
  assert.equal(paymentState(2), "cancelled");
  assert.equal(paymentState(3), "complete");
  assert.equal(paymentState(4), "complete");
  assert.equal(paymentState(100), "pending");
});
