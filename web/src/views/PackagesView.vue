<template>
  <section class="page">
    <div class="page-title"><div><h1>工具包管理</h1><p>集中管理内部工具与发布版本。</p></div><el-button type="primary" :icon="Plus" @click="router.push('/packages/new')">新建工具包</el-button></div>
    <div class="panel">
      <form class="toolbar" @submit.prevent="search">
        <el-input v-model="keyword" aria-label="搜索工具包名称或描述" placeholder="搜索工具包名称或描述" :prefix-icon="Search" clearable @clear="search" />
        <el-button type="primary" :icon="Search" native-type="submit">搜索</el-button>
        <el-button :icon="Refresh" :loading="loading" @click="load">刷新</el-button>
      </form>
    </div>
    <div class="panel table-panel" v-loading="loading">
      <el-empty v-if="failed" description="加载失败，请刷新重试"><el-button @click="load">重新加载</el-button></el-empty>
      <div v-else class="table-scroll">
        <el-table :data="filtered" style="min-width: 850px">
          <el-table-column label="工具包名称" min-width="200"><template #default="{ row }"><el-button link type="primary" class="package-name" @click="open(row)">{{ row.name }}</el-button></template></el-table-column>
          <el-table-column prop="description" label="描述" min-width="240" show-overflow-tooltip />
          <el-table-column prop="versionCount" label="版本数量" width="110" />
          <el-table-column label="最新版本" width="130"><template #default="{ row }">{{ row.latestVersion ?? '暂无版本' }}</template></el-table-column>
          <el-table-column label="操作" width="330"><template #default="{ row }"><div class="table-actions">
            <el-button link type="primary" :icon="View" @click="open(row)">查看详情</el-button>
            <el-button link type="primary" :icon="Edit" @click="edit(row)">编辑</el-button>
            <el-button link type="danger" :icon="Delete" :disabled="!!deleting" @click="remove(row)">删除</el-button>
          </div></template></el-table-column>
          <template #empty><el-empty :description="applied ? '没有匹配的工具包，请调整搜索条件' : '暂无工具包，点击“新建工具包”开始使用'" /></template>
        </el-table>
      </div>
      <p v-if="!failed" class="count-label">共 {{ filtered.length }} 个工具包</p>
    </div>
  </section>
</template>
<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Plus, Search, Refresh, View, Edit, Delete } from '@element-plus/icons-vue'
import { api, type AdminPackage } from '../api'
const router = useRouter()
const packages = ref<AdminPackage[]>([])
const keyword = ref('')
const applied = ref('')
const loading = ref(false)
const failed = ref(false)
const deleting = ref('')
const filtered = computed(() => packages.value.filter(item => (item.name + ' ' + item.description).toLowerCase().includes(applied.value)))
function search() { applied.value = keyword.value.trim().toLowerCase() }
async function load() {
  if (loading.value) return
  loading.value = true
  failed.value = false
  try { packages.value = (await api.get<AdminPackage[]>('/api/admin/packages')).data }
  catch { failed.value = true }
  finally { loading.value = false }
}
function open(row: AdminPackage) { router.push('/packages/' + encodeURIComponent(row.name)) }
function edit(row: AdminPackage) { router.push('/packages/' + encodeURIComponent(row.name) + '/edit') }
async function remove(row: AdminPackage) {
  if (deleting.value) return
  try {
    await ElMessageBox.confirm('将删除工具包“' + row.name + '”及其全部版本，此操作无法撤销。', '删除工具包', { type: 'warning', confirmButtonText: '确认删除', cancelButtonText: '取消' })
  } catch { return }
  deleting.value = row.name
  try { await api.delete('/api/admin/packages/' + encodeURIComponent(row.name)); ElMessage.success('工具包已删除'); await load() }
  catch { /* 请求层统一提示。 */ }
  finally { deleting.value = '' }
}
onMounted(load)
</script>
