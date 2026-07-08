<template>
  <section class="page">
    <div class="page-title">
      <div>
        <h1>Packages</h1>
        <p>Search, inspect, and prepare internal tool releases.</p>
      </div>
      <el-button type="primary" @click="$router.push('/packages/new')">新增package</el-button>
    </div>
    <div class="panel">
      <div class="toolbar">
        <el-input v-model="keyword" placeholder="Search packages" clearable @keyup.enter="load" />
        <el-button :loading="loading" @click="load">Search</el-button>
      </div>
      <el-table :data="packages" v-loading="loading" @row-click="openPackage">
        <el-table-column prop="name" label="Name" min-width="180" />
        <el-table-column prop="description" label="Description" min-width="280" />
        <el-table-column prop="versionCount" label="Versions" width="110" />
        <el-table-column prop="latestVersion" label="Latest" width="140">
          <template #default="{ row }">{{ row.latestVersion ?? '-' }}</template>
        </el-table-column>
        <el-table-column label="Actions" width="160">
          <template #default="{ row }">
            <el-button size="small" @click.stop="editPackage(row)">Edit</el-button>
            <el-button size="small" type="danger" @click.stop="deletePackage(row)">Delete</el-button>
          </template>
        </el-table-column>
      </el-table>
    </div>
  </section>
</template>

<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { ElMessage, ElMessageBox } from 'element-plus'
import { api, type AdminPackage } from '../api'

const router = useRouter()
const packages = ref<AdminPackage[]>([])
const keyword = ref('')
const loading = ref(false)

async function load() {
  loading.value = true
  try {
    const { data } = await api.get<AdminPackage[]>('/api/admin/packages')
    packages.value = keyword.value
      ? data.filter((item) => `${item.name} ${item.description}`.toLowerCase().includes(keyword.value.toLowerCase()))
      : data
  } finally {
    loading.value = false
  }
}

function openPackage(row: AdminPackage) {
  router.push(`/packages/${row.name}`)
}

function editPackage(row: AdminPackage) {
  router.push({
    path: `/packages/${row.name}/edit`,
    query: {
      name: row.name,
      description: row.description
    }
  })
}

async function deletePackage(row: AdminPackage) {
  await ElMessageBox.confirm(`Delete package ${row.name} and all versions?`, 'Delete package', {
    type: 'warning',
    confirmButtonText: 'Delete',
    cancelButtonText: 'Cancel'
  })
  await api.delete(`/api/admin/packages/${row.name}`)
  ElMessage.success('Package deleted')
  await load()
}

onMounted(load)
</script>
