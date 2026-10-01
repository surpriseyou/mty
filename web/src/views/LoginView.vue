<template>
  <main class="login-page">
    <div class="login-brand">
      <div class="brand"><span class="brand-mark">M</span><strong>MTY 工具仓库</strong></div>
      <h1>让工具管理<br />与版本发布更清晰</h1>
      <p>集中管理内部工具包，上传、签名与发布版本。</p>
    </div>
    <section class="login-panel">
      <div class="page-title"><div><h1>登录控制台</h1><p>使用管理员账户继续操作。</p></div></div>
      <el-form label-position="top" @submit.prevent="login">
        <el-form-item label="用户名"><el-input v-model="username" aria-label="用户名" placeholder="请输入用户名" autocomplete="username" :disabled="loading" /></el-form-item>
        <el-form-item label="密码"><el-input v-model="password" aria-label="密码" type="password" placeholder="请输入密码" autocomplete="current-password" show-password :disabled="loading" /></el-form-item>
        <el-button type="primary" :loading="loading" native-type="submit">登录</el-button>
      </el-form>
    </section>
  </main>
</template>
<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { ElMessage } from 'element-plus'
import { api } from '../api'
const router = useRouter()
const username = ref('')
const password = ref('')
const loading = ref(false)
async function login() {
  if (loading.value) return
  if (!username.value.trim() || !password.value) { ElMessage.warning('请输入用户名和密码'); return }
  loading.value = true
  try {
    const { data } = await api.post('/api/auth/login', { username: username.value.trim(), password: password.value })
    localStorage.setItem('mty-token', data.token)
    await router.push('/')
  } catch { /* 请求层统一显示中文错误。 */ }
  finally { loading.value = false }
}
</script>
