<script setup lang="ts">
import AppIcon from "./AppIcon.vue";
import { useFeaturesStore } from "@/stores/features";
import { isDesktop, isPreview } from "@/platform";
const features = useFeaturesStore();
const development = import.meta.env.DEV;
</script>
<template>
  <div class="auth-layout">
    <header class="auth-header">
      <RouterLink to="/login"
        ><span class="brand-symbol"
          ><AppIcon name="lightning" :size="23" /></span
        ><span class="brand-word"
          >{{ features.config?.brand_name || "Sufe" }}<small>速飞</small></span
        ></RouterLink
      ><span>让连接，自由一点。</span>
    </header>
    <div class="auth-body">
      <section class="auth-story">
        <span class="eyebrow">A LITTLE CLOSER TO THE WORLD</span>
        <h1>世界很大，<br />自由连接每一刻<span>。</span></h1>
        <p>
          工作、学习，或是探索新的可能。<br />简单的一次连接，让精彩不再遥远。
        </p>
        <div class="orbit-art" aria-hidden="true">
          <div class="orbit o1" />
          <div class="orbit o2" />
          <div class="orbit o3" />
          <div class="orbital-core">
            <svg viewBox="0 0 160 160">
              <defs>
                <linearGradient id="globe-color" x2="1" y2="1">
                  <stop stop-color="#f9f8ff" />
                  <stop offset="1" stop-color="#d8ceef" />
                </linearGradient>
              </defs>
              <circle
                cx="80"
                cy="80"
                r="70"
                fill="url(#globe-color)"
                stroke="#fff"
                stroke-width="2"
              />
              <g stroke="#b4a1d6" fill="none" opacity=".65" stroke-width="1">
                <ellipse cx="80" cy="80" rx="30" ry="70" />
                <ellipse cx="80" cy="80" rx="53" ry="70" />
                <ellipse cx="80" cy="80" rx="70" ry="25" />
                <ellipse cx="80" cy="80" rx="70" ry="48" />
                <path d="M10 80h140M80 10v140" />
              </g></svg
            ><span class="orbit-lock"
              ><AppIcon name="shield" :size="27"
            /></span>
          </div>
          <span class="orbit-dot d1" /><span class="orbit-dot d2" /><span
            class="orbit-dot d3"
          /><span class="orbit-tag tag1"
            ><AppIcon name="globe" :size="15" />全球精选线路</span
          ><span class="orbit-tag tag2"
            ><AppIcon name="lightning" :size="15" />一键，自由相连</span
          >
        </div>
        <div class="auth-values">
          <span><AppIcon name="shield" :size="15" />安全连接</span><i /><span
            ><AppIcon name="globe" :size="15" />全平台体验</span
          ><i /><span><AppIcon name="headphones" :size="15" />贴心支持</span>
        </div>
      </section>
      <section class="auth-form-area">
        <slot />
        <p v-if="!isDesktop && !isPreview" class="browser-note">
          当前为浏览器界面，请使用桌面客户端登录连接。
        </p>
        <a
          v-if="development && !isDesktop && !isPreview"
          href="/?preview=1#/"
          class="preview-link"
          >查看界面预览（示例数据）<AppIcon name="arrow" :size="14"
        /></a>
      </section>
    </div>
    <footer>
      © {{ new Date().getFullYear() }} Sufe <span>轻松连接，尽享可能。</span>
    </footer>
  </div>
</template>
<style scoped>
.auth-layout {
  min-height: 100vh;
  background: var(--surface);
  display: flex;
  flex-direction: column;
}
.auth-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 32px 6vw;
  gap: 20px;
}
.auth-header > a {
  display: flex;
  align-items: center;
  gap: 11px;
  color: var(--text);
}
.auth-header > span {
  font-size: 11px;
  color: var(--muted);
  letter-spacing: 1px;
}
.auth-body {
  display: grid;
  grid-template-columns: 1.1fr 1fr;
  flex: 1;
  align-items: center;
  max-width: 1220px;
  width: 100%;
  margin: 0 auto;
  padding: 20px 55px 35px;
  gap: 80px;
}
.auth-story {
  position: relative;
  padding-top: 15px;
}
.auth-story .eyebrow {
  font-size: 9px;
  color: #b1a5cb;
  letter-spacing: 2px;
}
.auth-story h1 {
  font-size: 43px;
  line-height: 1.45;
  letter-spacing: -1.6px;
  margin: 18px 0 18px;
  font-weight: 650;
}
.auth-story h1 > span {
  color: #9d8ed5;
}
.auth-story > p {
  font-size: 12px;
  color: var(--muted);
  line-height: 2.2;
}
.orbit-art {
  height: 285px;
  position: relative;
  width: 390px;
  margin: 2px 0 4px -30px;
}
.orbit {
  border: 1px solid #b9a6d528;
  border-radius: 50%;
  position: absolute;
  left: 50%;
  top: 50%;
  transform: translate(-50%, -50%) rotate(-28deg);
}
.o1 {
  width: 345px;
  height: 170px;
}
.o2 {
  width: 290px;
  height: 270px;
}
.o3 {
  width: 220px;
  height: 285px;
}
.orbital-core {
  width: 169px;
  height: 169px;
  position: absolute;
  top: 53px;
  left: 110px;
  filter: drop-shadow(0 12px 19px #8770b524);
}
.orbit-lock {
  position: absolute;
  right: 3px;
  bottom: 7px;
  width: 49px;
  height: 49px;
  border-radius: 15px;
  background: #967ed3;
  color: #fff;
  border: 3px solid white;
  display: flex;
  align-items: center;
  justify-content: center;
  box-shadow: 0 5px 15px #b4a5d724;
}
.orbit-tag {
  display: flex;
  align-items: center;
  gap: 8px;
  position: absolute;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 9px;
  padding: 11px 14px;
  box-shadow: 0 8px 25px #51426907;
  font-size: 10px;
  color: #9689ae;
  white-space: nowrap;
}
.tag1 {
  top: 35px;
  right: -2px;
}
.tag2 {
  bottom: 36px;
  left: 6px;
}
.orbit-dot {
  height: 6px;
  width: 6px;
  border-radius: 50%;
  background: #b1a0d5;
  position: absolute;
  border: 3px solid #f3effb;
  box-sizing: content-box;
}
.d1 {
  top: 26px;
  left: 138px;
}
.d2 {
  top: 129px;
  left: 30px;
  width: 4px;
  height: 4px;
}
.d3 {
  bottom: 46px;
  right: 87px;
  width: 5px;
  height: 5px;
}
.auth-values {
  display: flex;
  gap: 17px;
  align-items: center;
  color: var(--muted);
  font-size: 10px;
}
.auth-values > span {
  display: flex;
  gap: 7px;
  align-items: center;
}
.auth-values svg {
  color: #a295bb;
}
.auth-values i {
  height: 10px;
  width: 1px;
  background: var(--border);
}
.auth-form-area {
  padding: 34px 16px;
  border: 1px solid var(--border);
  border-radius: 24px;
  background: var(--surface);
  box-shadow: 0 10px 70px #3a326506;
  min-width: 0;
  max-width: 460px;
}
.auth-form-area :deep(.login-shell),
.auth-form-area :deep(.auth-shell) {
  min-height: 0;
  background: none;
  padding: 0;
  display: block;
}
.auth-form-area :deep(.n-card) {
  background: none;
  border: 0;
  border-radius: 0;
  box-shadow: none;
  backdrop-filter: none;
  max-width: none;
}
.auth-form-area :deep(.login-head),
.auth-form-area :deep(.auth-head) {
  text-align: left;
}
.auth-form-area :deep(.brand) {
  font-size: 24px;
  letter-spacing: -0.6px;
  margin-bottom: 8px;
}
.auth-form-area :deep(.n-divider) {
  margin: 26px 0;
}
.auth-form-area :deep(.n-form-item-label) {
  font-size: 12px;
}
.auth-form-area :deep(.n-form-item) {
  margin-bottom: 2px;
}
.auth-form-area :deep(.n-text) {
  font-size: 12px;
}
.auth-form-area :deep(.n-button--primary-type) {
  height: 44px;
  font-size: 13px;
}
.auth-form-area :deep(.n-space .n-button--text-type) {
  font-size: 11px;
}
.browser-note {
  font-size: 10px;
  color: var(--muted);
  text-align: center;
  margin: 9px 10px;
}
.preview-link {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  font-size: 10px;
  margin-top: 12px;
}
.auth-layout footer {
  padding: 24px 6vw;
  font-size: 10px;
  color: var(--muted);
  display: flex;
  justify-content: space-between;
}
@media (max-width: 1000px) {
  .auth-body {
    gap: 35px;
    padding: 20px 40px 30px;
  }
  .auth-story h1 {
    font-size: 34px;
  }
  .orbit-art {
    transform: scale(0.88);
    transform-origin: left center;
    width: 350px;
  }
  .auth-values {
    gap: 11px;
    font-size: 9px;
  }
  .auth-form-area {
    padding: 22px 5px;
  }
}
@media (max-width: 760px) {
  .auth-header {
    padding: 22px 24px;
  }
  .auth-header > span {
    display: none;
  }
  .auth-story {
    display: none;
  }
  .auth-body {
    display: flex;
    justify-content: center;
    padding: 20px 18px;
    align-items: center;
  }
  .auth-form-area {
    width: 100%;
    padding: 22px 4px;
    border: 0;
    box-shadow: none;
  }
  .auth-layout footer {
    padding: 24px;
    font-size: 9px;
  }
  .auth-form-area :deep(.n-card__content) {
    padding: 18px;
  }
  .auth-form-area :deep(.brand) {
    font-size: 27px;
  }
}
</style>
