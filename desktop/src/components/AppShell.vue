<script setup lang="ts">
import { computed, ref, onMounted, onUnmounted, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useDialog, useMessage } from 'naive-ui';
import AppIcon from './AppIcon.vue';
import { useAuthStore } from '@/stores/auth';
import { useConnectionStore } from '@/stores/connection';
import { useThemeStore } from '@/stores/theme';
import { useFeaturesStore, type Feature } from '@/stores/features';
import { usePlanStore } from '@/stores/plan';
import { useSupportStore } from '@/stores/support';
import { isPreview } from '@/platform';
import { bytes } from '@/utils/format';
const auth=useAuthStore(), conn=useConnectionStore(), theme=useThemeStore(), features=useFeaturesStore(), plans=usePlanStore();
const route=useRoute(), router=useRouter(), dialog=useDialog(), message=useMessage();
const mobileOpen=ref(false);
const support=useSupportStore();
watch(()=>[auth.session?.email,features.config?.chatwoot_base_url,features.config?.chatwoot_inbox_identifier,features.enabled('chatwoot')],()=>{
  if(auth.session&&features.enabled('chatwoot'))void support.start();else support.stop();
},{immediate:true});
const navigation: {label:string;icon:string;path:string;feature?:Feature}[]=[
  {label:'总览',icon:'grid',path:'/'},{label:'节点',icon:'globe',path:'/nodes'},
  {label:'订阅套餐',icon:'bag',path:'/plans',feature:'purchase'},
  {label:'我的订单',icon:'card',path:'/orders'},
  {label:'分流规则',icon:'rules',path:'/rules',feature:'custom_rules'},
];
const tools=[{label:'连接监控',icon:'activity',path:'/connections'},{label:'运行日志',icon:'terminal',path:'/logs'},{label:'设置',icon:'settings',path:'/settings'}];
const used=computed(()=> (auth.subscribe?.u||0)+(auth.subscribe?.d||0));
const total=computed(()=> auth.subscribe?.transfer_enable||0);
const percent=computed(()=>total.value?Math.min(100,used.value/total.value*100):0);
const active=(path:string)=>path==='/'?route.path==='/':route.path.startsWith(path);
const labels: Record<string,string>={'/':'总览','/nodes':'节点','/plans':'订阅套餐','/orders':'我的订单','/rules':'分流规则','/connections':'连接监控','/logs':'运行日志','/settings':'设置','/notices':'公告中心','/account':'账户中心','/support':'在线客服','/tickets':'服务工单'};
watch(()=>route.path,()=>mobileOpen.value=false);
let timer: number|undefined;
onMounted(()=>{ void conn.hydrate().catch(()=>undefined); void features.ensure(); void plans.ensure(); timer=window.setInterval(()=>void features.ensure(true),60000); });
onUnmounted(()=>{clearInterval(timer);support.stop();});
async function signOut(){
  try { if(conn.isConnected||conn.isBusy) await conn.disconnect(); await auth.logout(); await router.replace('/login'); }
  catch {message.error('退出未完成，请重试。');}
}
function logout(){dialog.warning({title:'退出当前账户',content:'退出后将断开连接，你可以随时重新登录。',positiveText:'退出登录',negativeText:'取消',onPositiveClick:signOut});}
</script>
<template>
  <div class="app-layout">
    <button v-if="mobileOpen" class="nav-overlay" aria-label="关闭导航" @click="mobileOpen=false" />
    <aside class="sidebar" :class="{open:mobileOpen}">
      <RouterLink to="/" class="brand"><span class="brand-symbol"><AppIcon name="lightning" :size="23" /></span><span class="brand-word">{{features.config?.brand_name||'Sufe'}}<small>速飞</small></span></RouterLink>
      <div class="sidebar-label">工作空间</div>
      <nav aria-label="主导航"><template v-for="item in navigation" :key="item.path"><RouterLink v-if="!item.feature||features.enabled(item.feature)" :to="item.path" :class="{active:active(item.path)}" :aria-current="active(item.path)?'page':undefined"><AppIcon :name="item.icon" :size="19" /><span>{{item.label}}</span><span v-if="active(item.path)" class="nav-dot" /></RouterLink></template></nav>
      <div class="sidebar-label second">偏好与工具</div>
      <nav aria-label="工具导航"><RouterLink v-for="item in tools" :key="item.path" :to="item.path" :class="{active:active(item.path)}"><AppIcon :name="item.icon" :size="19" /><span>{{item.label}}</span></RouterLink></nav>
      <div class="sidebar-bottom">
        <div v-if="features.enabled('purchase')" class="subscription-mini"><div><AppIcon name="lightning" :size="16" /><span>{{plans.nameFor(auth.subscribe?.plan_id)||'我的订阅'}}</span><span class="mini-badge">{{auth.subscribe?.plan_id?'已订阅':'未开通'}}</span></div><p>本期已用 <strong>{{bytes(used)}} / {{bytes(total)}}</strong></p><div class="mini-track"><i :style="{width:percent+'%'}" /></div><RouterLink to="/plans">{{auth.subscribe?.plan_id?'管理我的订阅':'选择订阅计划'}}<AppIcon name="arrow" :size="15" /></RouterLink></div>
        <RouterLink v-if="features.enabled('chatwoot')||features.enabled('tickets')" :to="features.enabled('chatwoot')?'/support':'/tickets'" class="help-link"><AppIcon name="headphones" :size="18" /><span>帮助与支持</span><AppIcon name="chevron" :size="14" /></RouterLink>
        <div class="sidebar-account"><RouterLink to="/account" class="account-link"><span class="avatar">{{(auth.session?.email?.[0]||'S').toUpperCase()}}</span><div><strong>{{auth.session?.email?.split('@')[0]||'我的账户'}}</strong><small>管理账户与权益</small></div></RouterLink><button class="logout-button" title="退出登录" aria-label="退出登录" @click="logout"><AppIcon name="logout" :size="17" /></button></div>
      </div>
    </aside>
    <div class="workspace">
      <header class="workspace-header"><div class="breadcrumb"><button class="mobile-menu icon-button" aria-label="打开导航" @click="mobileOpen=!mobileOpen"><AppIcon name="menu" /></button><span class="breadcrumb-root">工作空间</span><span class="slash">/</span><strong>{{labels[route.path]||'服务工单'}}</strong></div><div class="header-actions"><span class="connection-label" :class="{online:conn.isConnected}"><i class="status-dot" />{{conn.isConnected?'连接已建立':conn.isBusy?'正在连接':'尚未连接'}}</span><span class="header-divider" /><button class="header-icon" :aria-label="theme.dark?'切换浅色主题':'切换深色主题'" @click="theme.toggle"><AppIcon :name="theme.dark?'sun':'moon'" :size="19" /></button><RouterLink v-if="features.enabled('notice')" class="header-icon" to="/notices" aria-label="公告中心"><AppIcon name="bell" :size="19" /></RouterLink><RouterLink to="/account" class="avatar small" aria-label="账户中心">{{(auth.session?.email?.[0]||'S').toUpperCase()}}</RouterLink></div></header>
      <div v-if="isPreview" class="preview-banner"><AppIcon name="info" :size="14" />界面预览 · 当前为示例数据，不建立真实网络连接或订单。</div>
      <RouterLink v-if="features.enabled('chatwoot')&&support.unread&&route.path!=='/support'" to="/support" class="support-notice"><AppIcon name="chat" :size="18"/><span>客服有 {{support.unread}} 条新消息<small>{{support.latestNotice||'点击查看会话'}}</small></span><AppIcon name="chevron" :size="15"/></RouterLink>
      <main id="main-content" class="workspace-content"><RouterView /></main>
      <footer class="workspace-footer"><span><i class="status-dot" /> Sufe · 让连接，自由一点</span><span>Powered by mihomo <i>·</i> v0.1.0</span></footer>
    </div>
  </div>
</template>
<style scoped>
.support-notice{position:fixed;right:28px;bottom:26px;z-index:30;display:flex;align-items:center;gap:13px;padding:17px 21px;border:1px solid #b7a6ee;border-radius:14px;background:var(--surface);color:var(--primary);box-shadow:0 8px 35px #55427420;font-size:12px;max-width:calc(100vw - 36px)}.support-notice small{display:block;font-size:10px;color:var(--muted);margin-top:4px;max-width:240px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.app-layout{display:flex;min-height:100vh}.sidebar{width:222px;position:fixed;inset:0 auto 0 0;background:var(--surface);border-right:1px solid var(--border);padding:32px 18px 0;display:flex;flex-direction:column;z-index:40}.brand{display:flex;align-items:center;gap:10px;color:var(--text);padding:0 10px;margin-bottom:43px}.sidebar-label{font-size:10px;letter-spacing:1.2px;color:var(--muted);padding-left:16px;margin-bottom:12px}.sidebar-label.second{margin-top:33px}.sidebar nav{display:flex;flex-direction:column;gap:7px}.sidebar nav a{padding:12px 16px;display:flex;align-items:center;gap:13px;color:#7d8093;font-size:13px;border-radius:10px;position:relative}.sidebar nav a:hover{background:var(--surface-soft);color:var(--primary)}.sidebar nav a.active{background:var(--lavender);color:var(--primary);font-weight:600}.nav-dot{margin-left:auto;width:5px;height:5px;border-radius:50%;background:var(--primary)}.sidebar-bottom{margin-top:auto;padding-top:36px}.subscription-mini{background:var(--surface-soft);padding:16px 13px;border:1px solid var(--border);border-radius:12px}.subscription-mini>div:first-child{display:flex;align-items:center;gap:6px;font-size:12px;font-weight:600}.subscription-mini svg{color:var(--primary)}.mini-badge{font-size:9px;margin-left:auto;color:var(--primary);font-weight:400;background:var(--lavender);padding:1px 4px;border-radius:3px}.subscription-mini p{font-size:9px;color:var(--muted);margin:14px 0 8px}.subscription-mini p strong{font-weight:400;float:right;font-size:9px}.mini-track{height:4px;background:var(--border);border-radius:3px;overflow:hidden}.mini-track i{height:100%;display:block;background:var(--primary);border-radius:3px}.subscription-mini>a{display:flex;justify-content:space-between;align-items:center;margin-top:14px;font-size:11px}.help-link{display:flex;align-items:center;gap:10px;padding:22px 13px;color:var(--muted);font-size:12px}.help-link svg:last-child{margin-left:auto}.sidebar-account{display:flex;align-items:center;gap:8px;border-top:1px solid var(--border);padding:20px 4px}.account-link{display:flex;gap:9px;align-items:center;color:var(--text);min-width:0;flex:1}.account-link>div{min-width:0}.account-link strong{display:block;font-size:12px;font-weight:600;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.account-link small{display:block;font-size:10px;color:var(--muted);margin-top:2px}.avatar{width:35px;height:35px;border-radius:50%;background:#e9e5f8;color:#8573c1;display:inline-flex;align-items:center;justify-content:center;font-size:14px;font-weight:650;flex-shrink:0}.avatar.small{width:30px;height:30px;font-size:12px;margin-left:8px}.logout-button{padding:6px;background:none;border:0;color:var(--muted)}.workspace{margin-left:222px;min-width:0;flex:1;display:flex;flex-direction:column}.workspace-header{height:80px;display:flex;align-items:center;justify-content:space-between;padding:0 38px;border-bottom:1px solid var(--border);background:var(--surface);gap:12px}.breadcrumb{display:flex;gap:15px;align-items:center;font-size:12px}.breadcrumb-root,.slash{color:var(--muted)}.breadcrumb strong{font-weight:500}.header-actions{display:flex;gap:21px;align-items:center}.connection-label{font-size:11px;color:var(--muted);display:flex;align-items:center;gap:7px}.connection-label.online{color:var(--green)}.header-divider{height:16px;width:1px;background:var(--border)}.header-icon{padding:0;border:0;background:none;color:var(--muted);display:inline-flex}.workspace-content{padding:34px 38px 20px;max-width:1480px;width:100%;margin:0 auto;flex:1;min-width:0}.workspace-footer{display:flex;justify-content:space-between;padding:20px 38px 22px;color:var(--muted);font-size:10px;gap:10px}.workspace-footer span{display:flex;align-items:center;gap:6px}.workspace-footer .status-dot{width:4px;height:4px;color:var(--green)}.workspace-footer i{font-style:normal;margin:0 4px}.preview-banner{font-size:11px;padding:6px 38px;background:var(--lavender);color:var(--primary);display:flex;align-items:center;gap:7px}.mobile-menu{display:none}.nav-overlay{position:fixed;inset:0;background:#11142c50;z-index:39;border:0}
@media(min-width:1550px){.sidebar{width:240px;padding-left:24px;padding-right:24px}.workspace{margin-left:240px}.workspace-content{padding-top:40px}}
@media(max-height:810px) and (min-width:761px){.sidebar{padding-top:22px}.brand{margin-bottom:26px}.sidebar nav a{padding:10px 16px}.sidebar-label.second{margin-top:22px}.sidebar-bottom{padding-top:22px}.subscription-mini{padding:12px}.help-link{padding:16px 13px}.sidebar-account{padding:14px 4px}}
@media(max-width:1100px){.workspace-header{padding:0 26px}.workspace-content{padding:28px 26px 16px}.workspace-footer{padding:18px 26px}.header-actions{gap:15px}.sidebar{width:200px;padding-left:13px;padding-right:13px}.workspace{margin-left:200px}.brand-word small{display:none}}
@media(max-width:760px){.sidebar{transform:translateX(-100%);width:240px;padding:26px 18px 0;transition:transform .2s}.sidebar.open{transform:translateX(0)}.workspace{margin-left:0}.workspace-header{height:66px;padding:0 20px}.workspace-content{padding:24px 18px 16px}.workspace-footer{padding:18px;font-size:9px}.breadcrumb-root,.slash,.header-divider{display:none}.breadcrumb{gap:12px}.mobile-menu{display:flex;width:30px;height:30px;border:0;background:none}.header-actions{gap:15px}.connection-label{font-size:10px}.header-icon{display:none}.preview-banner{padding:7px 20px;font-size:10px}}
</style>
