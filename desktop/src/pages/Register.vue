<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref } from "vue";
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import {
  NAlert,
  NButton,
  NCard,
  NCheckbox,
  NDivider,
  NForm,
  NFormItem,
  NH1,
  NImage,
  NInput,
  NInputGroup,
  NSpace,
  NText,
  useMessage,
  type FormInst,
  type FormRules,
} from "naive-ui";
import { open as shellOpen } from "@tauri-apps/plugin-shell";
import { api } from "@/api";
import { useAuthStore } from "@/stores/auth";
import { useSiteStore } from "@/stores/site";
import { formatError } from "@/utils/error";
import CaptchaWidget from "@/components/CaptchaWidget.vue";

const { t } = useI18n();
const router = useRouter();
const auth = useAuthStore();
const site = useSiteStore();
const message = useMessage();

const formRef = ref<FormInst | null>(null);
const captchaRef = ref<InstanceType<typeof CaptchaWidget> | null>(null);
const submitting = ref(false);
const sendingCode = ref(false);
const codeCountdown = ref(0);
const captchaToken = ref<string | null>(null);
let countdownTimer: number | undefined;

const model = reactive({
  email: "",
  password: "",
  passwordConfirm: "",
  emailCode: "",
  inviteCode: "",
  agreeTos: false,
});

const siteConfig = computed(() => site.config);
const requiresEmailVerify = computed(() => siteConfig.value?.is_email_verify ?? false);
const requiresInvite = computed(() => siteConfig.value?.is_invite_force ?? false);
const captchaRequired = computed(() => siteConfig.value?.is_captcha ?? false);
const captchaType = computed(() => siteConfig.value?.captcha_type ?? "");
const tosUrl = computed(() => siteConfig.value?.tos_url ?? "");
const allowedSuffixes = computed(() => siteConfig.value?.email_whitelist_suffix ?? []);

const rules = computed<FormRules>(() => ({
  email: [
    { required: true, trigger: ["blur"], message: t("register.fillAll") },
    {
      trigger: ["blur"],
      validator: (_r: unknown, value: string) => {
        if (!value) return true;
        if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value))
          return new Error(t("register.invalidEmail"));
        if (allowedSuffixes.value.length > 0) {
          const ok = allowedSuffixes.value.some((s) =>
            value.toLowerCase().endsWith(s.toLowerCase()),
          );
          if (!ok)
            return new Error(
              t("register.suffixMismatch", {
                suffixes: allowedSuffixes.value.join(", "),
              }),
            );
        }
        return true;
      },
    },
  ],
  password: [
    { required: true, trigger: ["blur"], message: t("register.fillAll") },
    {
      trigger: ["blur"],
      validator: (_r: unknown, value: string) =>
        !value || value.length >= 8 || new Error(t("register.passwordTooShort")),
    },
  ],
  passwordConfirm: {
    required: true,
    trigger: ["blur"],
    validator: (_r: unknown, value: string) =>
      value === model.password || new Error(t("register.passwordMismatch")),
  },
  emailCode: requiresEmailVerify.value
    ? { required: true, trigger: ["blur"], message: t("register.fillAll") }
    : {},
  inviteCode: requiresInvite.value
    ? { required: true, trigger: ["blur"], message: t("register.inviteRequired") }
    : {},
}));

onMounted(() => {
  site.ensure().catch(() => undefined);
});

onUnmounted(() => {
  if (countdownTimer) window.clearInterval(countdownTimer);
});

async function onSendCode() {
  const email = model.email.trim();
  if (!email) {
    message.error(t("register.fillAll"));
    return;
  }
  sendingCode.value = true;
  try {
    await api.sendEmailVerify(email);
    message.success(t("register.codeSent"));
    codeCountdown.value = 60;
    countdownTimer = window.setInterval(() => {
      codeCountdown.value -= 1;
      if (codeCountdown.value <= 0 && countdownTimer) {
        window.clearInterval(countdownTimer);
        countdownTimer = undefined;
      }
    }, 1000);
  } catch (e) {
    message.error(formatError(e, t));
  } finally {
    sendingCode.value = false;
  }
}

async function resolveCaptcha(): Promise<string | undefined> {
  if (!captchaRequired.value) return undefined;
  if (captchaType.value === "recaptcha-v3") {
    return (await captchaRef.value?.execute("register")) ?? undefined;
  }
  return captchaToken.value ?? undefined;
}

async function openTos() {
  if (!tosUrl.value) return;
  try {
    await shellOpen(tosUrl.value);
  } catch (e) {
    message.error(formatError(e, t));
  }
}

async function onSubmit() {
  try {
    await formRef.value?.validate();
  } catch {
    return;
  }
  if (tosUrl.value && !model.agreeTos) {
    message.error(t("register.mustAgreeTos"));
    return;
  }
  submitting.value = true;
  try {
    const token = await resolveCaptcha();
    if (captchaRequired.value && captchaType.value !== "recaptcha-v3" && !token) {
      message.error(t("login.captchaRequired"));
      submitting.value = false;
      return;
    }
    const summary = await auth.register({
      email: model.email.trim(),
      password: model.password,
      emailCode: model.emailCode,
      inviteCode: model.inviteCode || undefined,
      turnstile: captchaType.value === "turnstile" ? token : undefined,
      recaptcha:
        captchaType.value === "recaptcha" || captchaType.value === "recaptcha-v3"
          ? token
          : undefined,
    });
    message.success(t("register.success", { email: summary.email }));
    router.push("/");
  } catch (e) {
    captchaRef.value?.reset();
    captchaToken.value = null;
    message.error(formatError(e, t));
  } finally {
    submitting.value = false;
  }
}
</script>

<template>
  <div class="auth-shell">
    <NCard class="auth-card" :bordered="false" size="large">
      <div class="auth-head">
        <NImage
          v-if="siteConfig?.logo"
          :src="siteConfig.logo"
          width="56"
          height="56"
          preview-disabled
          object-fit="contain"
        />
        <NH1 class="brand">{{ t("register.heading") }}</NH1>
        <NText depth="3">{{ t("register.tagline") }}</NText>
      </div>
      <NDivider />
      <NAlert
        v-if="allowedSuffixes.length"
        type="info"
        :show-icon="true"
        class="suffix-alert"
      >
        {{ t("register.suffixHint", { suffixes: allowedSuffixes.join(", ") }) }}
      </NAlert>
      <NForm
        ref="formRef"
        :model="model"
        :rules="rules"
        label-placement="top"
        require-mark-placement="right-hanging"
        size="large"
      >
        <NFormItem :label="t('register.emailLabel')" path="email">
          <NInput v-model:value="model.email" placeholder="user@example.com" clearable />
        </NFormItem>
        <NFormItem
          v-if="requiresEmailVerify"
          :label="t('register.codeLabel')"
          path="emailCode"
        >
          <NInputGroup>
            <NInput v-model:value="model.emailCode" placeholder="123456" />
            <NButton
              :disabled="codeCountdown > 0 || sendingCode"
              :loading="sendingCode"
              @click="onSendCode"
            >
              {{
                codeCountdown > 0
                  ? t("register.codeResend", { sec: codeCountdown })
                  : t("register.codeSend")
              }}
            </NButton>
          </NInputGroup>
        </NFormItem>
        <NFormItem :label="t('register.passwordLabel')" path="password">
          <NInput
            v-model:value="model.password"
            type="password"
            show-password-on="mousedown"
          />
        </NFormItem>
        <NFormItem
          :label="t('register.passwordConfirmLabel')"
          path="passwordConfirm"
        >
          <NInput
            v-model:value="model.passwordConfirm"
            type="password"
            show-password-on="mousedown"
          />
        </NFormItem>
        <NFormItem
          v-if="requiresInvite"
          :label="t('register.inviteLabel')"
          path="inviteCode"
        >
          <NInput v-model:value="model.inviteCode" placeholder="ABCDEF" />
        </NFormItem>
        <CaptchaWidget
          v-if="siteConfig"
          ref="captchaRef"
          :site-config="siteConfig"
          @token="(v: string) => (captchaToken = v)"
          @error="captchaToken = null"
        />
        <NFormItem v-if="tosUrl" :show-label="false">
          <NCheckbox v-model:checked="model.agreeTos">
            <i18n-t keypath="register.tosLabel" tag="span">
              <template #link>
                <NButton text size="small" @click.prevent="openTos">
                  {{ t("register.tosLink") }}
                </NButton>
              </template>
            </i18n-t>
          </NCheckbox>
        </NFormItem>
        <NSpace vertical>
          <NButton
            type="primary"
            block
            :loading="submitting"
            @click="onSubmit"
          >
            {{ submitting ? t("register.submitting") : t("register.submit") }}
          </NButton>
          <NSpace justify="end">
            <NButton text size="small" @click="router.push('/login')">
              {{ t("register.backToLogin") }}
            </NButton>
          </NSpace>
        </NSpace>
      </NForm>
    </NCard>
  </div>
</template>

<style scoped>
.auth-shell {
  min-height: 100vh;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  background: linear-gradient(160deg, #0f172a 0%, #1e293b 50%, #0f766e 100%);
}
.auth-card {
  width: 100%;
  max-width: 460px;
  border-radius: 18px;
  backdrop-filter: blur(20px);
}
.auth-head {
  text-align: center;
}
.brand {
  margin: 0 0 4px;
  letter-spacing: 0.04em;
}
.suffix-alert {
  margin-bottom: 12px;
}
</style>
