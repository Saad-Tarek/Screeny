<script lang="ts">
  import { onMount } from "svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { api, type Analysis, type CaptureRow, type CoreEvent } from "$lib/api";

  const PAGE_SIZE = 60;

  let captures = $state<CaptureRow[]>([]);
  let lastError = $state<string | null>(null);
  let deliveryError = $state<string | null>(null);
  let lastDelivery = $state<string | null>(null);
  let analysisError = $state<string | null>(null);
  let searchQuery = $state("");
  let searchResults = $state<CaptureRow[] | null>(null);
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  let loadingMore = $state(false);
  let reachedEnd = $state(false);
  let capturing = $state(false);
  let selected = $state<CaptureRow | null>(null);
  let selectedAnalysis = $state<Analysis | null>(null);
  let detailError = $state<string | null>(null);
  let detailLoading = $state(false);

  async function openCapture(row: CaptureRow) {
    selected = row;
    selectedAnalysis = null;
    detailError = null;
    detailLoading = true;
    try {
      selectedAnalysis = await api.getAnalysis(row.id);
    } catch (e) {
      detailError = String(e);
    } finally {
      detailLoading = false;
    }
  }

  function closeCapture() {
    selected = null;
    selectedAnalysis = null;
  }

  function timeOf(row: CaptureRow): string {
    return new Date(row.taken_at).toLocaleTimeString();
  }

  function dayOf(row: CaptureRow): string {
    return new Date(row.taken_at).toLocaleDateString(undefined, {
      weekday: "long",
      year: "numeric",
      month: "long",
      day: "numeric",
    });
  }

  function badges(row: CaptureRow): { sink: string; ok: boolean }[] {
    if (!row.delivery_summary) return [];
    return row.delivery_summary.split(",").map((entry) => {
      const [sink, status] = entry.split(":");
      return { sink, ok: status === "sent" };
    });
  }

  function patchDeliveries(ids: number[], sink: string, status: string) {
    const patch = (row: CaptureRow): CaptureRow => {
      if (!ids.includes(row.id)) return row;
      const rest = (row.delivery_summary ?? "")
        .split(",")
        .filter((e) => e && !e.startsWith(`${sink}:`));
      return { ...row, delivery_summary: [...rest, `${sink}:${status}`].join(",") };
    };
    captures = captures.map(patch);
    if (searchResults) searchResults = searchResults.map(patch);
  }

  /** Group consecutive captures (already newest-first) by calendar day. */
  function grouped(rows: CaptureRow[]): { day: string; rows: CaptureRow[] }[] {
    const groups: { day: string; rows: CaptureRow[] }[] = [];
    for (const row of rows) {
      const day = dayOf(row);
      const last = groups[groups.length - 1];
      if (last && last.day === day) last.rows.push(row);
      else groups.push({ day, rows: [row] });
    }
    return groups;
  }

  async function loadMore() {
    if (loadingMore || reachedEnd) return;
    loadingMore = true;
    try {
      const before = captures.length ? captures[captures.length - 1].id : undefined;
      const page = await api.listCaptures(PAGE_SIZE, before);
      captures = [...captures, ...page];
      reachedEnd = page.length < PAGE_SIZE;
    } catch (e) {
      lastError = String(e);
    } finally {
      loadingMore = false;
    }
  }

  function onSearchInput() {
    clearTimeout(searchTimer);
    const query = searchQuery.trim();
    if (!query) {
      searchResults = null;
      return;
    }
    searchTimer = setTimeout(async () => {
      try {
        searchResults = await api.searchCaptures(query);
      } catch (e) {
        lastError = String(e);
      }
    }, 250);
  }

  async function captureNow() {
    capturing = true;
    lastError = null;
    try {
      await api.captureNow();
    } catch (e) {
      lastError = String(e);
    } finally {
      capturing = false;
    }
  }

  onMount(() => {
    loadMore();
    let unlisten: UnlistenFn | undefined;
    listen<CoreEvent>("core-event", (event) => {
      const core = event.payload;
      if (core.type === "capture_taken") {
        captures = [core.data, ...captures];
        lastError = null;
      } else if (core.type === "capture_failed") {
        lastError = core.data.message;
      } else if (core.type === "analysis_completed") {
        analysisError = null;
        captures = captures.map((c) =>
          c.id === core.data.capture_id ? { ...c, description: core.data.description } : c
        );
      } else if (core.type === "analysis_failed") {
        analysisError = core.data.message;
      } else if (core.type === "delivery_failed") {
        deliveryError = `${core.data.sink}: ${core.data.message}`;
        patchDeliveries(core.data.capture_ids, core.data.sink, "failed");
      } else if (core.type === "delivery_succeeded") {
        deliveryError = null;
        lastDelivery = `Sent ${core.data.count} capture${core.data.count > 1 ? "s" : ""} via ${core.data.sink}`;
        patchDeliveries(core.data.capture_ids, core.data.sink, "sent");
      }
    }).then((fn) => (unlisten = fn));
    return () => unlisten?.();
  });
</script>

<header class="page-head">
  <h1>Dashboard</h1>
  <input
    class="search"
    type="search"
    placeholder="Search on-screen text and descriptions…"
    bind:value={searchQuery}
    oninput={onSearchInput}
  />
  <button class="primary" onclick={captureNow} disabled={capturing}>
    {capturing ? "Capturing…" : "Capture now"}
  </button>
</header>

{#if lastError}
  <div class="error">
    Capture problem: {lastError}
  </div>
{/if}
{#if analysisError}
  <div class="error">
    AI analysis is failing (captures still work): {analysisError}
    <br /><small>
      Check Settings → AI analysis — the selected model must be a
      <strong>vision</strong> model (e.g. Qwen2.5-VL, LLaVA, Moondream).
      Use "Analyze latest capture" there to verify.
    </small>
  </div>
{/if}
{#if deliveryError}
  <div class="error">
    Delivery problem — captures stay safe locally. {deliveryError}
  </div>
{:else if lastDelivery}
  <div class="notice">{lastDelivery}</div>
{/if}

{#if searchResults !== null}
  <h2 class="day">
    {searchResults.length} result{searchResults.length === 1 ? "" : "s"} for “{searchQuery.trim()}”
  </h2>
  {#if searchResults.length === 0}
    <div class="empty">
      <p>Nothing found.</p>
      <p class="hint">
        Search covers AI-extracted text — captures taken before enabling AI
        analysis aren't indexed.
      </p>
    </div>
  {:else}
    <div class="grid">
      {#each searchResults as row (row.id)}
        <button class="card" title={row.path} onclick={() => openCapture(row)}>
          <img
            src={convertFileSrc(row.path)}
            alt={`Screenshot at ${timeOf(row)}`}
            loading="lazy"
          />
          {#if row.description}
            <p class="snippet">{row.description}</p>
          {/if}
          <div class="caption">
            <span>{dayOf(row)} {timeOf(row)}</span>
          </div>
        </button>
      {/each}
    </div>
  {/if}
{:else if captures.length === 0 && !loadingMore}
  <div class="empty">
    <p>No captures yet.</p>
    <p class="hint">
      Screeny will take its first screenshot after one capture interval, or
      press <strong>Capture now</strong>.
    </p>
  </div>
{:else}
  {#each grouped(captures) as group (group.day)}
    <section>
      <h2 class="day">{group.day}</h2>
      <div class="grid">
        {#each group.rows as row (row.id)}
          <button class="card" title={row.path} onclick={() => openCapture(row)}>
            <img
              src={convertFileSrc(row.path)}
              alt={`Screenshot at ${timeOf(row)}`}
              loading="lazy"
            />
            {#if row.description}
              <p class="snippet">{row.description}</p>
            {/if}
            <div class="caption">
              <span>{timeOf(row)}</span>
              <span class="badges">
                {#each badges(row) as badge (badge.sink)}
                  <span class="badge" class:ok={badge.ok} title={`${badge.sink}: ${badge.ok ? "sent" : "failed"}`}>
                    {badge.sink === "email" ? "✉" : "✈"}{badge.ok ? "" : "!"}
                  </span>
                {/each}
                <span class="dim">{row.width}×{row.height}</span>
              </span>
            </div>
          </button>
        {/each}
      </div>
    </section>
  {/each}
  {#if !reachedEnd}
    <button class="load-more" onclick={loadMore} disabled={loadingMore}>
      {loadingMore ? "Loading…" : "Load older captures"}
    </button>
  {/if}
{/if}

<svelte:window onkeydown={(e) => e.key === "Escape" && closeCapture()} />

{#if selected}
  <div class="overlay">
    <button class="backdrop" aria-label="Close capture details" onclick={closeCapture}></button>
    <div class="detail" role="dialog" aria-modal="true" aria-label="Capture details">
      <header class="detail-head">
        <div>
          <strong>{dayOf(selected)} {timeOf(selected)}</strong>
          <span class="dim">
            — {selected.monitor}, {selected.width}×{selected.height}
          </span>
        </div>
        <button class="close" onclick={closeCapture} aria-label="Close">✕</button>
      </header>
      <div class="detail-body">
        <img src={convertFileSrc(selected.path)} alt="Full-size capture" />
        {#if detailLoading}
          <p class="dim">Loading analysis…</p>
        {:else if detailError}
          <div class="error">Could not load the analysis: {detailError}</div>
        {:else if selectedAnalysis}
          <section>
            <h3>AI description</h3>
            <p class="detail-description">{selectedAnalysis.description}</p>
          </section>
          {#if selectedAnalysis.ocr_text}
            <section>
              <h3>On-screen text</h3>
              <pre class="ocr">{selectedAnalysis.ocr_text}</pre>
            </section>
          {/if}
          <p class="dim detail-meta">
            Analyzed by {selectedAnalysis.model} in {(selectedAnalysis.latency_ms / 1000).toFixed(1)}s
          </p>
        {:else}
          <p class="dim">No AI analysis for this capture.</p>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .page-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 16px;
  }
  .search {
    flex: 1;
    max-width: 420px;
    background: #10131a;
    border: 1px solid #2c3342;
    color: #e6e8ee;
    border-radius: 8px;
    padding: 8px 12px;
    font-size: 14px;
  }
  .search::placeholder {
    color: #6b7385;
  }
  .snippet {
    margin: 0;
    padding: 0 10px 9px;
    font-size: 12px;
    line-height: 1.45;
    color: #97a0b4;
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  h1 {
    font-size: 20px;
    margin: 0;
  }
  .primary {
    background: #2f6feb;
    color: white;
    border: none;
    border-radius: 8px;
    padding: 8px 16px;
    font-size: 14px;
    cursor: pointer;
  }
  .primary:hover:not(:disabled) {
    background: #3d7cf5;
  }
  .primary:disabled {
    opacity: 0.6;
    cursor: default;
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
  .notice {
    background: #16281d;
    border: 1px solid #2b5a3c;
    color: #a3dcb6;
    border-radius: 8px;
    padding: 10px 14px;
    margin-bottom: 16px;
    font-size: 14px;
  }
  .empty {
    text-align: center;
    color: #8b93a7;
    margin-top: 80px;
  }
  .empty .hint {
    font-size: 13px;
  }
  .day {
    font-size: 13px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: #8b93a7;
    margin: 20px 0 10px;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: 12px;
  }
  .card {
    margin: 0;
    padding: 0;
    display: block;
    width: 100%;
    text-align: left;
    font: inherit;
    color: inherit;
    cursor: pointer;
    background: #171a21;
    border: 1px solid #262b36;
    border-radius: 10px;
    overflow: hidden;
  }
  .card:hover {
    border-color: #39456b;
  }
  .card img {
    display: block;
    width: 100%;
    aspect-ratio: 16 / 9;
    object-fit: cover;
    background: #0c0e12;
  }
  .caption {
    display: flex;
    justify-content: space-between;
    padding: 7px 10px;
    font-size: 12px;
  }
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 10;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 32px;
  }
  .backdrop {
    position: absolute;
    inset: 0;
    background: #000000aa;
    border: none;
    cursor: default;
  }
  .detail {
    position: relative;
    background: #171a21;
    border: 1px solid #2c3342;
    border-radius: 12px;
    width: min(860px, 100%);
    max-height: 100%;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .detail-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 12px 16px;
    border-bottom: 1px solid #262b36;
    font-size: 14px;
  }
  .close {
    background: none;
    border: none;
    color: #aab2c3;
    font-size: 15px;
    padding: 4px 8px;
    border-radius: 6px;
    cursor: pointer;
  }
  .close:hover {
    background: #1f2430;
    color: #e6e8ee;
  }
  .detail-body {
    padding: 16px;
    overflow-y: auto;
  }
  .detail-body img {
    display: block;
    width: 100%;
    border-radius: 8px;
    background: #0c0e12;
    margin-bottom: 14px;
  }
  .detail-body h3 {
    font-size: 13px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: #8b93a7;
    margin: 14px 0 6px;
  }
  .detail-description {
    margin: 0;
    font-size: 14px;
    line-height: 1.55;
  }
  .ocr {
    margin: 0;
    padding: 12px;
    background: #10131a;
    border: 1px solid #262b36;
    border-radius: 8px;
    font-size: 13px;
    line-height: 1.5;
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 300px;
    overflow-y: auto;
  }
  .detail-meta {
    font-size: 12px;
    margin: 12px 0 0;
  }
  .dim {
    color: #7c8598;
  }
  .badges {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .badge {
    color: #e0a83c;
    font-size: 12px;
  }
  .badge.ok {
    color: #45d483;
  }
  .load-more {
    display: block;
    margin: 24px auto;
    background: #1f2430;
    color: #aab2c3;
    border: 1px solid #2c3342;
    border-radius: 8px;
    padding: 8px 20px;
    cursor: pointer;
  }
  .load-more:hover:not(:disabled) {
    color: #e6e8ee;
  }
</style>
