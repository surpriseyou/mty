<template>
  <section class="page home-page">
    <div class="page-title">
      <div><h1>首页概览</h1><p>查看工具仓库规模与版本下载情况。</p></div>
      <el-button :icon="Refresh" :loading="loading" @click="load">刷新</el-button>
    </div>
    <div class="panel overview-stats" v-loading="loading" aria-label="仓库统计">
      <el-empty v-if="statsFailed" description="统计加载失败，请刷新重试" />
      <template v-else>
        <div v-for="metric in metrics" :key="metric.key" class="overview-metric">
          <el-icon class="metric-icon" :class="metric.key"><component :is="metric.icon" /></el-icon>
          <div><p class="metric-label">{{ metric.label }}</p><strong class="metric-value">{{ summary ? summary[metric.key].toLocaleString('zh-CN') : '—' }}</strong></div>
        </div>
      </template>
    </div>
    <p class="overview-note">下载次数汇总当前存量版本，包含客户端和管理端下载。</p>
    <div class="panel overview-tools" v-loading="loading">
      <div class="overview-heading"><h2>工具速览</h2><el-button link type="primary" @click="router.push('/packages')">查看全部工具<el-icon><ArrowRight /></el-icon></el-button></div>
      <el-empty v-if="packagesFailed" description="工具加载失败，请刷新重试" />
      <div v-else class="table-scroll">
        <el-table :data="packages.slice(0, 5)" style="min-width: 720px">
          <el-table-column label="工具包名称" min-width="240"><template #default="{ row }"><el-button link class="package-name" @click="open(row.name)">{{ row.name }}</el-button></template></el-table-column>
          <el-table-column prop="versionCount" label="版本数量" min-width="140" />
          <el-table-column label="最新版本" min-width="160"><template #default="{ row }">{{ row.latestVersion ?? '暂无版本' }}</template></el-table-column>
          <el-table-column label="操作" width="200"><template #default="{ row }"><el-button link type="primary" :icon="View" @click="open(row.name)">查看详情</el-button></template></el-table-column>
          <template #empty><el-empty description="暂无工具包，请前往工具包管理创建" /></template>
        </el-table>
      </div>
    </div>
  </section>
</template>
<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { Box, Collection, Document, Download, Refresh, View, ArrowRight } from '@element-plus/icons-vue'
import { api, type AdminOverview, type AdminPackage } from '../api'
const router = useRouter()
const summary = ref<AdminOverview | null>(null)
const packages = ref<AdminPackage[]>([])
const loading = ref(false)
const statsFailed = ref(false)
const packagesFailed = ref(false)
const metrics = [
  { key: 'packageCount', label: '工具数量', icon: Box },
  { key: 'versionCount', label: '版本数量', icon: Collection },
  { key: 'publishedVersionCount', label: '已发布版本', icon: Document },
  { key: 'downloadCount', label: '累计下载次数', icon: Download }
] as const
function open(name: string) { router.push('/packages/' + encodeURIComponent(name)) }
async function load() {
  if (loading.value) return
  loading.value = true
  statsFailed.value = false
  packagesFailed.value = false
  const [stats, tools] = await Promise.allSettled([
    api.get<AdminOverview>('/api/admin/overview'),
    api.get<AdminPackage[]>('/api/admin/packages')
  ])
  if (stats.status === 'fulfilled') summary.value = stats.value.data
  else { summary.value = null; statsFailed.value = true }
  if (tools.status === 'fulfilled') packages.value = tools.value.data
  else { packages.value = []; packagesFailed.value = true }
  loading.value = false
}
onMounted(load)
</script>
