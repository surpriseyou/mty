<template>
  <section class="page">
    <div class="page-title">
      <div>
        <h1>{{ name }}</h1>
        <p>{{ detail?.description ?? 'Package details' }}</p>
      </div>
      <div class="title-actions">
        <el-button type="primary" @click="openNewPackage">新增package</el-button>
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
        <el-table-column label="Actions" width="460">
          <template #default="{ row }">
            <el-button size="small" @click="downloadVersion(row)">Download</el-button>
            <el-button size="small" type="success" @click="publish(row)">Publish</el-button>
            <el-button size="small" @click="unpublish(row)">Unpublish</el-button>
            <el-button size="small" type="primary" @click="resign(row)">Re-sign</el-button>
            <el-button size="small" type="danger" @click="deleteVersion(row)">Delete</el-button>
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
type VersionRow = Detail['versions'][number]

async function load() {
  loading.value = true
  try {
    const { data } = await api.get<Detail>(`/api/admin/packages/${name}`)
    detail.value = data
  } finally {
    loading.value = false
  }
}

function versionTargetParams(version: VersionRow) {
  return {
    platform: version.platform,
    arch: version.arch
  }
}

async function publish(version: VersionRow) {
  await api.post(`/api/admin/packages/${name}/versions/${version.version}/publish`, null, {
    params: versionTargetParams(version)
  })
  ElMessage.success('Version published')
  await load()
}

async function unpublish(version: VersionRow) {
  await api.post(`/api/admin/packages/${name}/versions/${version.version}/unpublish`, null, {
    params: versionTargetParams(version)
  })
  ElMessage.success('Version unpublished')
  await load()
}

async function resign(version: VersionRow) {
  await api.post(`/api/admin/packages/${name}/versions/${version.version}/resign`, null, {
    params: versionTargetParams(version)
  })
  ElMessage.success('Version signed')
  await load()
}

async function downloadVersion(version: VersionRow) {
  const response = await api.get(`/api/admin/packages/${name}/versions/${version.version}/download`, {
    params: versionTargetParams(version),
    responseType: 'blob'
  })
  const blobUrl = URL.createObjectURL(response.data)
  const link = document.createElement('a')
  link.href = blobUrl
  link.download = `${name}-${version.version}-${version.platform}-${version.arch}.mty`
  document.body.appendChild(link)
  link.click()
  link.remove()
  URL.revokeObjectURL(blobUrl)
  await load()
}

function openNewPackage() {
  router.push({
    path: '/packages/new',
    query: {
      name,
      description: detail.value?.description ?? ''
    }
  })
}

async function deleteVersion(version: VersionRow) {
  await ElMessageBox.confirm(
    `Delete version ${version.version} (${version.platform}/${version.arch})?`,
    'Delete version',
    {
      type: 'warning',
      confirmButtonText: 'Delete',
      cancelButtonText: 'Cancel'
    }
  )
  await api.delete(`/api/admin/packages/${name}/versions/${version.version}`, {
    params: versionTargetParams(version)
  })
  ElMessage.success('Version deleted')
  await load()
}

onMounted(load)
</script>

<style scoped>
.title-actions {
  display: flex;
  gap: 8px;
}
</style>
