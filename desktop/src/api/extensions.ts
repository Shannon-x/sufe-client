import { invoke } from "@/platform";
export type { ClientConfig } from "@/stores/features";
export interface GiftCardCheck {
  code_info: Record<string, unknown>;
  reward_preview: Record<string, unknown>;
  can_redeem: boolean;
  reason: string | null;
}
export interface GiftCardResult {
  message: string;
  rewards: Record<string, unknown>;
  invite_rewards: unknown;
  template_name: string;
}
export interface GiftCardHistory {
  data: Array<{
    id: number;
    code: string;
    template_name: string;
    template_type_name: string;
    rewards_given: Record<string, unknown>;
    created_at: string | number;
  }>;
  pagination: {
    current_page: number;
    last_page: number;
    per_page: number;
    total: number;
  };
}
export interface Invites {
  codes: Array<{ id: number; code: string; status: number }>;
  stat: number[];
}
export const extensions = {
  fetchClientConfig: () =>
    invoke<import("@/stores/features").ClientConfig>("fetch_client_config"),
  checkGiftCard: (code: string) =>
    invoke<GiftCardCheck>("gift_card_check", { code }),
  redeemGiftCard: (code: string) =>
    invoke<GiftCardResult>("gift_card_redeem", { code }),
  giftCardHistory: () => invoke<GiftCardHistory>("gift_card_history"),
  fetchInvites: () => invoke<Invites>("fetch_invites"),
  createInvite: (code?: string) => invoke<boolean>("create_invite", { code }),
};
