<script lang="ts">
  // Loading / error / empty placeholder for views that fetch once (P9 empty and error states).
  interface Props {
    /** What is being read, for the messages: "drives", "system information". */
    what: string;
    error: string | null;
    /** Shows the loading line when there is no error. */
    loading?: boolean;
    /** Shown when neither loading nor failed (nothing found). */
    empty?: string | null;
    retry?: () => void;
  }
  let { what, error, loading = false, empty = null, retry }: Props = $props();
</script>

{#if error}
  <div class="state error" role="alert">
    <p>
      <b>Could not read {what}.</b>
      <span>{error}</span>
    </p>
    {#if retry}<button type="button" onclick={retry}>Try again</button>{/if}
  </div>
{:else if loading}
  <p class="state" role="status" aria-live="polite">Reading {what}…</p>
{:else if empty}
  <div class="state" role="status">
    <p>{empty}</p>
    {#if retry}<button type="button" onclick={retry}>Check again</button>{/if}
  </div>
{/if}

<style>
  .state {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.6rem 1rem;
    margin: 0 0 0.9rem;
    padding: 0.7rem 0.9rem;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--panel);
    color: var(--muted);
    font-size: 0.85rem;
  }
  .error {
    border-color: color-mix(in srgb, var(--hot) 50%, var(--line));
  }
  p {
    flex: 1;
    min-width: 12rem;
    margin: 0;
  }
  .error b {
    display: block;
    color: var(--hot);
  }
  .error span {
    overflow-wrap: anywhere;
  }
  button {
    padding: 0.3rem 0.8rem;
    border: 1px solid var(--line);
    border-radius: 7px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 0.8rem;
    cursor: pointer;
  }
  button:hover {
    border-color: var(--muted);
  }
</style>
