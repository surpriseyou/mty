<template>
  <section class="page">
    <div class="page-title"><div><h1>{{ name }}</h1><p class="tool-description">{{ detail?.description || '工具包详情与版本管理' }}</p></div><div class="title-actions"><el-button :icon="Back" @click="router.push('/packages')">返回列表</el-button><el-button :icon="Edit" :disabled="!detail || failed" @click="router.push('/packages/' + encodeURIComponent(name) + '/edit')">编辑信息</el-button><el-button type="primary" :icon="Upload" :disabled="!detail || failed" @click="newVersion">上传新版本</el-button></div></div>
    <div class="panel" v-loading="loading">
      <div class="toolbar"><h2 class="section-heading" style="margin: 0; flex: 1">版本列表</h2><el-button :icon="Refresh" :loading="loading" @click="load">刷新</el-button></div>
      <el-empty v-if="failed" description="加载失败，请刷新重试" />
      <div v-else class="table-scroll" style="margin-top: 20px">
        <el-table :data="detail?.versions ?? []" style="min-width: 1080px">
          <el-table-column prop="version" label="版本号" min-width="130" />
          <el-table-column label="目标平台 / 架构" min-width="185"><template #default="{ row }">{{ row.platform }} / {{ row.arch }}</template></el-table-column>
          <el-table-column label="状态" width="110"><template #default="{ row }"><el-tag :type="row.status === 'Published' ? 'success' : row.status === 'Draft' ? 'info' : 'warning'" effect="light">{{ statusLabel(row.status) }}</el-tag></template></el-table-column>
          <el-table-column prop="downloadCount" label="下载次数" width="100" />
          <el-table-column label="校验与签名" width="130"><template #default="{ row }"><el-button link type="primary" @click="selected = row">查看信息</el-button></template></el-table-column>
          <el-table-column label="操作" width="420"><template #default="{ row }"><div class="table-actions">
            <el-button link type="primary" :icon="Download" :disabled="busy" @click="download(row)">下载</el-button>
            <el-tooltip content="发布后可复制下载链接" :disabled="row.status === 'Published'"><span><el-button link type="primary" :icon="CopyDocument" :disabled="row.status !== 'Published' || busy || copying" @click="copyLink(row)">复制下载链接</el-button></span></el-tooltip>
            <el-button v-if="row.status !== 'Published'" link type="primary" :disabled="busy" @click="mutate(row, 'publish')">发布</el-button>
            <el-button v-else link type="warning" :disabled="busy" @click="mutate(row, 'unpublish')">下架</el-button>
            <el-dropdown trigger="click" :disabled="busy" @command="(command: string) => mutate(row, command)">
              <el-button link :disabled="busy" aria-label="更多版本操作">更多<el-icon><ArrowDown /></el-icon></el-button>
              <template #dropdown><el-dropdown-menu><el-dropdown-item command="resign">重新签名</el-dropdown-item><el-dropdown-item command="delete" divided>删除版本</el-dropdown-item></el-dropdown-menu></template>
            </el-dropdown>
          </div></template></el-table-column>
          <template #empty><el-empty description="暂无版本，点击“上传新版本”添加草稿" /></template>
        </el-table>
      </div>
      <p v-if="detail && !failed" class="count-label">共 {{ detail.versions.length }} 个版本 · 上传后需发布才可供客户端安装</p>
    </div>
    <el-dialog :model-value="!!selected" title="校验与签名信息" width="680px" @close="selected = null">
      <template v-if="selected"><p>{{ selected.version }} · {{ selected.platform }} / {{ selected.arch }}</p><h3>SHA256 校验值</h3><p class="checksum">{{ selected.sha256 }}</p><h3>签名</h3><p class="checksum">{{ selected.signature || '暂无签名' }}</p></template>
      <template #footer><el-button @click="selected = null">关闭</el-button></template>
    </el-dialog>
    <el-dialog :model-value="!!manualLink" title="手动复制下载链接" width="680px" @close="manualLink = ''">
      <p>浏览器未允许自动复制，请选中以下链接并复制。</p>
      <el-input :model-value="manualLink" type="textarea" :autosize="{ minRows: 3, maxRows: 6 }" readonly aria-label="下载链接" @focus="selectLink" />
      <template #footer><el-button @click="manualLink = ''">关闭</el-button></template>
    </el-dialog>
  </section>
</template>
<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Back, Upload, Refresh, Download, ArrowDown, CopyDocument, Edit } from '@element-plus/icons-vue'
import { api, type AdminPackageDetail, type AdminPackageVersion } from '../api'
import { statusLabel } from '../ui'
type VersionRow = AdminPackageVersion
const route = useRoute()
const router = useRouter()
const name = String(route.params.name)
const base = '/api/admin/packages/' + encodeURIComponent(name)
const detail = ref<AdminPackageDetail | null>(null)
const selected = ref<VersionRow | null>(null)
const loading = ref(false)
const failed = ref(false)
const busy = ref(false)
const copying = ref(false)
const manualLink = ref('')
async function load() {
  if (loading.value) return
  loading.value = true; failed.value = false
  try { detail.value = (await api.get<AdminPackageDetail>(base)).data }
  catch { failed.value = true }
  finally { loading.value = false }
}
function versionUrl(row: VersionRow) { return base + '/versions/' + encodeURIComponent(row.version) }
function params(row: VersionRow) { return { platform: row.platform, arch: row.arch } }
function newVersion() { router.push('/packages/' + encodeURIComponent(name) + '/upload') }
function selectLink(event: FocusEvent) { (event.target as HTMLTextAreaElement).select() }
async function copyLink(row: VersionRow) {
  if (row.status !== 'Published' || copying.value || busy.value) return
  const url = new URL(api.getUri({
    url: '/api/packages/' + encodeURIComponent(name) + '/versions/' + encodeURIComponent(row.version) + '/download',
    params: params(row)
  }), window.location.href).href
  copying.value = true
  try {
    await navigator.clipboard.writeText(url)
    ElMessage.success('下载链接已复制')
  } catch { manualLink.value = url }
  finally { copying.value = false }
}
async function mutate(row: VersionRow, action: string) {
  if (busy.value || !['publish', 'unpublish', 'resign', 'delete'].includes(action)) return
  busy.value = true
  try {
    if (action === 'delete' || action === 'unpublish') {
      const title = action === 'delete' ? '删除版本' : '下架版本'
      try { await ElMessageBox.confirm((action === 'delete' ? '删除后无法恢复。确认删除 ' : '下架后客户端将无法安装此版本。确认下架 ') + name + '@' + row.version + '（' + row.platform + '/' + row.arch + '）？', title, { type: 'warning', confirmButtonText: action === 'delete' ? '确认删除' : '确认下架', cancelButtonText: '取消' }) } catch { return }
    }
    if (action === 'delete') await api.delete(versionUrl(row), { params: params(row) })
    else await api.post(versionUrl(row) + '/' + action, null, { params: params(row) })
    ElMessage.success(({ publish: '版本已发布', unpublish: '版本已下架', resign: '版本已重新签名', delete: '版本已删除' } as Record<string, string>)[action])
    await load()
  } catch { /* 请求层统一提示。 */ }
  finally { busy.value = false }
}
async function download(row: VersionRow) {
  if (busy.value) return
  busy.value = true
  try {
    const response = await api.get(versionUrl(row) + '/download', { params: params(row), responseType: 'blob' })
    const url = URL.createObjectURL(response.data)
    const link = document.createElement('a')
    link.href = url
    link.download = name + '-' + row.version + '-' + row.platform + '-' + row.arch + '.mty'
    document.body.appendChild(link); link.click(); link.remove()
    setTimeout(() => URL.revokeObjectURL(url), 1000)
    await load()
  } catch { /* 请求层统一提示。 */ }
  finally { busy.value = false }
}
onMounted(load)
</script>
