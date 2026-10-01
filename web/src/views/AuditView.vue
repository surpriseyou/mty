<template>
  <section class="page">
    <div class="page-title"><div><h1>审计日志</h1><p>查看近期管理员操作，追踪工具包与版本变更。</p></div><el-button :icon="Refresh" :loading="loading" @click="load">刷新</el-button></div>
    <div class="panel" v-loading="loading">
      <el-empty v-if="failed" description="加载失败，请刷新重试" />
      <div v-else class="table-scroll">
        <el-table :data="logs" style="min-width: 800px">
          <el-table-column label="操作时间" width="210"><template #default="{ row }">{{ formatTime(row.createdAt) }}</template></el-table-column>
          <el-table-column prop="actor" label="操作人" width="140" show-overflow-tooltip />
          <el-table-column label="操作" width="160"><template #default="{ row }">{{ auditAction(row.action) }}</template></el-table-column>
          <el-table-column label="目标类型" width="150"><template #default="{ row }">{{ targetLabel(row.targetType) }}</template></el-table-column>
          <el-table-column prop="target" label="操作对象" min-width="220" show-overflow-tooltip />
          <template #empty><el-empty description="暂无操作记录" /></template>
        </el-table>
      </div>
      <p v-if="!failed" class="count-label">共 {{ logs.length }} 条记录</p>
    </div>
  </section>
</template>
<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { Refresh } from '@element-plus/icons-vue'
import { api, type AuditLog } from '../api'
import { auditAction, targetLabel, formatTime } from '../ui'
const logs = ref<AuditLog[]>([])
const loading = ref(false)
const failed = ref(false)
async function load() {
  if (loading.value) return
  loading.value = true; failed.value = false
  try { logs.value = (await api.get<AuditLog[]>('/api/admin/audit-logs')).data }
  catch { failed.value = true }
  finally { loading.value = false }
}
onMounted(load)
</script>
