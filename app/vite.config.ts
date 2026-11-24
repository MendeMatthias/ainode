import { defineConfig } from "vite";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// Port 1450 so AInode, easyNode (1430, previews on 1440) and the miner can run `tauri dev` side by side.
export default defineConfig(async () => ({
  clearScreen: false,
  // The offline library runs in a module worker (src/knowledge/worker.ts).
  worker: { format: "es" },
  server: {
    port: 1450,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1451 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
}));
