// https://nuxt.com/docs/api/configuration/nuxt-config
export default defineNuxtConfig({
  telemetry: false,
  compatibilityDate: '2025-07-15',
  devtools: { enabled: true },
  modules: ['convex-nuxt'],
  convex: {
    url: process.env.CONVEX_URL
  }
})