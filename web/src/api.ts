import axios from 'axios'
import { ElMessage } from 'element-plus'
import { errorText } from './ui'

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
    ElMessage.error(errorText(status, data?.title ?? data?.error))

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

export interface AdminOverview {
  packageCount: number
  versionCount: number
  publishedVersionCount: number
  downloadCount: number
}

export interface AdminPackageDetail {
  name: string
  description: string
  versions: AdminPackageVersion[]
}

export interface AdminPackageVersion extends Omit<PackageVersion, 'downloadUrl'> {
  status: string
  downloadCount: number
  createdAt: string
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
  description?: string | null
  platform: string
  arch: string
  entry: string
  files: Array<{ path: string; sha256: string; executable?: boolean }>
  dependencies?: Array<{ name: string; version: string }>
}
