<template>
  <section class="page">
    <div class="page-title">
      <div><h1>{{ isEdit ? '编辑工具包' : isCreate ? '新建工具包' : '上传版本' }}</h1><p>{{ isEdit ? '更新工具包名称与描述。' : isCreate ? '填写基本信息，创建后可继续上传版本。' : '上传后保存为草稿，可在工具包详情中发布。' }}</p></div>
      <el-button :icon="Back" @click="back">返回{{ targetName && !isCreate ? '工具包详情' : '工具包列表' }}</el-button>
    </div>
    <div v-if="failed" class="panel"><el-empty description="工具信息加载失败，请重试"><el-button @click="loadInfo">重新加载</el-button></el-empty></div>
    <div v-else-if="isCreate || isEdit" class="panel form-panel" v-loading="infoLoading">
      <el-form label-position="top" :disabled="!ready" @submit.prevent="savePackage">
        <el-form-item label="工具包名称" required><el-input v-model="packageForm.name" aria-label="工具包名称" placeholder="例如：demo-tool" :disabled="loading" /></el-form-item>
        <el-form-item label="描述" required><el-input v-model="packageForm.description" aria-label="描述" type="textarea" :autosize="{ minRows: 4, maxRows: 10 }" :maxlength="512" show-word-limit placeholder="说明该工具的用途，可分行填写" :disabled="loading" /></el-form-item>
        <div class="form-actions"><el-button type="primary" native-type="submit" :loading="loading">{{ isEdit ? '保存修改' : '创建工具包' }}</el-button><el-button :disabled="loading" @click="back">取消</el-button></div>
      </el-form>
    </div>
    <div v-else class="panel" v-loading="infoLoading">
      <p class="upload-target">当前工具包：<strong>{{ targetName }}</strong></p>
      <el-tabs v-model="mode" class="version-tabs" :before-leave="() => !loading && !reading">
        <el-tab-pane label=".mty 工具包" name="package">
          <el-upload ref="packageUpload" class="file-upload" drag accept=".mty" :auto-upload="false" :show-file-list="false" :multiple="false" :disabled="!ready || loading || reading" :on-change="selectPackageFile">
            <el-icon class="upload-icon"><CloudUploadIcon /></el-icon><p class="upload-copy">拖拽 .mty 文件到此处，或<span class="upload-link">点击选择</span></p>
            <p class="field-hint">读取 manifest.json，自动识别工具包与版本信息</p>
          </el-upload>
          <div v-if="packageFile" class="selected-file"><span><el-icon><Document /></el-icon> {{ packageFile.name }}</span><div class="table-actions"><el-button link type="primary" :icon="Refresh" :disabled="loading || reading" @click="reselect(packageUpload)">重新选择</el-button><el-button link type="danger" :icon="Delete" :disabled="loading" @click="clearPackage">移除</el-button></div></div>
          <p v-if="reading" class="field-hint" role="status">正在读取清单，请稍候…</p>
          <div v-if="manifest" class="manifest-preview">
            <h2 class="section-heading">清单预览</h2>
            <dl class="manifest-grid">
              <div><dt>工具包名称</dt><dd>{{ manifest.name }}</dd></div><div><dt>版本号</dt><dd>{{ manifest.version }}</dd></div>
              <div><dt>目标平台</dt><dd>{{ platformLabel(manifest.platform) }}</dd></div><div><dt>架构</dt><dd>{{ manifest.arch }}</dd></div>
              <div><dt>文件数量</dt><dd>{{ manifest.files.length }}</dd></div>
            </dl>
          </div>
          <div class="form-actions"><el-button type="primary" :icon="Upload" :disabled="!ready || !manifest || reading" :loading="loading" @click="uploadPackage">上传为草稿</el-button><el-button :disabled="loading" @click="back">取消</el-button></div>
        </el-tab-pane>
        <el-tab-pane label="可执行文件" name="executable">
          <el-form label-position="top" :disabled="!ready">
            <el-row :gutter="24">
              <el-col :xs="24" :sm="12"><el-form-item label="版本号" required><el-input v-model="executableForm.version" aria-label="版本号" placeholder="例如：1.0.0" :disabled="loading" /></el-form-item></el-col>
              <el-col :xs="24" :sm="12"><el-form-item label="目标平台"><el-select v-model="executableForm.platform" aria-label="目标平台" :disabled="loading"><el-option label="Windows" value="windows" /><el-option label="Linux" value="linux" /><el-option label="macOS" value="macos" /></el-select></el-form-item></el-col>
              <el-col :xs="24" :sm="12"><el-form-item label="架构"><el-select v-model="executableForm.arch" aria-label="架构" :disabled="loading"><el-option v-for="arch in architectures" :key="arch" :label="arch" :value="arch" /></el-select></el-form-item></el-col>
            </el-row>
            <el-upload ref="executableUpload" class="file-upload" drag :auto-upload="false" :show-file-list="false" :multiple="false" :disabled="!ready || loading" :on-change="selectExecutableFile">
              <el-icon class="upload-icon"><CloudUploadIcon /></el-icon><p class="upload-copy">拖拽可执行文件到此处，或<span class="upload-link">点击选择</span></p><p class="field-hint">自动生成 .mty 工具包并保存为草稿</p>
            </el-upload>
            <div v-if="executableFile" class="selected-file"><span><el-icon><Document /></el-icon> {{ executableFile.name }}</span><div class="table-actions"><el-button link type="primary" :icon="Refresh" :disabled="loading" @click="reselect(executableUpload)">重新选择</el-button><el-button link type="danger" :icon="Delete" :disabled="loading" @click="clearExecutable">移除</el-button></div></div>
            <div class="form-actions"><el-button type="primary" :disabled="!ready || !executableFile" :loading="loading" @click="generatePackage">生成并上传草稿</el-button><el-button :disabled="loading" @click="back">取消</el-button></div>
            <p v-if="targetName === 'mty'" class="field-hint">此版本用于 MTY 客户端自更新，发布后生效。</p>
          </el-form>
        </el-tab-pane>
      </el-tabs>
    </div>
  </section>
</template>
<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import type { UploadFile, UploadInstance } from 'element-plus'
import { ElMessage } from 'element-plus'
import { Back, Delete, Upload, Document, Refresh } from '@element-plus/icons-vue'
import CloudUploadIcon from '../components/CloudUploadIcon.vue'
import { api, type PackageManifest, type AdminPackageDetail } from '../api'
import { isManifest } from '../ui'
const route = useRoute()
const router = useRouter()
const loading = ref(false)
const reading = ref(false)
const mode = ref('package')
const packageUpload = ref<UploadInstance>()
const executableUpload = ref<UploadInstance>()
const packageFile = ref<File | null>(null)
const executableFile = ref<File | null>(null)
const manifest = ref<PackageManifest | null>(null)
const isEdit = computed(() => route.meta.editPackage === true)
const isCreate = computed(() => route.path === '/packages/new')
const originalName = String(route.params.name ?? '')
const targetName = originalName
const packageForm = reactive({ name: targetName, description: '' })
const infoLoading = ref(false)
const failed = ref(false)
const ready = ref(isCreate.value)
const executableForm = reactive({ version: '', platform: 'windows', arch: 'x86_64' })
const architectures = ['x86_64', 'x64', 'aarch64', 'arm64', 'x86']
async function loadInfo() {
  if (isCreate.value || infoLoading.value) return
  infoLoading.value = true
  failed.value = false
  ready.value = false
  try {
    const { data } = await api.get<AdminPackageDetail>('/api/admin/packages/' + encodeURIComponent(targetName))
    packageForm.name = data.name
    packageForm.description = data.description
    ready.value = true
  } catch { failed.value = true }
  finally { infoLoading.value = false }
}
function platformLabel(platform: string) { return ({ windows: 'Windows', linux: 'Linux', macos: 'macOS' } as Record<string, string>)[platform] ?? platform }
function detailPath(name: string) { return '/packages/' + encodeURIComponent(name) }
function back() { router.push(targetName && !isCreate.value ? detailPath(targetName) : '/packages') }
function clearPackage() { packageFile.value = null; manifest.value = null; reading.value = false; packageUpload.value?.clearFiles() }
function clearExecutable() { executableFile.value = null; executableUpload.value?.clearFiles() }
function reselect(upload?: UploadInstance) { (upload?.$el.querySelector('input[type=file]') as HTMLInputElement | undefined)?.click() }
async function selectPackageFile(upload: UploadFile) {
  const file = upload.raw
  packageUpload.value?.clearFiles()
  if (!file) return
  clearPackage()
  packageFile.value = file
  if (!file.name.toLowerCase().endsWith('.mty')) { ElMessage.error('请选择 .mty 工具包文件'); return }
  reading.value = true
  try {
    const { default: JSZip } = await import('jszip')
    const archive = await JSZip.loadAsync(file)
    const entry = archive.file('manifest.json')
    if (!entry) throw new Error('工具包缺少 manifest.json')
    const parsed: unknown = JSON.parse(await entry.async('string'))
    if (!isManifest(parsed)) throw new Error('清单字段无效，请检查名称、版本、平台、架构及文件列表')
    if (targetName && parsed.name.toLowerCase() !== targetName.toLowerCase()) throw new Error('清单中的工具包名称与当前工具包不一致')
    if (packageFile.value === file) manifest.value = parsed
  } catch (error) {
    if (packageFile.value === file) ElMessage.error(error instanceof Error && error.message.startsWith('清单') || error instanceof Error && error.message.startsWith('工具包') ? (error as Error).message : '无法读取工具包，请检查文件及 manifest.json')
  } finally { if (packageFile.value === file) reading.value = false }
}
function selectExecutableFile(upload: UploadFile) {
  executableFile.value = upload.raw ?? null
  executableUpload.value?.clearFiles()
  if (executableFile.value?.size === 0) { clearExecutable(); ElMessage.error('可执行文件不能为空') }
}
async function savePackage() {
  if (loading.value || !ready.value) return
  const body = { name: packageForm.name.trim(), description: packageForm.description.trim() }
  if (!body.name || !body.description) { ElMessage.warning('请填写工具包名称和描述'); return }
  loading.value = true
  try {
    if (isEdit.value) await api.put('/api/admin/packages/' + encodeURIComponent(originalName), body)
    else await api.post('/api/admin/packages', body)
    ElMessage.success(isEdit.value ? '工具包已更新' : '工具包已创建，可继续上传版本')
    await router.push(detailPath(body.name))
  } catch { /* 请求层统一提示。 */ }
  finally { loading.value = false }
}
async function uploadPackage() {
  if (loading.value || !ready.value || reading.value || !packageFile.value || !manifest.value) return
  const name = targetName
  loading.value = true
  try {
    const body = new FormData()
    body.append('file', packageFile.value)
    await api.post('/api/admin/packages/' + encodeURIComponent(name) + '/versions', body)
    ElMessage.success('版本已上传为草稿')
    await router.push(detailPath(name))
  } catch { /* 请求层统一提示。 */ }
  finally { loading.value = false }
}
async function generatePackage() {
  if (loading.value || !ready.value || !executableFile.value) return
  const name = targetName
  if (!executableForm.version.trim()) { ElMessage.warning('请填写版本号'); return }
  loading.value = true
  try {
    const body = new FormData()
    body.append('version', executableForm.version.trim())
    body.append('platform', executableForm.platform)
    body.append('arch', executableForm.arch)
    body.append('description', packageForm.description.trim())
    body.append('file', executableFile.value)
    await api.post('/api/admin/packages/' + encodeURIComponent(name) + '/versions/from-executable', body)
    ElMessage.success('已生成并上传 .mty 草稿')
    await router.push(detailPath(name))
  } catch { /* 请求层统一提示。 */ }
  finally { loading.value = false }
}
onMounted(loadInfo)
</script>
