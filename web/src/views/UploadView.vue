<template>
  <section class="page">
    <div class="page-title">
      <div>
        <h1>{{ isEdit ? 'Edit package' : '新增package' }}</h1>
        <p>{{ isEdit ? 'Update package metadata.' : 'Create a package or prepare a new internal tool release.' }}</p>
      </div>
    </div>
    <div class="panel">
      <el-form label-position="top">
        <el-row :gutter="16">
          <el-col :span="12">
            <el-form-item label="Package name">
              <el-input v-model="packageForm.name" placeholder="demo-tool" />
            </el-form-item>
          </el-col>
          <el-col :span="12">
            <el-form-item label="Description">
              <el-input v-model="packageForm.description" placeholder="Internal utility" />
            </el-form-item>
          </el-col>
        </el-row>
        <el-button type="primary" :loading="loading" @click="savePackage">
          {{ isEdit ? 'Save changes' : 'Create package' }}
        </el-button>
      </el-form>

      <el-tabs v-if="!isEdit" v-model="mode" class="version-tabs">
        <el-tab-pane label=".mty package" name="package">
          <el-form label-position="top">
            <el-alert
              v-if="manifest"
              type="success"
              :closable="false"
              class="manifest-summary"
              :title="`${manifest.name} ${manifest.version} · ${manifest.platform}/${manifest.arch}`"
              :description="manifest.description ?? `${manifest.files.length} files in package`"
            />
            <el-empty v-else description="Choose a .mty package to read manifest.json before upload" />
            <el-form-item label=".mty file">
              <el-upload :auto-upload="false" :limit="1" :on-change="selectPackageFile">
                <el-button>Choose .mty</el-button>
              </el-upload>
            </el-form-item>
            <el-button type="primary" :disabled="!manifest" :loading="loading" @click="uploadPackage">Upload draft</el-button>
          </el-form>
        </el-tab-pane>

        <el-tab-pane label="Executable file" name="executable">
          <el-form label-position="top">
            <el-row :gutter="16">
              <el-col :span="12">
                <el-form-item label="Version">
                  <el-input v-model="executableForm.version" placeholder="1.0.0" />
                </el-form-item>
              </el-col>
              <el-col :span="12">
                <el-form-item label="Platform">
                  <el-select v-model="executableForm.platform">
                    <el-option label="windows" value="windows" />
                    <el-option label="linux" value="linux" />
                    <el-option label="macos" value="macos" />
                  </el-select>
                </el-form-item>
              </el-col>
              <el-col :span="12">
                <el-form-item label="Architecture">
                  <el-select v-model="executableForm.arch">
                    <el-option label="x86_64" value="x86_64" />
                    <el-option label="x64" value="x64" />
                    <el-option label="aarch64" value="aarch64" />
                    <el-option label="arm64" value="arm64" />
                    <el-option label="x86" value="x86" />
                  </el-select>
                </el-form-item>
              </el-col>
            </el-row>
            <el-form-item label="Executable file">
              <el-upload :auto-upload="false" :limit="1" :on-change="selectExecutableFile">
                <el-button>Choose executable</el-button>
              </el-upload>
            </el-form-item>
            <el-button type="primary" :disabled="!executableFile" :loading="loading" @click="generatePackage()">Generate .mty draft</el-button>
            <el-button :disabled="!executableFile" :loading="loading" @click="generatePackage('mty')">Upload self-update</el-button>
          </el-form>
        </el-tab-pane>
      </el-tabs>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, reactive, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import type { UploadFile } from 'element-plus'
import { ElMessage } from 'element-plus'
import { api, type PackageManifest } from '../api'

const route = useRoute()
const router = useRouter()
const loading = ref(false)
const mode = ref('package')
const packageFile = ref<File | null>(null)
const executableFile = ref<File | null>(null)
const manifest = ref<PackageManifest | null>(null)
const isEdit = computed(() => route.meta.editPackage === true)
const originalName = String(route.params.name ?? '')
const packageForm = reactive({
  name: queryValue(route.query.name) || originalName,
  description: queryValue(route.query.description)
})
const executableForm = reactive({
  version: '',
  platform: 'windows',
  arch: 'x86_64'
})

function queryValue(value: unknown) {
  return Array.isArray(value) ? String(value[0] ?? '') : String(value ?? '')
}

async function selectPackageFile(uploadFile: UploadFile) {
  packageFile.value = uploadFile.raw ?? null
  manifest.value = null
  if (!packageFile.value) return

  try {
    const zip = await import('jszip')
    const archive = await zip.default.loadAsync(packageFile.value)
    const manifestEntry = archive.file('manifest.json')
    if (!manifestEntry) {
      ElMessage.error('manifest.json is missing from this .mty package')
      return
    }
    manifest.value = JSON.parse(await manifestEntry.async('string')) as PackageManifest
    if (!packageForm.name) {
      packageForm.name = manifest.value.name
    }
    if (!packageForm.description) {
      packageForm.description = manifest.value.description ?? ''
    }
  } catch {
    ElMessage.error('Unable to read manifest.json from this .mty package')
  }
}

function selectExecutableFile(uploadFile: UploadFile) {
  executableFile.value = uploadFile.raw ?? null
}

async function savePackage() {
  if (!packageForm.name || !packageForm.description) {
    ElMessage.warning('Package name and description are required')
    return
  }

  loading.value = true
  try {
    if (isEdit.value) {
      await api.put(`/api/admin/packages/${originalName}`, packageForm)
      ElMessage.success('Package updated')
      router.push(`/packages/${packageForm.name}`)
    } else {
      await api.post('/api/admin/packages', packageForm)
      ElMessage.success('Package created')
      router.push(`/packages/${packageForm.name}`)
    }
  } finally {
    loading.value = false
  }
}

async function uploadPackage() {
  if (!packageFile.value) {
    ElMessage.warning('Choose a .mty file first')
    return
  }

  loading.value = true
  try {
    const body = new FormData()
    body.append('file', packageFile.value)
    await api.post(`/api/admin/packages/${manifest.value!.name}/versions`, body)
    ElMessage.success('Version uploaded as draft')
    router.push(`/packages/${manifest.value!.name}`)
  } finally {
    loading.value = false
  }
}

async function generatePackage(forcedName?: string) {
  if (!executableFile.value) {
    ElMessage.warning('Choose an executable file first')
    return
  }
  const packageName = forcedName ?? packageForm.name
  if (!packageName || !executableForm.version) {
    ElMessage.warning('Package name and version are required')
    return
  }

  loading.value = true
  try {
    const body = new FormData()
    body.append('version', executableForm.version)
    body.append('platform', executableForm.platform)
    body.append('arch', executableForm.arch)
    body.append('description', packageForm.description)
    body.append('file', executableFile.value)
    await api.post(`/api/admin/packages/${packageName}/versions/from-executable`, body)
    ElMessage.success(forcedName ? 'Self-update package uploaded as draft' : 'Generated .mty draft from executable')
    router.push(`/packages/${packageName}`)
  } finally {
    loading.value = false
  }
}
</script>

<style scoped>
.manifest-summary {
  margin-bottom: 18px;
}

.version-tabs {
  margin-top: 24px;
}
</style>
