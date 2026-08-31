import { createRouter, createWebHistory } from 'vue-router'

import DashboardView from '../views/DashboardView.vue'
import ProjectsView from '../views/ProjectsView.vue'
import DatabasesView from '../views/DatabasesView.vue'
import SwitchView from '../views/SwitchView.vue'
import LogsView from '../views/LogsView.vue'
import SettingsView from '../views/SettingsView.vue'

// Imported eagerly rather than as `() => import(...)`. Code-splitting pays off
// when chunks travel over a network and most users never open most routes —
// neither is true here: six screens, all bundled into the installer, all on
// local disk. Lazy loading only bought a blank frame on each route's first
// visit, so the whole app is loaded up front and every menu switch is instant.
const router = createRouter({
  history: createWebHistory(import.meta.env.BASE_URL),
  routes: [
    {
      path: '/',
      name: 'dashboard',
      component: DashboardView,
    },
    {
      path: '/projects',
      name: 'projects',
      component: ProjectsView,
    },
    {
      path: '/databases',
      name: 'databases',
      component: DatabasesView,
    },
    {
      path: '/switch',
      name: 'switch',
      component: SwitchView,
    },
    {
      path: '/logs',
      name: 'logs',
      component: LogsView,
    },
    {
      path: '/settings',
      name: 'settings',
      component: SettingsView,
    },
  ],
})

export default router
