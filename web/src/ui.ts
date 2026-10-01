import type { PackageManifest } from './api'

// Technical identifiers are preserved; only the presentation labels are translated.
export function statusLabel(status: string) {
  return ({ Draft: '草稿', Published: '已发布', Unpublished: '已下架' } as Record<string, string>)[status] ?? '未知状态'
}
export function auditAction(action: string) {
  return ({
    login: '登录', 'create-package': '创建工具包', 'update-package': '编辑工具包',
    'delete-package': '删除工具包', 'upload-version': '上传版本', 'generate-version': '生成版本',
    'publish-version': '发布版本', 'unpublish-version': '下架版本',
    'resign-version': '重新签名', 'delete-version': '删除版本'
  } as Record<string, string>)[action] ?? '其他操作'
}
export function targetLabel(type: string) {
  return ({ auth: '身份认证', package: '工具包', 'package-version': '工具包版本' } as Record<string, string>)[type] ?? '其他对象'
}
export function formatTime(value: string) {
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? '时间未知' : date.toLocaleString('zh-CN', { hour12: false })
}
export function isManifest(value: unknown): value is PackageManifest {
  if (!value || typeof value !== 'object') return false
  const m = value as Record<string, unknown>
  return ['name', 'version', 'platform', 'arch', 'entry'].every(key => typeof m[key] === 'string' && (m[key] as string).trim().length > 0)
    && (m.description == null || typeof m.description === 'string')
    && Array.isArray(m.files) && m.files.every(file => file && typeof file.path === 'string' && typeof file.sha256 === 'string')
}
export function errorText(status?: number, title?: string) {
  const known: Record<string, string> = {
    'Package already exists': '该工具包名称已存在，请更换名称。',
    'Version already exists for platform and architecture': '该版本在指定平台和架构下已存在。',
    'Invalid .mty package': '.mty 文件无效，请检查文件及清单。',
    'Manifest name does not match route package name': '清单中的工具包名称与上传目标不一致。',
    'Executable file is required': '请选择非空的可执行文件。'
  }
  if (title && Object.hasOwn(known, title)) return known[title]
  if (status === 401) return '登录信息无效或已过期，请重新登录。'
  if (status === 403) return '权限不足，无法执行此操作。'
  if (status === 404) return '未找到工具包、版本或接口，请刷新后重试。'
  if (status === 409) return '数据冲突，请检查工具包名称、版本及目标平台。'
  if (status === 400 || status === 422) return '提交内容无效，请检查输入及文件。'
  if (!status) return '无法连接服务器，请检查网络后重试。'
  return '操作失败，请稍后重试。'
}
