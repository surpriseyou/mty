<template>
  <main class="login-page">
    <section class="login-panel">
      <div class="page-title">
        <div>
          <h1>MTY Admin</h1>
          <p>Sign in to maintain tool releases.</p>
        </div>
      </div>
      <el-form label-position="top" @submit.prevent="login">
        <el-form-item label="Username">
          <el-input v-model="username" autocomplete="username" />
        </el-form-item>
        <el-form-item label="Password">
          <el-input v-model="password" type="password" autocomplete="current-password" show-password />
        </el-form-item>
        <el-button type="primary" :loading="loading" native-type="submit" class="full">Sign in</el-button>
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
const username = ref('admin')
const password = ref('ChangeMe123!')
const loading = ref(false)

async function login() {
  loading.value = true
  try {
    const { data } = await api.post('/api/auth/login', { username: username.value, password: password.value })
    localStorage.setItem('mty-token', data.token)
    router.push('/packages')
  } catch {
    ElMessage.error('Invalid username or password')
  } finally {
    loading.value = false
  }
}
</script>

<style scoped>
.full {
  width: 100%;
}
</style>
