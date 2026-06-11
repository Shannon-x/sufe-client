<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
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
  NSpace,
  NText,
  useMessage,
  type FormInst,
  type FormRules,
} from "naive-ui";
import { useAuthStore } from "@/stores/auth";
import { useSiteStore } from "@/stores/site";
import { useThemeStore } from "@/stores/theme";
import { formatError } from "@/utils/error";
import CaptchaWidget from "@/components/CaptchaWidget.vue";

const { t } = useI18n();
const router = useRouter();
const route = useRoute();
const auth = useAuthStore();
const site = useSiteStore();
const theme = useThemeStore();
const message = useMessage();

const formRef = ref<FormInst | null>(null);
const captchaRef = ref<InstanceType<typeof CaptchaWidget> | null>(null);
const submitting = ref(false);
const captchaToken = ref<string | null>(null);

const model = reactive({
  email: "",
  password: "",
});

const rules: FormRules = {
  email: { required: true, trigger: ["blur"], message: t("login.fillAll") },
  password: { required: true, trigger: ["blur"], message: t("login.fillAll") },
};

const siteConfig = computed(() => site.config);
const captchaRequired = computed(() => siteConfig.value?.is_captcha ?? false);
const captchaType = computed(() => siteConfig.value?.captcha_type ?? "");

onMounted(() => {
  site.ensure().catch(() => undefined);
});

async function resolveCaptcha(): Promise<string | undefined> {
  if (!captchaRequired.value) return undefined;
  if (captchaType.value === "recaptcha-v3") {
    return (await captchaRef.value?.execute("login")) ?? undefined;
  }
  return captchaToken.value ?? undefined;
}

async function onSubmit() {
  try {
    await formRef.value?.validate();
  } catch {
    return;
  }
  // If the panel admin enabled captcha but forgot the site key we can't
  // produce a token — surface that loudly. If the configured provider is
  // one this build doesn't know how to render, bypass the gate and let the
  // backend reject the submission with its own error.
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
    const summary = await auth.login({
      email: model.email.trim(),
      password: model.password,
      // Pass the provider tag + token; the Rust side routes the token into
      // the exact field the panel's CaptchaService reads (turnstile_token /
      // recaptcha_v3_token / recaptcha_data). Collapsing v3 into the v2
      // field was what broke Turnstile/v3 panels.
      captchaType: captchaRequired.value ? captchaType.value : undefined,
      captchaToken: token,
    });
    message.success(t("login.success", { email: summary.email }));
    const redirect = (route.query.redirect as string) || "/";
    if (redirect === "/" || redirect === "/home") {
      sessionStorage.setItem("xboard.autoConnectAfterLogin", "1");
    }
    router.push(redirect);
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
  <div class="login-shell">
    <NCard class="login-card" :bordered="false" size="large">
      <div class="login-head">
        <NImage
          v-if="siteConfig?.logo"
          :src="siteConfig.logo"
          width="64"
          height="64"
          preview-disabled
          object-fit="contain"
          class="brand-logo"
        />
        <NH1 class="brand">{{ t("app.title") }}</NH1>
        <!-- Plain text only — never v-html — backend description is untrusted. -->
        <NText depth="3">
          {{ siteConfig?.app_description || t("app.tagline") }}
        </NText>
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
        <NFormItem :label="t('login.emailLabel')" path="email">
          <NInput
            v-model:value="model.email"
            type="text"
            placeholder="user@example.com"
            clearable
          />
        </NFormItem>
        <NFormItem :label="t('login.passwordLabel')" path="password">
          <NInput
            v-model:value="model.password"
            type="password"
            show-password-on="mousedown"
            placeholder="••••••••"
            @keyup.enter="onSubmit"
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
            attr-type="submit"
            :loading="submitting"
            @click="onSubmit"
          >
            {{ submitting ? t("login.submitting") : t("login.submit") }}
          </NButton>
          <NSpace justify="space-between">
            <NSpace>
              <NButton text size="small" @click="router.push('/register')">
                {{ t("login.toRegister") }}
              </NButton>
              <NButton
                text
                size="small"
                @click="router.push('/forget-password')"
              >
                {{ t("login.toForget") }}
              </NButton>
            </NSpace>
            <NButton text size="small" @click="theme.toggle">
              {{ theme.dark ? "☀︎" : "☾" }}
            </NButton>
          </NSpace>
        </NSpace>
      </NForm>
    </NCard>
  </div>
</template>

<style scoped>
.login-shell {
  min-height: 100vh;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  background: linear-gradient(160deg, #0f172a 0%, #1e293b 50%, #0f766e 100%);
}

.login-card {
  width: 100%;
  max-width: 420px;
  border-radius: 18px;
  backdrop-filter: blur(20px);
}

.login-head {
  text-align: center;
}

.brand-logo {
  margin: 0 auto 8px;
  display: block;
}

.brand {
  margin: 0 0 4px;
  letter-spacing: 0.04em;
}
</style>
