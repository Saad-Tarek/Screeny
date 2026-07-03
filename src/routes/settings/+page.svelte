<script lang="ts">
  import { onMount } from "svelte";
  import { api, type Config } from "$lib/api";

  let config = $state<Config | null>(null);
  let saving = $state(false);
  let savedAt = $state<number | null>(null);
  let error = $state<string | null>(null);
  let keepForever = $state(false);

  let passwordInput = $state("");
  let passwordSaved = $state(false);
  let autostart = $state(false);
  let testState = $state<"idle" | "sending" | "ok" | "failed">("idle");
  let testError = $state<string | null>(null);

  let apiKeyInput = $state("");
  let apiKeySaved = $state(false);
  let models = $state<string[]>([]);
  let modelsError = $state<string | null>(null);
  let loadingModels = $state(false);

  let tgTokenInput = $state("");
  let tgTokenSaved = $state(false);
  let tgTestState = $state<"idle" | "sending" | "ok" | "failed">("idle");
  let tgTestError = $state<string | null>(null);
  let tgChats = $state<import("$lib/api").DiscoveredChat[]>([]);
  let tgDiscoverError = $state<string | null>(null);
  let tgDiscovering = $state(false);

  onMount(async () => {
    try {
      const loaded = await api.getConfig();
      keepForever = loaded.capture.retention_days === null;
      config = loaded;
      passwordSaved = await api.emailPasswordSet();
      autostart = await api.getAutostart();
      apiKeySaved = await api.llmApiKeySet();
      tgTokenSaved = await api.telegramTokenSet();
      if (loaded.llm.enabled) refreshModels();
    } catch (e) {
      error = String(e);
    }
  });

  function backendChanged() {
    if (!config) return;
    config.llm.base_url = "";
    models = [];
    modelsError = null;
  }

  async function refreshModels() {
    const pending = pendingConfig();
    if (!pending) return;
    loadingModels = true;
    modelsError = null;
    try {
      if (apiKeyInput.trim()) {
        await api.setLlmApiKey(apiKeyInput);
        apiKeyInput = "";
        apiKeySaved = true;
      }
      models = await api.listModels(pending);
      if (models.length === 0) {
        modelsError = "Connected, but no models are installed on this backend.";
      }
    } catch (e) {
      models = [];
      modelsError = String(e);
    } finally {
      loadingModels = false;
    }
  }

  function pendingConfig(): Config | null {
    if (!config) return null;
    return {
      ...config,
      capture: {
        ...config.capture,
        retention_days: keepForever ? null : (config.capture.retention_days ?? 30),
      },
    };
  }

  async function save(event: Event) {
    event.preventDefault();
    const toSave = pendingConfig();
    if (!toSave) return;
    saving = true;
    error = null;
    try {
      if (passwordInput.trim()) {
        await api.setEmailPassword(passwordInput);
        passwordInput = "";
        passwordSaved = true;
      }
      config = await api.setConfig(toSave);
      keepForever = config.capture.retention_days === null;
      savedAt = Date.now();
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }

  async function toggleAutostart() {
    try {
      await api.setAutostart(autostart);
    } catch (e) {
      error = String(e);
      autostart = !autostart;
    }
  }

  async function saveTgTokenIfTyped() {
    if (tgTokenInput.trim()) {
      await api.setTelegramToken(tgTokenInput);
      tgTokenInput = "";
      tgTokenSaved = true;
    }
  }

  async function tgDiscover() {
    tgDiscovering = true;
    tgDiscoverError = null;
    try {
      await saveTgTokenIfTyped();
      tgChats = await api.telegramDiscoverChats();
      if (tgChats.length === 0) {
        tgDiscoverError =
          "No chats found. Open Telegram, send your bot any message, then try again.";
      } else if (config && !config.channels.telegram.chat_id) {
        config.channels.telegram.chat_id = String(tgChats[0].id);
      }
    } catch (e) {
      tgDiscoverError = String(e);
    } finally {
      tgDiscovering = false;
    }
  }

  async function sendTgTest() {
    const pending = pendingConfig();
    if (!pending) return;
    tgTestState = "sending";
    tgTestError = null;
    try {
      await saveTgTokenIfTyped();
      await api.testTelegram(pending);
      tgTestState = "ok";
    } catch (e) {
      tgTestState = "failed";
      tgTestError = String(e);
    }
  }

  async function sendTest() {
    const pending = pendingConfig();
    if (!pending) return;
    testState = "sending";
    testError = null;
    try {
      // Save a freshly typed password first so the test uses it.
      if (passwordInput.trim()) {
        await api.setEmailPassword(passwordInput);
        passwordInput = "";
        passwordSaved = true;
      }
      await api.testEmail(pending);
      testState = "ok";
    } catch (e) {
      testState = "failed";
      testError = String(e);
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

    <section>
      <h2>AI analysis</h2>

      <label class="field row">
        <input type="checkbox" bind:checked={config.llm.enabled} />
        <span>Analyze captures with a vision model (OCR + description)</span>
      </label>

      {#if config.llm.enabled}
        <label class="field">
          <span>Backend</span>
          <select bind:value={config.llm.backend} onchange={backendChanged}>
            <option value="ollama">Ollama (local, private — recommended)</option>
            <option value="lmstudio">LM Studio (local, private)</option>
            <option value="custom">OpenAI-compatible API (cloud)</option>
          </select>
          {#if config.llm.backend === "custom"}
            <small>
              Cloud APIs see your screenshots. For privacy, prefer a local backend.
            </small>
          {/if}
        </label>

        <label class="field">
          <span>Server URL</span>
          <input
            type="text"
            placeholder={config.llm.backend === "ollama"
              ? "http://localhost:11434"
              : config.llm.backend === "lmstudio"
                ? "http://localhost:1234"
                : "https://api.openai.com"}
            bind:value={config.llm.base_url}
          />
        </label>

        {#if config.llm.backend === "custom"}
          <label class="field">
            <span>API key {apiKeySaved ? "(saved in system keychain)" : ""}</span>
            <input
              type="password"
              placeholder={apiKeySaved ? "••••••••  — type to replace" : "sk-…"}
              bind:value={apiKeyInput}
            />
          </label>
        {/if}

        <div class="test-row">
          <button
            type="button"
            class="secondary"
            onclick={refreshModels}
            disabled={loadingModels}
          >
            {loadingModels ? "Connecting…" : "Connect & list models"}
          </button>
          {#if models.length > 0}
            <span class="saved">Connected ✓ ({models.length} models)</span>
          {/if}
        </div>
        {#if modelsError}
          <span class="test-failed">{modelsError}</span>
        {/if}

        <label class="field">
          <span>Vision model</span>
          {#if models.length > 0}
            <select bind:value={config.llm.model}>
              {#if config.llm.model && !models.includes(config.llm.model)}
                <option value={config.llm.model}>{config.llm.model} (not installed)</option>
              {/if}
              {#each models as model (model)}
                <option value={model}>{model}</option>
              {/each}
            </select>
          {:else}
            <input type="text" placeholder="e.g. moondream" bind:value={config.llm.model} />
          {/if}
          <small>
            Recommended: moondream (small/fast) · qwen2.5vl:3b (balanced) ·
            qwen2.5vl:7b (best OCR).
          </small>
        </label>
      {/if}
    </section>

    <section>
      <h2>Email delivery</h2>

      <label class="field row">
        <input type="checkbox" bind:checked={config.channels.email.enabled} />
        <span>Email each capture batch</span>
      </label>

      {#if config.channels.email.enabled}
        <div class="grid-2">
          <label class="field">
            <span>SMTP host</span>
            <input type="text" placeholder="smtp.gmail.com" bind:value={config.channels.email.smtp_host} />
          </label>
          <label class="field">
            <span>Port</span>
            <input type="number" min="1" max="65535" bind:value={config.channels.email.smtp_port} />
          </label>
        </div>

        <label class="field">
          <span>Connection security</span>
          <select bind:value={config.channels.email.security}>
            <option value="ssl">SSL / TLS (port 465)</option>
            <option value="starttls">STARTTLS (port 587)</option>
          </select>
        </label>

        <label class="field">
          <span>Username</span>
          <input type="text" placeholder="you@gmail.com" bind:value={config.channels.email.username} />
        </label>

        <label class="field">
          <span>Password {passwordSaved ? "(saved in system keychain)" : ""}</span>
          <input
            type="password"
            placeholder={passwordSaved ? "••••••••  — type to replace" : "App password"}
            bind:value={passwordInput}
          />
          <small>
            For Gmail, create an App Password (requires 2-Step Verification).
            Stored in your OS keychain, never in a file.
          </small>
        </label>

        <div class="grid-2">
          <label class="field">
            <span>From address</span>
            <input type="email" placeholder="you@gmail.com" bind:value={config.channels.email.from} />
          </label>
          <label class="field">
            <span>To address</span>
            <input type="email" placeholder="you@gmail.com" bind:value={config.channels.email.to} />
          </label>
        </div>

        <label class="field">
          <span>Email content</span>
          <select bind:value={config.channels.email.content}>
            <option value="image">Screenshots only</option>
            <option value="analysis">AI analysis text only (smallest emails)</option>
            <option value="both">Screenshots + AI analysis</option>
          </select>
          {#if config.channels.email.content !== "image" && !config.llm.enabled}
            <small class="test-failed">
              AI analysis is disabled above — emails will say "no analysis available".
            </small>
          {/if}
        </label>

        <label class="field">
          <span>Screenshots per email: {config.channels.email.batch_size}</span>
          <input type="range" min="1" max="60" bind:value={config.channels.email.batch_size} />
          <small>
            Raise this to bundle shots and stay under provider limits
            (Gmail allows ~500 emails/day).
          </small>
        </label>

        <div class="test-row">
          <button type="button" class="secondary" onclick={sendTest} disabled={testState === "sending"}>
            {testState === "sending" ? "Sending…" : "Send test email"}
          </button>
          {#if testState === "ok"}
            <span class="saved">Test sent ✓ — check your inbox</span>
          {:else if testState === "failed"}
            <span class="test-failed">{testError}</span>
          {/if}
        </div>
      {/if}
    </section>

    <section>
      <h2>Telegram delivery</h2>

      <label class="field row">
        <input type="checkbox" bind:checked={config.channels.telegram.enabled} />
        <span>Send each capture to a Telegram chat</span>
      </label>

      {#if config.channels.telegram.enabled}
        <label class="field">
          <span>Bot token {tgTokenSaved ? "(saved in system keychain)" : ""}</span>
          <input
            type="password"
            placeholder={tgTokenSaved ? "••••••••  — type to replace" : "123456:ABC-DEF…"}
            bind:value={tgTokenInput}
          />
          <small>
            Create a bot in Telegram by messaging <strong>@BotFather</strong> →
            /newbot. Paste the token here, then send your new bot any message.
          </small>
        </label>

        <div class="grid-2">
          <label class="field">
            <span>Chat ID</span>
            <input type="text" placeholder="e.g. 123456789" bind:value={config.channels.telegram.chat_id} />
          </label>
          <div class="field">
            <span>&nbsp;</span>
            <button type="button" class="secondary" onclick={tgDiscover} disabled={tgDiscovering}>
              {tgDiscovering ? "Looking…" : "Detect chat ID"}
            </button>
          </div>
        </div>
        {#if tgChats.length > 0}
          <div class="chip-row">
            {#each tgChats as chat (chat.id)}
              <button
                type="button"
                class="chip"
                class:selected={config.channels.telegram.chat_id === String(chat.id)}
                onclick={() => {
                  if (config) config.channels.telegram.chat_id = String(chat.id);
                }}
              >
                {chat.label} ({chat.id})
              </button>
            {/each}
          </div>
        {/if}
        {#if tgDiscoverError}
          <span class="test-failed">{tgDiscoverError}</span>
        {/if}

        <label class="field">
          <span>Message content</span>
          <select bind:value={config.channels.telegram.content}>
            <option value="image">Screenshots only</option>
            <option value="analysis">AI analysis text only</option>
            <option value="both">Screenshot with AI caption</option>
          </select>
        </label>

        <div class="test-row">
          <button type="button" class="secondary" onclick={sendTgTest} disabled={tgTestState === "sending"}>
            {tgTestState === "sending" ? "Sending…" : "Send test message"}
          </button>
          {#if tgTestState === "ok"}
            <span class="saved">Test sent ✓ — check Telegram</span>
          {:else if tgTestState === "failed"}
            <span class="test-failed">{tgTestError}</span>
          {/if}
        </div>
      {/if}
    </section>

    <section>
      <h2>System</h2>
      <label class="field row">
        <input type="checkbox" bind:checked={autostart} onchange={toggleAutostart} />
        <span>Start Screeny automatically at login</span>
      </label>
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
  .grid-2 {
    display: grid;
    grid-template-columns: 2fr 1fr;
    gap: 10px;
  }
  .test-row {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
  }
  .secondary {
    background: #1f2430;
    color: #aab2c3;
    border: 1px solid #2c3342;
    border-radius: 8px;
    padding: 8px 14px;
    font-size: 14px;
    cursor: pointer;
  }
  .secondary:hover:not(:disabled) {
    color: #e6e8ee;
  }
  .test-failed {
    color: #f2b8bf;
    font-size: 13px;
  }
  .chip-row {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .chip {
    background: #10131a;
    border: 1px solid #2c3342;
    color: #aab2c3;
    border-radius: 999px;
    padding: 5px 12px;
    font-size: 13px;
    cursor: pointer;
  }
  .chip.selected {
    border-color: #2f6feb;
    color: #ffffff;
    background: #1c2947;
  }
  input[type="text"],
  input[type="email"],
  input[type="password"],
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
