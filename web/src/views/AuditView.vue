<template>
  <section class="page">
    <div class="page-title">
      <div>
        <h1>Audit</h1>
        <p>Recent administrator operations.</p>
      </div>
      <el-button :loading="loading" @click="load">Refresh</el-button>
    </div>
    <div class="panel">
      <el-table :data="logs" v-loading="loading">
        <el-table-column prop="createdAt" label="Time" width="230" />
        <el-table-column prop="actor" label="Actor" width="140" />
        <el-table-column prop="action" label="Action" width="170" />
        <el-table-column prop="targetType" label="Target type" width="150" />
        <el-table-column prop="target" label="Target" min-width="220" />
      </el-table>
    </div>
  </section>
</template>

<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { api, type AuditLog } from '../api'

const logs = ref<AuditLog[]>([])
const loading = ref(false)

async function load() {
  loading.value = true
  try {
    const { data } = await api.get<AuditLog[]>('/api/admin/audit-logs')
    logs.value = data
  } finally {
    loading.value = false
  }
}

onMounted(load)
</script>

