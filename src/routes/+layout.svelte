<script lang="ts">
  import { onMount } from "svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { page } from "$app/stores";
  import { api, type CoreEvent, type RunState } from "$lib/api";

  let { children } = $props();

  let runState = $state<RunState>("running");
  let totalCaptures = $state(0);

  async function refreshStatus() {
    try {
      const status = await api.getStatus();
      runState = status.state;
      totalCaptures = status.total_captures;
    } catch {
      // backend not ready yet; the event listener will catch us up
    }
  }

  async function toggle() {
    runState = await api.setRunState(runState !== "running");
  }

  onMount(() => {
    refreshStatus();
    let unlisten: UnlistenFn | undefined;
    listen<CoreEvent>("core-event", (event) => {
      const core = event.payload;
      if (core.type === "state_changed") runState = core.data.state;
      if (core.type === "capture_taken") totalCaptures += 1;
    }).then((fn) => (unlisten = fn));
    return () => unlisten?.();
  });
</script>

<div class="shell">
  <aside class="sidebar">
    <div class="brand">
      <span class="brand-dot" class:paused={runState === "paused"}></span>
      Screeny
    </div>
    <nav>
      <a href="/" class:active={$page.url.pathname === "/"}>Dashboard</a>
      <a href="/settings" class:active={$page.url.pathname.startsWith("/settings")}>Settings</a>
    </nav>
    <div class="sidebar-footer">
      <button class="toggle" onclick={toggle}>
        {runState === "running" ? "Pause" : "Resume"}
      </button>
      <div class="meta">{totalCaptures} captures</div>
    </div>
  </aside>
  <main class="content">
    {@render children()}
  </main>
</div>

<style>
  :global(html, body) {
    margin: 0;
    height: 100%;
    background: #101216;
    color: #e6e8ee;
    font-family: "Segoe UI", system-ui, -apple-system, sans-serif;
  }
  :global(*) {
    box-sizing: border-box;
  }
  .shell {
    display: flex;
    height: 100vh;
  }
  .sidebar {
    width: 190px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    background: #171a21;
    border-right: 1px solid #262b36;
    padding: 16px 12px;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 700;
    font-size: 17px;
    padding: 4px 8px 16px;
  }
  .brand-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: #45d483;
    box-shadow: 0 0 6px #45d48388;
  }
  .brand-dot.paused {
    background: #e0a83c;
    box-shadow: 0 0 6px #e0a83c88;
  }
  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
  }
  nav a {
    color: #aab2c3;
    text-decoration: none;
    padding: 8px 10px;
    border-radius: 8px;
    font-size: 14px;
  }
  nav a:hover {
    background: #1f2430;
    color: #e6e8ee;
  }
  nav a.active {
    background: #26304a;
    color: #ffffff;
  }
  .sidebar-footer {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .toggle {
    background: #2a3350;
    color: #e6e8ee;
    border: 1px solid #39456b;
    border-radius: 8px;
    padding: 8px;
    cursor: pointer;
    font-size: 14px;
  }
  .toggle:hover {
    background: #344066;
  }
  .meta {
    font-size: 12px;
    color: #7c8598;
    text-align: center;
  }
  .content {
    flex: 1;
    overflow-y: auto;
    padding: 20px 24px;
  }
</style>
