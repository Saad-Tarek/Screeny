<script lang="ts">
  import { onMount } from "svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { goto } from "$app/navigation";
  import { page } from "$app/stores";
  import { check, type Update } from "@tauri-apps/plugin-updater";
  import { relaunch } from "@tauri-apps/plugin-process";
  import { api, type CoreEvent, type RunState } from "$lib/api";

  const UPDATE_CHECK_INTERVAL_MS = 4 * 60 * 60 * 1000;

  let { children } = $props();

  let runState = $state<RunState>("running");
  let totalCaptures = $state(0);
  let sidebarOpen = $state(true);
  let availableUpdate = $state<Update | null>(null);
  let updateState = $state<"idle" | "installing" | "failed">("idle");

  async function checkForUpdate() {
    try {
      availableUpdate = await check();
    } catch {
      // offline, or a dev build with no published release — try again later
    }
  }

  async function installUpdate() {
    if (!availableUpdate || updateState === "installing") return;
    updateState = "installing";
    try {
      await availableUpdate.downloadAndInstall();
      await relaunch();
    } catch {
      updateState = "failed";
    }
  }

  const onboarding = $derived($page.url.pathname.startsWith("/onboarding"));

  function toggleSidebar() {
    sidebarOpen = !sidebarOpen;
    try {
      localStorage.setItem("sidebar-open", String(sidebarOpen));
    } catch {
      // storage unavailable; the toggle still works for this session
    }
  }

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
    try {
      sidebarOpen = localStorage.getItem("sidebar-open") !== "false";
    } catch {
      // storage unavailable; keep the default
    }
    refreshStatus();
    checkForUpdate();
    const updateTimer = setInterval(checkForUpdate, UPDATE_CHECK_INTERVAL_MS);
    api
      .getConfig()
      .then((config) => {
        if (!config.onboarding_complete) goto("/onboarding");
      })
      .catch(() => {});
    let unlisten: UnlistenFn | undefined;
    listen<CoreEvent>("core-event", (event) => {
      const core = event.payload;
      if (core.type === "state_changed") runState = core.data.state;
      if (core.type === "capture_taken") totalCaptures += 1;
    }).then((fn) => (unlisten = fn));
    return () => {
      clearInterval(updateTimer);
      unlisten?.();
    };
  });
</script>

<div class="shell">
  <aside class="sidebar" class:hidden={onboarding} class:collapsed={!sidebarOpen}>
    <div class="brand">
      <button
        class="burger"
        onclick={toggleSidebar}
        aria-label={sidebarOpen ? "Collapse sidebar" : "Expand sidebar"}
        title={sidebarOpen ? "Collapse sidebar" : "Expand sidebar"}
      >
        ☰
      </button>
      {#if sidebarOpen}
        <span class="brand-dot" class:paused={runState === "paused"}></span>
        Screeny
      {/if}
    </div>
    {#if sidebarOpen}
      <nav>
        <a href="/" class:active={$page.url.pathname === "/"}>Dashboard</a>
        <a href="/settings" class:active={$page.url.pathname.startsWith("/settings")}>Settings</a>
      </nav>
      <div class="sidebar-footer">
        {#if availableUpdate}
          <button class="update" onclick={installUpdate} disabled={updateState === "installing"}>
            {#if updateState === "installing"}
              Installing update…
            {:else if updateState === "failed"}
              Update failed — retry
            {:else}
              ⬆ Update to v{availableUpdate.version}
            {/if}
          </button>
        {/if}
        <button class="toggle" onclick={toggle}>
          {runState === "running" ? "Pause" : "Resume"}
        </button>
        <div class="meta">{totalCaptures} captures</div>
      </div>
    {:else}
      {#if availableUpdate}
        <button
          class="rail-update"
          onclick={installUpdate}
          disabled={updateState === "installing"}
          aria-label={`Install update v${availableUpdate.version}`}
          title={`Install update v${availableUpdate.version}`}
        >
          ⬆
        </button>
      {/if}
      <span
        class="brand-dot rail-dot"
        class:paused={runState === "paused"}
        title={runState === "running" ? "Capturing" : "Paused"}
      ></span>
    {/if}
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
  .sidebar.hidden {
    display: none;
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
  .sidebar.collapsed {
    width: 52px;
    align-items: center;
    padding: 16px 8px;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 700;
    font-size: 17px;
    padding: 4px 0 16px;
  }
  .burger {
    background: none;
    border: none;
    color: #aab2c3;
    font-size: 17px;
    padding: 2px 6px;
    border-radius: 6px;
    cursor: pointer;
    line-height: 1;
  }
  .burger:hover {
    background: #1f2430;
    color: #e6e8ee;
  }
  .rail-dot {
    margin-top: 4px;
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
  .update {
    background: #2f6feb;
    color: #ffffff;
    border: none;
    border-radius: 8px;
    padding: 8px;
    cursor: pointer;
    font-size: 13px;
    font-weight: 600;
  }
  .update:hover:not(:disabled) {
    background: #3d7cf5;
  }
  .update:disabled {
    opacity: 0.7;
    cursor: default;
  }
  .rail-update {
    background: #2f6feb;
    color: #ffffff;
    border: none;
    border-radius: 8px;
    width: 30px;
    height: 30px;
    cursor: pointer;
    font-size: 14px;
    margin-top: 4px;
  }
  .rail-update:hover:not(:disabled) {
    background: #3d7cf5;
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
