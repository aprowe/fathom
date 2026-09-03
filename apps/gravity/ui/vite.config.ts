import { fileURLToPath, URL } from 'node:url'

import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

const resolve = (path: string) => fileURLToPath(new URL(path, import.meta.url))

export default defineConfig({
  plugins: [react()],
  resolve: {
    // The UI runtime is consumed as source: there is no build step between editing a
    // control and seeing it, which is the point of keeping it in the same repo.
    alias: { '@fathom/ui': resolve('../../../packages/fathom-ui/src/index.ts') },
  },
  server: {
    // The generated wasm package sits outside this app's root.
    fs: { allow: [resolve('../../..')] },
  },
})
