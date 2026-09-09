import { invoke } from "@/platform";
export interface SupportAttachment {
  id: number;
  data_url: string;
  file_type: string;
  fallback_title?: string;
}
export interface SupportMessage {
  id: number;
  content: string | null;
  message_type: number;
  created_at: number;
  private?: boolean;
  sender?: { name?: string; available_name?: string };
  attachments?: SupportAttachment[];
}
export interface SupportConversation {
  id: number;
  status: string;
  messages?: SupportMessage[];
}
export const supportApi = {
  restore: () =>
    invoke<{ conversations: SupportConversation[] }>("support_request", {
      action: "restore",
    }),
  start: () =>
    invoke<SupportConversation>("support_request", { action: "start" }),
  messages: (conversationId: number) =>
    invoke<SupportMessage[]>("support_request", {
      action: "messages",
      conversationId,
    }),
  send: (conversationId: number, content: string) =>
    invoke<SupportMessage>("support_request", {
      action: "send",
      conversationId,
      content,
    }),
};
