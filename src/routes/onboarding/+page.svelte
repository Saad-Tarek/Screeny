<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import {
    api,
    RECOMMENDED_MODELS,
    type Config,
    type CoreEvent,
    type DetectResult,
    type PullProgressEvent,
  } from "$lib/api";

  type Step = "welcome" | "capture" | "ai" | "finish";
  const STEPS: Step[] = ["welcome", "capture", "ai", "finish"];

  let step = $state<Step>("welcome");
  let config = $state<Config | null>(null);
  let error = $state<string | null>(null);

  // capture check
  let captureState = $state<"idle" | "trying" | "ok" | "failed">("idle");
  let capturePreview = $state<string | null>(null);
  let captureError = $state<string | null>(null);

  // AI step
  let detect = $state<DetectResult | null>(null);
  let detecting = $state(false);
  let aiChoice = $state<"local" | "skip">("local");
  let chosenBackend = $state<"ollama" | "lmstudio">("ollama");
  let chosenModel = $state("");
  let pull = $state<{ model: string; percent: number | null; status: string } | null>(null);
  let pullDone = $state(false);
  let pullError = $state<string | null>(null);

  // finish step
  let autostart = $state(true);
  let finishing = $state(false);

  let unlistenCapture: UnlistenFn | undefined;
  let unlistenPull: UnlistenFn | undefined;

  onMount(() => {
    api.getConfig().then((c) => (config = c)).catch((e) => (error = String(e)));
    listen<CoreEvent>("core-event", (event) => {
      const core = event.payload;
      if (core.type === "capture_taken" && captureState === "trying") {
        captureState = "ok";
        capturePreview = convertFileSrc(core.data.path);
      } else if (core.type === "capture_failed" && captureState === "trying") {
        captureState = "failed";
        captureError = core.data.message;
      }
    }).then((fn) => (unlistenCapture = fn));
    listen<PullProgressEvent>("pull-progress", (event) => {
      const p = event.payload;
      pull = {
        model: p.model,
        status: p.status,
        percent:
          p.total && p.completed ? Math.round((p.completed / p.total) * 100) : null,
      };
    }).then((fn) => (unlistenPull = fn));
    return () => {
      unlistenCapture?.();
      unlistenPull?.();
    };
  });

  function next() {
    const i = STEPS.indexOf(step);
    step = STEPS[Math.min(i + 1, STEPS.length - 1)];
    if (step === "ai" && !detect) runDetect();
  }

  async function tryCapture() {
    captureState = "trying";
    captureError = null;
    try {
      await api.captureNow();
      // result arrives via the core-event listener
    } catch (e) {
      captureState = "failed";
      captureError = String(e);
    }
  }

  async function runDetect() {
    detecting = true;
    try {
      detect = await api.detectBackends();
      if (detect.ollama) {
        chosenBackend = "ollama";
        const visionInstalled = detect.ollama.find((m) =>
          RECOMMENDED_MODELS.some((r) => m.startsWith(r.tag.split(":")[0]))
        );
        chosenModel = visionInstalled ?? RECOMMENDED_MODELS[0].tag;
      } else if (detect.lmstudio) {
        chosenBackend = "lmstudio";
        chosenModel = detect.lmstudio[0] ?? "";
      }
    } catch (e) {
      error = String(e);
    } finally {
      detecting = false;
    }
  }

  async function pullChosenModel() {
    if (!chosenModel) return;
    pullError = null;
    pullDone = false;
    pull = { model: chosenModel, status: "starting…", percent: null };
    try {
      await api.pullModel(chosenModel);
      pullDone = true;
    } catch (e) {
      pullError = String(e);
    } finally {
      pull = null;
    }
  }

  const modelInstalled = $derived(
    !!detect?.ollama?.some((m) => m === chosenModel || m.startsWith(chosenModel + ":"))
  );

  async function finish() {
    if (!config) return;
    finishing = true;
    error = null;
    try {
      const backendDetected =
        chosenBackend === "ollama" ? !!detect?.ollama : !!detect?.lmstudio;
      const useAi = aiChoice === "local" && backendDetected && !!chosenModel;
      const updated: Config = {
        ...config,
        onboarding_complete: true,
        llm: useAi
          ? {
              ...config.llm,
              enabled: true,
              backend: chosenBackend,
              base_url:
                chosenBackend === "ollama"
                  ? "http://localhost:11434"
                  : "http://localhost:1234",
              model: chosenModel,
            }
          : config.llm,
      };
      await api.setConfig(updated);
      await api.setAutostart(autostart).catch(() => {});
      goto("/");
    } catch (e) {
      error = String(e);
    } finally {
      finishing = false;
    }
  }
</script>

<div class="wizard">
  <div class="progress">
    {#each STEPS as s, i (s)}
      <span class="dot" class:active={STEPS.indexOf(step) >= i}></span>
    {/each}
  </div>

  {#if error}
    <div class="error">{error}</div>
  {/if}

  {#if step === "welcome"}
    <h1>Welcome to Screeny</h1>
    <p>
      Screeny quietly takes a screenshot of your screen on a schedule, keeps a
      private local archive you can browse and search, and can optionally
      describe each capture with a local AI model or send captures to your inbox.
    </p>
    <p class="privacy">
      🔒 <strong>Private by default.</strong> Everything stays on this computer
      unless you turn on a delivery channel. Passwords and API keys live in your
      operating system's keychain.
    </p>
    <div class="nav">
      <button class="primary" onclick={next}>Get started</button>
    </div>
  {:else if step === "capture"}
    <h1>Test screen capture</h1>
    <p>Let's make sure Screeny can see your screen.</p>

    {#if captureState === "ok" && capturePreview}
      <img class="preview" src={capturePreview} alt="Test capture" />
      <p class="ok">Capture works ✓</p>
    {:else if captureState === "failed"}
      <div class="error">
        {captureError}
        <p class="hint">
          On macOS: System Settings → Privacy &amp; Security → Screen Recording →
          enable Screeny, then relaunch the app and try again.
        </p>
      </div>
    {/if}

    <div class="nav">
      <button class="secondary" onclick={tryCapture} disabled={captureState === "trying"}>
        {captureState === "trying"
          ? "Capturing…"
          : captureState === "ok"
            ? "Capture again"
            : "Take a test screenshot"}
      </button>
      <button class="primary" onclick={next} disabled={captureState !== "ok"}>Continue</button>
      <button class="link" onclick={next}>Skip</button>
    </div>
  {:else if step === "ai"}
    <h1>AI analysis (optional, fully local)</h1>
    <p>
      A small vision model can read the text on each screenshot and describe it,
      making your whole archive searchable — without anything leaving this
      computer.
    </p>

    {#if detecting}
      <p class="hint">Looking for Ollama and LM Studio on this machine…</p>
    {:else if detect}
      {#if detect.ollama}
        <p class="ok">Ollama detected ✓ ({detect.ollama.length} models installed)</p>
        <label class="field">
          <span>Model to use</span>
          <select bind:value={chosenModel}>
            {#each RECOMMENDED_MODELS as rec (rec.tag)}
              <option value={rec.tag}>{rec.label}</option>
            {/each}
            {#each detect.ollama.filter((m) => !RECOMMENDED_MODELS.some((r) => r.tag === m)) as m (m)}
              <option value={m}>{m} (installed)</option>
            {/each}
          </select>
          <small>
            {RECOMMENDED_MODELS.find((r) => r.tag === chosenModel)?.blurb ?? ""}
          </small>
        </label>

        {#if !modelInstalled}
          {#if pull}
            <div class="pull">
              <div class="bar">
                <div
                  class="fill"
                  style={`transform: scaleX(${(pull.percent ?? 3) / 100})`}
                ></div>
              </div>
              <small>{pull.status} {pull.percent !== null ? `— ${pull.percent}%` : ""}</small>
            </div>
          {:else if pullDone}
            <p class="ok">Model downloaded ✓</p>
          {:else}
            <button class="secondary" onclick={pullChosenModel}>
              Download {chosenModel} now
            </button>
          {/if}
          {#if pullError}
            <div class="error">{pullError}</div>
          {/if}
        {:else}
          <p class="ok">Model already installed ✓</p>
        {/if}
      {:else if detect.lmstudio}
        <p class="ok">LM Studio detected ✓ ({detect.lmstudio.length} models loaded)</p>
        <label class="field">
          <span>Model to use</span>
          <select bind:value={chosenModel}>
            {#each detect.lmstudio as m (m)}
              <option value={m}>{m}</option>
            {/each}
          </select>
          <small>
            ⚠ Screenshots need a <strong>vision</strong> model (e.g. Qwen2.5-VL,
            LLaVA, Gemma 3 vision). Text or code models cannot read images — if
            none of these are vision models, download one in LM Studio first,
            or skip and set this up later.
          </small>
        </label>
      {:else}
        <p>No local AI backend found.</p>
        <p class="hint">
          Install <button class="link" onclick={() => openUrl("https://ollama.com/download")}>Ollama (free)</button>,
          start it, then click detect again — or skip and set this up later in Settings.
        </p>
        <button class="secondary" onclick={runDetect}>Detect again</button>
      {/if}
    {/if}

    <div class="nav">
      <button
        class="primary"
        onclick={() => {
          aiChoice = "local";
          next();
        }}
        disabled={!chosenModel ||
          (chosenBackend === "ollama"
            ? !detect?.ollama || (!modelInstalled && !pullDone)
            : !detect?.lmstudio)}
      >
        Enable AI analysis
      </button>
      <button
        class="link"
        onclick={() => {
          aiChoice = "skip";
          next();
        }}
      >
        Skip for now
      </button>
    </div>
  {:else if step === "finish"}
    <h1>Almost done</h1>
    <label class="field row">
      <input type="checkbox" bind:checked={autostart} />
      <span>Start Screeny automatically when I log in</span>
    </label>
    <p class="hint">
      Email or other delivery channels can be configured anytime in Settings.
      Screeny lives in your system tray — closing the window keeps it running.
    </p>
    <div class="nav">
      <button class="primary" onclick={finish} disabled={finishing || !config}>
        {finishing ? "Finishing…" : "Start capturing"}
      </button>
    </div>
  {/if}
</div>

<style>
  .wizard {
    max-width: 560px;
    margin: 40px auto;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .progress {
    display: flex;
    gap: 8px;
    margin-bottom: 8px;
  }
  .dot {
    width: 34px;
    height: 5px;
    border-radius: 3px;
    background: #262b36;
  }
  .dot.active {
    background: #2f6feb;
  }
  h1 {
    font-size: 22px;
    margin: 0;
  }
  p {
    margin: 0;
    line-height: 1.55;
    color: #c6ccd9;
  }
  .privacy {
    background: #171a21;
    border: 1px solid #262b36;
    border-radius: 10px;
    padding: 12px 14px;
  }
  .nav {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 10px;
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
  .primary:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .primary:hover:not(:disabled) {
    background: #3d7cf5;
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
  .link {
    background: none;
    border: none;
    color: #7ea6f8;
    cursor: pointer;
    font-size: 14px;
    padding: 0;
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
  .field small,
  .hint {
    color: #8b93a7;
    font-size: 13px;
  }
  select {
    background: #10131a;
    border: 1px solid #2c3342;
    color: #e6e8ee;
    border-radius: 8px;
    padding: 8px 10px;
    font-size: 14px;
  }
  .preview {
    max-width: 100%;
    border-radius: 10px;
    border: 1px solid #2c3342;
  }
  .ok {
    color: #45d483;
  }
  .error {
    background: #3a1d20;
    border: 1px solid #74343c;
    color: #f2b8bf;
    border-radius: 8px;
    padding: 10px 14px;
    font-size: 14px;
  }
  .pull .bar {
    height: 8px;
    background: #171a21;
    border: 1px solid #262b36;
    border-radius: 5px;
    overflow: hidden;
  }
  .pull .fill {
    height: 100%;
    width: 100%;
    background: #2f6feb;
    transform-origin: left;
    transition: transform 0.3s;
  }
</style>
