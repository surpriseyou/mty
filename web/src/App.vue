<template>
  <router-view v-if="$route.path === '/login'" />
  <el-container v-else class="shell">
    <el-aside width="240px" class="sidebar">
      <div class="brand">
        <span class="brand-mark">M</span>
        <div>
          <strong>MTY Registry</strong>
          <small>tool supply console</small>
        </div>
      </div>
      <el-menu router :default-active="$route.path" class="nav">
        <el-menu-item index="/packages">
          <el-icon><Box /></el-icon>
          <span>Packages</span>
        </el-menu-item>
        <el-menu-item index="/upload">
          <el-icon><Upload /></el-icon>
          <span>Upload</span>
        </el-menu-item>
        <el-menu-item index="/audit">
          <el-icon><Document /></el-icon>
          <span>Audit</span>
        </el-menu-item>
      </el-menu>
    </el-aside>
    <el-container>
      <el-header class="topbar">
        <span>Package operations</span>
        <el-button text @click="logout">Sign out</el-button>
      </el-header>
      <el-main>
        <router-view />
      </el-main>
    </el-container>
  </el-container>
</template>

<script setup lang="ts">
import { Box, Document, Upload } from '@element-plus/icons-vue'
import { useRouter } from 'vue-router'

const router = useRouter()

function logout() {
  localStorage.removeItem('mty-token')
  router.push('/login')
}
</script>

