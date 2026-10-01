import assert from 'node:assert/strict'
import { isManifest, statusLabel, auditAction, targetLabel, formatTime, errorText } from '../src/ui.ts'
const manifest = { name: 'demo-tool', version: '1.0.0', platform: 'windows', arch: 'x86_64', entry: 'bin/demo.exe', files: [{ path: 'bin/demo.exe', sha256: 'abc' }] }
assert.equal(isManifest(manifest), true)
assert.equal(isManifest({ ...manifest, description: null }), true)
for (const value of [null, {}, { ...manifest, name: '' }, { ...manifest, files: null }, { ...manifest, files: [null] }, { ...manifest, description: {} }]) assert.equal(isManifest(value), false)
assert.equal(statusLabel('Published'), '已发布')
assert.equal(statusLabel('Unpublished'), '已下架')
assert.equal(statusLabel('Draft'), '草稿')
assert.equal(statusLabel('unexpected'), '未知状态')
assert.equal(auditAction('publish-version'), '发布版本')
assert.equal(targetLabel('package-version'), '工具包版本')
assert.equal(formatTime('invalid'), '时间未知')
assert.equal(errorText(409, 'Package already exists'), '该工具包名称已存在，请更换名称。')
assert.match(errorText(401), /重新登录/)
assert.match(errorText(undefined), /连接服务器/)
assert.equal(/[a-z]/i.test(errorText(500, 'Internal Server Error')), false)
console.log('中文标签、错误提示和清单校验检查通过')
