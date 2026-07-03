<script lang="ts">
  import { onMount } from "svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { api, type CaptureRow, type CoreEvent } from "$lib/api";

  const PAGE_SIZE = 60;

  let captures = $state<CaptureRow[]>([]);
  let lastError = $state<string | null>(null);
  let deliveryError = $state<string | null>(null);
  let lastDelivery = $state<string | null>(null);
  let searchQuery = $state("");
  let searchResults = $state<CaptureRow[] | null>(null);
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  let loadingMore = $state(false);
  let reachedEnd = $state(false);
  let capturing = $state(false);

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
        captures = captures.map((c) =>
          c.id === core.data.capture_id ? { ...c, description: core.data.description } : c
        );
      } else if (core.type === "delivery_failed") {
        deliveryError = `${core.data.sink}: ${core.data.message}`;
      } else if (core.type === "delivery_succeeded") {
        deliveryError = null;
        lastDelivery = `Sent ${core.data.count} capture${core.data.count > 1 ? "s" : ""} via ${core.data.sink}`;
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
        <figure class="card" title={row.path}>
          <img
            src={convertFileSrc(row.path)}
            alt={`Screenshot at ${timeOf(row)}`}
            loading="lazy"
          />
          {#if row.description}
            <p class="snippet">{row.description}</p>
          {/if}
          <figcaption>
            <span>{dayOf(row)} {timeOf(row)}</span>
          </figcaption>
        </figure>
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
          <figure class="card" title={row.path}>
            <img
              src={convertFileSrc(row.path)}
              alt={`Screenshot at ${timeOf(row)}`}
              loading="lazy"
            />
            {#if row.description}
              <p class="snippet">{row.description}</p>
            {/if}
            <figcaption>
              <span>{timeOf(row)}</span>
              <span class="dim">{row.width}×{row.height}</span>
            </figcaption>
          </figure>
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
    background: #171a21;
    border: 1px solid #262b36;
    border-radius: 10px;
    overflow: hidden;
  }
  .card img {
    display: block;
    width: 100%;
    aspect-ratio: 16 / 9;
    object-fit: cover;
    background: #0c0e12;
  }
  figcaption {
    display: flex;
    justify-content: space-between;
    padding: 7px 10px;
    font-size: 12px;
  }
  .dim {
    color: #7c8598;
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
