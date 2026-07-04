import axios from 'axios'
import { ElMessage } from 'element-plus'

declare global {
  interface Window {
    __MTY_CONFIG__?: {
      VITE_API_BASE_URL?: string
    }
  }
}

const apiBaseUrl =
  window.__MTY_CONFIG__?.VITE_API_BASE_URL ||
  import.meta.env.VITE_API_BASE_URL ||
  'http://localhost:5000'

export const api = axios.create({
  baseURL: apiBaseUrl
})

api.interceptors.request.use((config) => {
  const token = localStorage.getItem('mty-token')
  if (token) {
    config.headers.Authorization = `Bearer ${token}`
  }
  return config
})

api.interceptors.response.use(
  (response) => response,
  (error) => {
    const status = error.response?.status
    const data = error.response?.data
    const title = data?.title ?? data?.error ?? error.message
    const detail = data?.detail ? `: ${data.detail}` : ''

    if (status === 404) {
      ElMessage.error(`Not found${detail || ': package, version, or API endpoint does not exist'}`)
    } else if (status === 401 || status === 403) {
      ElMessage.error('Session expired or permission denied. Sign in again.')
    } else if (status === 409) {
      ElMessage.error(`Conflict${detail || ': this version already exists'}`)
    } else {
      ElMessage.error(`${title}${detail}`)
    }

    return Promise.reject(error)
  }
)

export interface PackageSummary {
  name: string
  description: string
  latestVersion: string | null
}

export interface AdminPackage {
  id: string
  name: string
  description: string
  versionCount: number
  latestVersion: string | null
}

export interface PackageVersion {
  version: string
  platform: string
  arch: string
  sha256: string
  signature: string
  downloadUrl: string
}

export interface AuditLog {
  createdAt: string
  actor: string
  action: string
  targetType: string
  target: string
}

export interface PackageManifest {
  name: string
  version: string
  description?: string
  platform: string
  arch: string
  entry: string
  files: Array<{ path: string; sha256: string; executable?: boolean }>
  dependencies?: Array<{ name: string; version: string }>
}
