<template>
  <section class="page">
    <div class="page-title">
      <div>
        <h1>{{ name }}</h1>
        <p>{{ detail?.description ?? 'Package details' }}</p>
      </div>
      <div class="title-actions">
        <el-button @click="$router.push('/upload')">Upload version</el-button>
        <el-button type="danger" @click="deletePackage">Delete package</el-button>
      </div>
    </div>
    <div class="panel">
      <el-table :data="detail?.versions ?? []" v-loading="loading">
        <el-table-column prop="version" label="Version" width="140" />
        <el-table-column label="Target" width="160">
          <template #default="{ row }">{{ row.platform }} / {{ row.arch }}</template>
        </el-table-column>
        <el-table-column prop="sha256" label="SHA256" min-width="260" show-overflow-tooltip />
        <el-table-column prop="signature" label="Signature" min-width="220" show-overflow-tooltip />
        <el-table-column prop="status" label="Status" width="130" />
        <el-table-column prop="downloadCount" label="Downloads" width="120" />
        <el-table-column label="Actions" width="300">
          <template #default="{ row }">
            <el-button size="small" type="success" @click="publish(row.version)">Publish</el-button>
            <el-button size="small" @click="unpublish(row.version)">Unpublish</el-button>
            <el-button size="small" type="primary" @click="resign(row.version)">Re-sign</el-button>
          </template>
        </el-table-column>
      </el-table>
    </div>
  </section>
</template>

<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { ElMessage, ElMessageBox } from 'element-plus'
import { api, type PackageVersion } from '../api'

interface Detail {
  name: string
  description: string
  versions: Array<PackageVersion & { status: string; downloadCount: number }>
}

const route = useRoute()
const router = useRouter()
const name = String(route.params.name)
const detail = ref<Detail | null>(null)
const loading = ref(false)

async function load() {
  loading.value = true
  try {
    const { data } = await api.get<Detail>(`/api/admin/packages/${name}`)
    detail.value = data
  } finally {
    loading.value = false
  }
}

async function publish(version: string) {
  await api.post(`/api/admin/packages/${name}/versions/${version}/publish`)
  ElMessage.success('Version published')
  await load()
}

async function unpublish(version: string) {
  await api.post(`/api/admin/packages/${name}/versions/${version}/unpublish`)
  ElMessage.success('Version unpublished')
  await load()
}

async function resign(version: string) {
  await api.post(`/api/admin/packages/${name}/versions/${version}/resign`)
  ElMessage.success('Version signed')
  await load()
}

async function deletePackage() {
  await ElMessageBox.confirm(`Delete package ${name} and all versions?`, 'Delete package', {
    type: 'warning',
    confirmButtonText: 'Delete',
    cancelButtonText: 'Cancel'
  })
  await api.delete(`/api/admin/packages/${name}`)
  ElMessage.success('Package deleted')
  router.push('/packages')
}

onMounted(load)
</script>

<style scoped>
.title-actions {
  display: flex;
  gap: 8px;
}
</style>
