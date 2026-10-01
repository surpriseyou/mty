<template>
  <router-view v-if="route.path === '/login'" />
  <el-container v-else class="shell" :class="{ 'nav-collapsed': collapsed }">
    <aside class="sidebar desktop-nav">
      <div class="brand"><span class="brand-mark">M</span><strong v-if="!collapsed">MTY 工具仓库</strong></div>
      <el-menu router :default-active="activeNav" :collapse="collapsed" class="nav">
        <el-menu-item index="/"><el-icon><House /></el-icon><template #title>首页概览</template></el-menu-item>
        <el-menu-item index="/packages"><el-icon><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"><path d="m12 2 9 5v10l-9 5-9-5V7Zm0 0L3 7l9 5 9-5M12 12v10" /></svg></el-icon><template #title>工具包管理</template></el-menu-item>
        <el-menu-item index="/audit"><el-icon><Document /></el-icon><template #title>审计日志</template></el-menu-item>
      </el-menu>
    </aside>
    <el-drawer v-model="mobileNav" direction="ltr" size="260px" title="MTY 工具仓库">
      <el-menu router :default-active="activeNav" @select="mobileNav = false">
        <el-menu-item index="/"><el-icon><House /></el-icon>首页概览</el-menu-item>
        <el-menu-item index="/packages"><el-icon><Box /></el-icon>工具包管理</el-menu-item>
        <el-menu-item index="/audit"><el-icon><Document /></el-icon>审计日志</el-menu-item>
      </el-menu>
    </el-drawer>
    <el-container class="workspace">
      <el-header class="topbar">
        <div class="header-location">
          <el-button text class="desktop-toggle" :aria-label="collapsed ? '展开导航' : '折叠导航'" @click="collapsed = !collapsed"><el-icon><Fold v-if="!collapsed" /><Expand v-else /></el-icon></el-button>
          <el-button text class="mobile-toggle" aria-label="打开导航" @click="mobileNav = true"><el-icon><Expand /></el-icon></el-button>
          <el-breadcrumb separator="/"><el-breadcrumb-item :to="{ path: activeNav }">{{ navTitle }}</el-breadcrumb-item><el-breadcrumb-item v-if="subTitle">{{ subTitle }}</el-breadcrumb-item></el-breadcrumb>
        </div>
        <el-button text @click="logout"><el-icon><SwitchButton /></el-icon><span>退出登录</span></el-button>
      </el-header>
      <el-main><router-view :key="route.fullPath" /></el-main>
    </el-container>
  </el-container>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import { Box, Document, Fold, Expand, SwitchButton, House } from '@element-plus/icons-vue'
import { useRoute, useRouter } from 'vue-router'
const route = useRoute()
const router = useRouter()
const collapsed = ref(false)
const mobileNav = ref(false)
const activeNav = computed(() => route.path === '/' ? '/' : route.path === '/audit' ? '/audit' : '/packages')
const navTitle = computed(() => activeNav.value === '/' ? '首页概览' : activeNav.value === '/audit' ? '审计日志' : '工具包管理')
const subTitle = computed(() => route.meta.editPackage ? '编辑工具包' : route.path.endsWith('/upload') ? String(route.params.name) + ' / 上传版本' : route.path === '/packages/new' ? '新建工具包' : route.params.name ? String(route.params.name) : '')
function logout() {
  localStorage.removeItem('mty-token')
  router.push('/login')
}
</script>
