<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { NAlert } from "naive-ui";
import { useI18n } from "vue-i18n";
import type { SiteConfig } from "@/types";

// Renders captcha provider widgets (Cloudflare Turnstile / reCAPTCHA v2 / v3)
// against the public site config. Parent components either:
//   - listen to `@token` for v2-style challenges, OR
//   - call `execute(action)` via a template ref for invisible v3 challenges.
//
// The widget injects its provider script on mount and removes the script
// tag + any global callbacks on unmount to keep route changes leak-free.

const props = defineProps<{
  siteConfig: SiteConfig;
}>();

const emit = defineEmits<{
  token: [value: string];
  error: [reason: string];
}>();

const { t } = useI18n();

// Each instance gets a unique id so multiple captchas on screen can't
// collide on widgetId-shaped global callback names.
const instanceId = `xboard_captcha_${Math.random().toString(36).slice(2, 10)}`;
const callbackName = `${instanceId}_cb`;
const errorCbName = `${instanceId}_err`;

const containerRef = ref<HTMLDivElement | null>(null);
const widgetId = ref<string | number | null>(null);
const ready = ref(false);
const currentToken = ref<string | null>(null);
let injectedScriptSrc: string | null = null;

const provider = computed(() => props.siteConfig.captcha_type);
const required = computed(() => props.siteConfig.is_captcha);

const scriptSrc = computed(() => {
  switch (provider.value) {
    case "turnstile":
      return "https://challenges.cloudflare.com/turnstile/v0/api.js";
    case "recaptcha":
      return "https://www.google.com/recaptcha/api.js";
    case "recaptcha-v3":
      // v3 is invisible — the action is supplied at execute() time.
      return `https://www.google.com/recaptcha/api.js?render=${encodeURIComponent(
        props.siteConfig.recaptcha_v3_site_key,
      )}`;
    default:
      return null;
  }
});

const siteKey = computed(() => {
  switch (provider.value) {
    case "turnstile":
      return props.siteConfig.turnstile_site_key;
    case "recaptcha":
      return props.siteConfig.recaptcha_site_key;
    case "recaptcha-v3":
      return props.siteConfig.recaptcha_v3_site_key;
    default:
      return "";
  }
});

// True only when captcha is required by the backend AND the site key is
// missing. We surface a banner rather than silently bypassing — the
// matching backend submission would fail with `captcha_invalid` anyway.
const misconfigured = computed(() => required.value && !siteKey.value);

// True when the configured provider isn't one we recognise.
const unsupported = computed(
  () =>
    required.value &&
    provider.value !== "turnstile" &&
    provider.value !== "recaptcha" &&
    provider.value !== "recaptcha-v3",
);

function loadScript(src: string): Promise<void> {
  return new Promise((resolve, reject) => {
    if (document.querySelector(`script[src="${src}"]`)) {
      injectedScriptSrc = src;
      resolve();
      return;
    }
    const s = document.createElement("script");
    s.src = src;
    s.async = true;
    s.defer = true;
    s.onload = () => {
      injectedScriptSrc = src;
      resolve();
    };
    s.onerror = () => reject(new Error(`failed to load ${src}`));
    document.head.appendChild(s);
  });
}

function renderWidget() {
  if (!containerRef.value || !siteKey.value) return;
  const w = window as unknown as Record<string, any>;

  // v2 callbacks: success → emit token; expired/error → emit error.
  w[callbackName] = (token: string) => {
    currentToken.value = token;
    emit("token", token);
  };
  w[errorCbName] = () => {
    currentToken.value = null;
    emit("error", "captcha");
  };

  if (provider.value === "turnstile") {
    // Wait for the global helper to materialise.
    const tryRender = () => {
      const ts = (w.turnstile as
        | {
            render: (el: HTMLElement, opts: Record<string, unknown>) => string | number;
            reset?: (id: string | number) => void;
          }
        | undefined);
      if (!ts) {
        setTimeout(tryRender, 100);
        return;
      }
      widgetId.value = ts.render(containerRef.value!, {
        sitekey: siteKey.value,
        callback: callbackName,
        "error-callback": errorCbName,
        "expired-callback": errorCbName,
        theme: "auto",
      });
      ready.value = true;
    };
    tryRender();
  } else if (provider.value === "recaptcha") {
    const tryRender = () => {
      const grec = w.grecaptcha as
        | {
            ready: (cb: () => void) => void;
            render: (el: HTMLElement, opts: Record<string, unknown>) => number;
            reset?: (id: number) => void;
          }
        | undefined;
      if (!grec || !grec.ready) {
        setTimeout(tryRender, 100);
        return;
      }
      grec.ready(() => {
        widgetId.value = grec.render(containerRef.value!, {
          sitekey: siteKey.value,
          callback: callbackName,
          "error-callback": errorCbName,
          "expired-callback": errorCbName,
        });
        ready.value = true;
      });
    };
    tryRender();
  } else if (provider.value === "recaptcha-v3") {
    // v3 has no inline UI — just wait until grecaptcha is ready so
    // execute() doesn't have to retry.
    const tryReady = () => {
      const grec = w.grecaptcha as
        | { ready: (cb: () => void) => void }
        | undefined;
      if (!grec || !grec.ready) {
        setTimeout(tryReady, 100);
        return;
      }
      grec.ready(() => {
        ready.value = true;
      });
    };
    tryReady();
  }
}

async function execute(action = "submit"): Promise<string | undefined> {
  if (!required.value) return undefined;
  if (provider.value === "recaptcha-v3") {
    const grec = (window as any).grecaptcha as
      | {
          execute: (siteKey: string, opts: { action: string }) => Promise<string>;
        }
      | undefined;
    if (!grec || !siteKey.value) return undefined;
    const tok = await grec.execute(siteKey.value, { action });
    currentToken.value = tok;
    return tok;
  }
  return currentToken.value ?? undefined;
}

// `resolve` is the symmetric, action-agnostic entrypoint callers use when
// they just want "whatever token is appropriate for this submission" —
// e.g. the email-verify request, which doesn't carry semantic action labels.
// For v3 we still need an action string for analytics; "submit" is fine.
function resolve(action = "submit"): Promise<string | undefined> {
  return execute(action);
}

function reset() {
  currentToken.value = null;
  const w = window as unknown as Record<string, any>;
  if (provider.value === "turnstile" && widgetId.value !== null) {
    w.turnstile?.reset?.(widgetId.value);
  } else if (provider.value === "recaptcha" && widgetId.value !== null) {
    w.grecaptcha?.reset?.(widgetId.value);
  }
}

// Surface `misconfigured` / `unsupported` so the page-level submit handlers
// can distinguish "captcha required but admin forgot the site key" (block
// + toast) from "captcha required but client doesn't know this provider"
// (bypass; let the panel reject server-side).
defineExpose({ execute, resolve, reset, misconfigured, unsupported });

onMounted(async () => {
  if (!required.value || misconfigured.value || unsupported.value) return;
  const src = scriptSrc.value;
  if (!src) return;
  try {
    await loadScript(src);
    renderWidget();
  } catch (e) {
    emit("error", String(e));
  }
});

// If captcha config flips at runtime (rare — site config refresh) re-render.
watch(provider, () => {
  // Naive approach: just clear the slot. Full re-render on provider switch
  // is uncommon enough that we don't optimise it.
  ready.value = false;
  widgetId.value = null;
});

onBeforeUnmount(() => {
  const w = window as unknown as Record<string, any>;
  delete w[callbackName];
  delete w[errorCbName];
  if (injectedScriptSrc) {
    document
      .querySelectorAll(`script[src="${injectedScriptSrc}"]`)
      .forEach((s) => s.parentElement?.removeChild(s));
  }
});
</script>

<template>
  <div v-if="required" class="captcha-host">
    <NAlert
      v-if="misconfigured"
      type="warning"
      :title="t('captcha.misconfiguredTitle')"
      :show-icon="true"
    >
      {{ t("captcha.misconfiguredBody") }}
    </NAlert>
    <NAlert
      v-else-if="unsupported"
      type="warning"
      :title="t('captcha.unsupportedTitle')"
      :show-icon="true"
    >
      {{ t("captcha.unsupportedBody", { provider }) }}
    </NAlert>
    <div
      v-else-if="provider !== 'recaptcha-v3'"
      ref="containerRef"
      class="captcha-slot"
    />
    <!-- v3 is invisible — nothing to render here. -->
  </div>
</template>

<style scoped>
.captcha-host {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.captcha-slot {
  min-height: 65px;
}
</style>
