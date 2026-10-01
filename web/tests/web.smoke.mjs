// Run with PLAYWRIGHT_MODULE and PLAYWRIGHT_EXECUTABLE pointing to an existing installation.
import assert from 'node:assert/strict'
import { pathToFileURL } from 'node:url'
import { mkdir } from 'node:fs/promises'
import JSZip from 'jszip'
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ? pathToFileURL(process.env.PLAYWRIGHT_MODULE).href : 'playwright')
const browser = await chromium.launch({ executablePath: process.env.PLAYWRIGHT_EXECUTABLE, headless: true })
const origin = process.env.SMOKE_URL || 'http://127.0.0.1:5173'
const screenshots = process.env.SMOKE_SCREENSHOTS
const versions = ['Published', 'Draft', 'Unpublished'].map((status, i) => ({ version: i ? '1.0.' + i : '1.0.0+中文', platform: 'windows', arch: 'x86_64', status, downloadCount: 42, createdAt: '2026-10-01T00:00:00Z', sha256: 'abc', signature: 'sig' }))
let description = '第一行用途\n第二行说明'
let overview = { packageCount: 2, versionCount: 8, publishedVersionCount: 6, downloadCount: 128 }
let failStats = false
let downloadRequests = 0
let uploadBody = ''
const context = await browser.newContext({ viewport: { width: 1536, height: 1024 } })
await context.addInitScript(() => {
  localStorage.setItem('mty-token', 'test-token')
  window.__MTY_CONFIG__ = { VITE_API_BASE_URL: '/gateway' }
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: async value => {
    if (window.__clipboardFail) throw new Error('Denied')
    window.__copied = value
  } } })
})
await context.route('**/env.js', route => route.fulfill({ contentType: 'text/javascript', body: 'window.__MTY_CONFIG__ = { VITE_API_BASE_URL: "/gateway" }' }))
await context.route('**/gateway/api/**', async route => {
  const request = route.request()
  const path = new URL(request.url()).pathname.replace('/gateway', '')
  let data = {}
  if (path.endsWith('/download')) downloadRequests++
  if (path === '/api/admin/overview') {
    if (failStats) return route.fulfill({ status: 500, json: {} })
    data = overview
  } else if (path === '/api/admin/packages') data = ['demo-tool', 'mty'].map(name => ({ name, description, versionCount: 4, latestVersion: '1.0.0' }))
  else if (path.endsWith('/from-executable') || path.endsWith('/versions')) uploadBody = request.postData() || ''
  else if (request.method() === 'PUT') description = request.postDataJSON().description
  else data = { name: decodeURIComponent(path.split('/').at(-1)), description, versions }
  await route.fulfill({ json: data })
})
const page = await context.newPage()
const errors = []
page.on('pageerror', error => errors.push(error.message))
async function visit(path) {
  await page.goto(origin + path)
  await page.locator('h1').waitFor()
  await page.waitForFunction(() => !document.querySelector('.el-loading-mask'))
}
async function shot(name) {
  if (!screenshots) return
  await mkdir(screenshots, { recursive: true })
  await page.waitForFunction(() => !document.querySelector('.el-message'))
  await page.screenshot({ path: screenshots + '/' + name + '.png', fullPage: true, animations: 'disabled' })
}
try {
  await visit('/')
  await page.locator('.metric-value').filter({ hasText: '128' }).waitFor()
  assert.deepEqual(await page.locator('.metric-value').allTextContents(), ['2', '8', '6', '128'])
  assert.equal(await page.getByRole('menuitem', { name: '上传版本' }).count(), 0)
  await shot('home-desktop')
  await page.setViewportSize({ width: 390, height: 844 })
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true)
  assert.equal(await page.locator('.overview-stats').evaluate(el => getComputedStyle(el).gridTemplateColumns.split(' ').length), 2)
  await shot('home-mobile')
  await page.setViewportSize({ width: 1536, height: 1024 })
  overview = { packageCount: 0, versionCount: 0, publishedVersionCount: 0, downloadCount: 0 }
  await page.getByRole('button', { name: '刷新', exact: true }).click()
  await page.waitForFunction(() => [...document.querySelectorAll('.metric-value')].every(el => el.textContent === '0'))
  failStats = true
  await page.getByRole('button', { name: '刷新', exact: true }).click()
  await page.getByText('统计加载失败，请刷新重试').waitFor()
  assert.equal(await page.locator('.metric-value').count(), 0)
  failStats = false
  await visit('/packages/demo-tool')
  const copy = page.getByRole('button', { name: '复制下载链接', exact: true })
  await copy.first().waitFor()
  assert.equal(await copy.nth(1).isDisabled(), true)
  assert.equal(await copy.nth(2).isDisabled(), true)
  await copy.first().click()
  assert.equal(await page.evaluate(() => window.__copied), origin + '/gateway/api/packages/demo-tool/versions/1.0.0%2B%E4%B8%AD%E6%96%87/download?platform=windows&arch=x86_64')
  assert.equal(downloadRequests, 0)
  await page.evaluate(() => { window.__clipboardFail = true })
  await copy.first().click()
  await page.getByRole('dialog', { name: '手动复制下载链接' }).waitFor()
  assert.equal(await page.getByRole('textbox', { name: '下载链接', exact: true }).inputValue(), await page.evaluate(() => window.__copied))
  await page.getByRole('dialog').getByRole('button', { name: '关闭', exact: true }).click()
  assert.equal(await page.locator('.tool-description').evaluate(el => getComputedStyle(el).whiteSpace), 'pre-wrap')
  await page.getByRole('button', { name: '编辑信息', exact: true }).click()
  const text = page.getByRole('textbox', { name: '描述', exact: true })
  await text.waitFor()
  await page.waitForFunction(() => document.querySelector('textarea')?.value.includes('第一行'))
  assert.equal(await text.getAttribute('maxlength'), '512')
  await text.fill('更新用途\n多行详细说明')
  await page.getByRole('button', { name: '保存修改', exact: true }).click()
  await page.waitForURL('**/packages/demo-tool')
  await page.getByText('更新用途\n多行详细说明', { exact: true }).waitFor()
  await page.getByRole('button', { name: '上传新版本', exact: true }).click()
  await page.waitForURL('**/packages/demo-tool/upload')
  assert.equal(await page.getByRole('textbox', { name: '描述', exact: true }).count(), 0)
  await shot('upload-desktop')
  await page.getByRole('tab', { name: '可执行文件', exact: true }).click()
  await page.getByRole('textbox', { name: '版本号', exact: true }).fill('2.0.0')
  await page.locator('input[type=file]').nth(1).setInputFiles({ name: 'demo.exe', mimeType: 'application/octet-stream', buffer: Buffer.from('exe') })
  await page.getByRole('button', { name: '生成并上传草稿', exact: true }).click()
  await page.waitForURL('**/packages/demo-tool')
  assert.match(uploadBody, /更新用途\r?\n多行详细说明/)
  await visit('/upload?name=demo-tool')
  await page.waitForURL('**/packages/demo-tool/upload')
  const zip = new JSZip()
  zip.file('manifest.json', JSON.stringify({ name: 'wrong', version: '1.0.0', platform: 'windows', arch: 'x86_64', entry: 'demo.exe', files: [{ path: 'demo.exe', sha256: 'abc' }] }))
  await page.locator('input[type=file]').first().setInputFiles({ name: 'wrong.mty', mimeType: 'application/octet-stream', buffer: await zip.generateAsync({ type: 'nodebuffer' }) })
  await page.getByText('清单中的工具包名称与当前工具包不一致', { exact: true }).waitFor()
  assert.equal(await page.getByRole('button', { name: '上传为草稿', exact: true }).isDisabled(), true)
  zip.file('manifest.json', JSON.stringify({ name: 'demo-tool', description: '旧包中的描述', version: '1.0.0', platform: 'windows', arch: 'x86_64', entry: 'demo.exe', files: [{ path: 'demo.exe', sha256: 'abc' }] }))
  await page.locator('input[type=file]').first().setInputFiles({ name: 'demo.mty', mimeType: 'application/octet-stream', buffer: await zip.generateAsync({ type: 'nodebuffer' }) })
  await page.getByRole('heading', { name: '清单预览', exact: true }).waitFor()
  assert.equal(await page.getByText('旧包中的描述', { exact: true }).count(), 0)
  await page.getByRole('button', { name: '上传为草稿', exact: true }).click()
  await page.waitForURL('**/packages/demo-tool')
  assert.equal(description, '更新用途\n多行详细说明')
  await visit('/upload')
  await page.waitForURL('**/packages')
  assert.deepEqual(errors, [])
  console.log('首页统计/失败状态、移动布局、上传路由、复制降级、多行描述和上传元数据检查通过')
} finally { await browser.close() }
