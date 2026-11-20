import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';

export default defineConfig({
  plugins: [vue()],
  server: {
    port: 1450,
    strictPort: true,
    watch: {
      ignored: ['**/node_modules/**', '**/target/**', '**/crates/**', '**/dist/**']
    }
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
});
