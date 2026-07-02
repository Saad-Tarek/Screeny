<script lang="ts">
  import { onMount } from "svelte";
  import { api, type Config } from "$lib/api";

  let config = $state<Config | null>(null);
  let saving = $state(false);
  let savedAt = $state<number | null>(null);
  let error = $state<string | null>(null);
  let keepForever = $state(false);

  onMount(async () => {
    try {
      const loaded = await api.getConfig();
      keepForever = loaded.capture.retention_days === null;
      config = loaded;
    } catch (e) {
      error = String(e);
    }
  });

  async function save(event: Event) {
    event.preventDefault();
    if (!config) return;
    saving = true;
    error = null;
    try {
      const toSave: Config = {
        ...config,
        capture: {
          ...config.capture,
          retention_days: keepForever ? null : (config.capture.retention_days ?? 30),
        },
      };
      config = await api.setConfig(toSave);
      keepForever = config.capture.retention_days === null;
      savedAt = Date.now();
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }
</script>

<h1>Settings</h1>

{#if error}
  <div class="error">{error}</div>
{/if}

{#if config}
  <form onsubmit={save}>
    <section>
      <h2>Capture</h2>

      <label class="field">
        <span>Interval between screenshots (seconds)</span>
        <input
          type="number"
          min="5"
          max="3600"
          bind:value={config.capture.interval_seconds}
        />
        <small>Minimum 5 seconds. {Math.round(3600 / config.capture.interval_seconds)} captures/hour.</small>
      </label>

      <label class="field">
        <span>Image format</span>
        <select bind:value={config.capture.format}>
          <option value="jpeg">JPEG — smaller files (recommended)</option>
          <option value="png">PNG — lossless, larger files</option>
        </select>
      </label>

      {#if config.capture.format === "jpeg"}
        <label class="field">
          <span>JPEG quality: {config.capture.jpeg_quality}</span>
          <input
            type="range"
            min="30"
            max="100"
            bind:value={config.capture.jpeg_quality}
          />
        </label>
      {/if}

      <label class="field row">
        <input type="checkbox" bind:checked={config.capture.start_on_launch} />
        <span>Start capturing when Screeny launches</span>
      </label>
    </section>

    <section>
      <h2>Storage</h2>
      <label class="field row">
        <input type="checkbox" bind:checked={keepForever} />
        <span>Keep captures forever</span>
      </label>
      {#if !keepForever}
        <label class="field">
          <span>Delete captures older than (days)</span>
          <input
            type="number"
            min="1"
            max="3650"
            value={config.capture.retention_days ?? 30}
            oninput={(e) => {
              if (config)
                config.capture.retention_days = Number(e.currentTarget.value) || 30;
            }}
          />
        </label>
      {/if}
    </section>

    <div class="actions">
      <button type="submit" class="primary" disabled={saving}>
        {saving ? "Saving…" : "Save settings"}
      </button>
      {#if savedAt}
        <span class="saved">Saved ✓</span>
      {/if}
    </div>
  </form>
{:else if !error}
  <p class="loading">Loading…</p>
{/if}

<style>
  h1 {
    font-size: 20px;
    margin: 0 0 16px;
  }
  h2 {
    font-size: 14px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: #8b93a7;
    margin: 0 0 12px;
  }
  form {
    max-width: 480px;
    display: flex;
    flex-direction: column;
    gap: 24px;
  }
  section {
    background: #171a21;
    border: 1px solid #262b36;
    border-radius: 12px;
    padding: 18px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 14px;
  }
  .field.row {
    flex-direction: row;
    align-items: center;
    gap: 10px;
  }
  .field small {
    color: #7c8598;
  }
  input[type="number"],
  select {
    background: #10131a;
    border: 1px solid #2c3342;
    color: #e6e8ee;
    border-radius: 8px;
    padding: 8px 10px;
    font-size: 14px;
    width: 100%;
  }
  input[type="checkbox"] {
    width: 16px;
    height: 16px;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .primary {
    background: #2f6feb;
    color: white;
    border: none;
    border-radius: 8px;
    padding: 9px 18px;
    font-size: 14px;
    cursor: pointer;
  }
  .primary:hover:not(:disabled) {
    background: #3d7cf5;
  }
  .primary:disabled {
    opacity: 0.6;
  }
  .saved {
    color: #45d483;
    font-size: 14px;
  }
  .error {
    background: #3a1d20;
    border: 1px solid #74343c;
    color: #f2b8bf;
    border-radius: 8px;
    padding: 10px 14px;
    margin-bottom: 16px;
    font-size: 14px;
  }
  .loading {
    color: #8b93a7;
  }
</style>
