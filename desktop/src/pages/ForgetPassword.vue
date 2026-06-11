<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref } from "vue";
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import {
  NButton,
  NCard,
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
  emailCode: "",
  password: "",
  passwordConfirm: "",
});

const siteConfig = computed(() => site.config);
const captchaRequired = computed(() => siteConfig.value?.is_captcha ?? false);
const captchaType = computed(() => siteConfig.value?.captcha_type ?? "");

const rules = computed<FormRules>(() => ({
  email: [
    { required: true, trigger: ["blur"], message: t("forget.fillAll") },
    {
      trigger: ["blur"],
      validator: (_r: unknown, value: string) =>
        !value ||
        /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value) ||
        new Error(t("register.invalidEmail")),
    },
  ],
  emailCode: { required: true, trigger: ["blur"], message: t("forget.fillAll") },
  password: [
    { required: true, trigger: ["blur"], message: t("forget.fillAll") },
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
    message.error(t("forget.fillAll"));
    return;
  }
  if (captchaRef.value?.misconfigured) {
    message.error(t("captcha.misconfigured"));
    return;
  }
  sendingCode.value = true;
  try {
    // sendEmailVerify is captcha-gated on the backend — resolve and pass the
    // provider tag so the panel's CaptchaService finds the token.
    const captchaTok = await resolveCaptcha();
    await api.sendEmailVerify(
      email,
      captchaRequired.value ? captchaType.value : undefined,
      captchaTok,
    );
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
    return (await captchaRef.value?.execute("forget")) ?? undefined;
  }
  return captchaToken.value ?? undefined;
}

async function onSubmit() {
  try {
    await formRef.value?.validate();
  } catch {
    return;
  }
  if (captchaRef.value?.misconfigured) {
    message.error(t("captcha.misconfigured"));
    return;
  }
  submitting.value = true;
  try {
    const token = await resolveCaptcha();
    if (
      captchaRequired.value &&
      captchaType.value !== "recaptcha-v3" &&
      !token &&
      !captchaRef.value?.unsupported &&
      !captchaRef.value?.misconfigured
    ) {
      message.error(t("login.captchaRequired"));
      submitting.value = false;
      return;
    }
    await api.forgetPassword({
      email: model.email.trim(),
      password: model.password,
      emailCode: model.emailCode,
      captchaType: captchaRequired.value ? captchaType.value : undefined,
      captchaToken: token,
    });
    message.success(t("forget.success"));
    // Don't auto-login — make the user re-enter the new password to confirm
    // the change took effect. Clear any stale session first so the user lands
    // cleanly on the login screen.
    await auth.logout();
    router.push("/login");
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
        <NH1 class="brand">{{ t("forget.heading") }}</NH1>
        <NText depth="3">{{ t("forget.tagline") }}</NText>
      </div>
      <NDivider />
      <NForm
        ref="formRef"
        :model="model"
        :rules="rules"
        label-placement="top"
        require-mark-placement="right-hanging"
        size="large"
      >
        <NFormItem :label="t('forget.emailLabel')" path="email">
          <NInput v-model:value="model.email" placeholder="user@example.com" clearable />
        </NFormItem>
        <NFormItem :label="t('forget.codeLabel')" path="emailCode">
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
        <NFormItem :label="t('forget.passwordLabel')" path="password">
          <NInput
            v-model:value="model.password"
            type="password"
            show-password-on="mousedown"
          />
        </NFormItem>
        <NFormItem
          :label="t('forget.passwordConfirmLabel')"
          path="passwordConfirm"
        >
          <NInput
            v-model:value="model.passwordConfirm"
            type="password"
            show-password-on="mousedown"
          />
        </NFormItem>
        <CaptchaWidget
          v-if="siteConfig"
          ref="captchaRef"
          :site-config="siteConfig"
          @token="(v: string) => (captchaToken = v)"
          @error="captchaToken = null"
        />
        <NSpace vertical>
          <NButton
            type="primary"
            block
            :loading="submitting"
            @click="onSubmit"
          >
            {{ submitting ? t("forget.submitting") : t("forget.submit") }}
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
  max-width: 440px;
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
</style>
