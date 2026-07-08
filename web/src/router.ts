import { createRouter, createWebHistory } from 'vue-router'
import LoginView from './views/LoginView.vue'
import PackagesView from './views/PackagesView.vue'
import PackageDetailView from './views/PackageDetailView.vue'
import UploadView from './views/UploadView.vue'
import AuditView from './views/AuditView.vue'

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/login', component: LoginView },
    { path: '/', redirect: '/packages' },
    { path: '/packages', component: PackagesView },
    { path: '/packages/new', component: UploadView },
    { path: '/packages/:name/edit', component: UploadView, meta: { editPackage: true } },
    { path: '/packages/:name', component: PackageDetailView },
    { path: '/upload', component: UploadView },
    { path: '/audit', component: AuditView }
  ]
})

router.beforeEach((to) => {
  if (to.path !== '/login' && !localStorage.getItem('mty-token')) {
    return '/login'
  }
})
