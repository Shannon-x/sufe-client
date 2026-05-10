import { isCommandError, type CommandError } from "@/types";

export function formatError(err: unknown, t: (k: string, v?: any) => string): string {
  if (isCommandError(err)) {
    const e = err as CommandError;
    switch (e.kind) {
      case "network":
        return t("errors.network");
      case "unauthorized":
        return t("errors.unauthorized");
      case "api":
        return t("errors.api", { message: e.message });
      default:
        return t("errors.other", { message: e.message });
    }
  }
  if (err instanceof Error) return err.message;
  return String(err);
}
