<script lang="ts">
  import Card from './Card.svelte';
  import { gameRows, newRule, pollLabel, POLL_CHOICES, withRule, type GameRow } from '../games';
  import { prefs, profiles, savePrefs } from '../prefs.svelte';
  import { MODES } from '../overlay';
  import type { GameRule, GameSession, OverlayMode, Profile } from '../types';
  import { scrollable } from '../scrollable';

  interface Props {
    sessions: GameSession[] | null;
    /** The game being played (its executable), if any. */
    playing: string | null;
  }
  let { sessions, playing }: Props = $props();

  const rows = $derived(gameRows(prefs.games, sessions ?? [], playing));
  let error = $state<string | null>(null);

  const nameOf = (p: Profile) => profiles.list.find((x) => x.id === p)?.name ?? p;
  const fallback = $derived(prefs.games.defaultProfile ? nameOf(prefs.games.defaultProfile) : 'no change');

  async function change(row: GameRow, patch: Partial<GameRule>): Promise<void> {
    try {
      await savePrefs({ games: withRule(prefs.games, { ...row.rule, name: row.name, ...patch }) });
      error = null;
    } catch (e) {
      error = String(e);
    }
  }

  function setProfile(row: GameRow, v: string): void {
    if (v === 'default') void change(row, { profile: null, keep: false });
    else if (v === 'keep') void change(row, { profile: null, keep: true });
    else void change(row, { profile: v as Profile, keep: false });
  }
  const profileValue = (r: GameRule) => (r.keep ? 'keep' : (r.profile ?? 'default'));
  const overlayValue = (r: GameRule) => (r.overlay === false ? 'off' : (r.overlayMode ?? (r.overlay ? 'on' : 'default')));
  function setOverlay(row: GameRow, v: string): void {
    if (v === 'default') void change(row, { overlay: null, overlayMode: null });
    else if (v === 'off') void change(row, { overlay: false, overlayMode: null });
    else if (v === 'on') void change(row, { overlay: true, overlayMode: null });
    else void change(row, { overlay: true, overlayMode: v as OverlayMode });
  }
</script>

<Card title="Game profiles" subtitle={prefs.games.enabled ? 'On' : 'Off'} wide>
  {#if !prefs.games.enabled}
    <p class="note">Switching profiles for games is off in Settings → Gaming; what you set here applies once it is on.</p>
  {/if}
  {#if rows.length === 0}
    <p class="note">Games appear here once you play them. Each can have its own profile, fans, overlay and update rate.</p>
  {:else}
    <div class="table-wrap" use:scrollable role="region" aria-label="Game profiles">
      <table>
        <thead>
          <tr>
            <th scope="col">Game</th>
            <th scope="col">Profile</th>
            <th scope="col">Fans</th>
            <th scope="col">Overlay</th>
            <th scope="col">Update rate</th>
            <th scope="col"><span class="sr-only">Actions</span></th>
          </tr>
        </thead>
        <tbody>
          {#each rows as row (row.exe)}
            <tr class:ignored={row.rule.ignore}>
              <th scope="row">
                {row.name}
                {#if row.exe === playing?.toLowerCase()}<small class="tag">playing</small>{/if}
              </th>
              {#if row.rule.ignore}
                <td colspan="4" class="muted">Not a game: never detected, switches nothing.</td>
                <td class="actions">
                  <button type="button" onclick={() => void change(row, { ignore: false })}>It's a game</button>
                </td>
              {:else}
                <td data-label="Profile">
                  <select
                    aria-label="Profile for {row.name}"
                    value={profileValue(row.rule)}
                    onchange={(e) => setProfile(row, (e.currentTarget as HTMLSelectElement).value)}
                  >
                    <option value="default">Default ({fallback})</option>
                    {#each profiles.list as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
                    <option value="keep">Don't switch</option>
                  </select>
                </td>
                <td data-label="Fans">
                  <select
                    aria-label="Fan settings for {row.name}"
                    value={row.rule.fans ?? 'follow'}
                    onchange={(e) => {
                      const v = (e.currentTarget as HTMLSelectElement).value;
                      void change(row, { fans: v === 'follow' ? null : (v as Profile) });
                    }}
                  >
                    <option value="follow">The profile's</option>
                    {#each profiles.list as p (p.id)}<option value={p.id}>{p.name}'s</option>{/each}
                  </select>
                </td>
                <td data-label="Overlay">
                  <select
                    aria-label="Overlay for {row.name}"
                    value={overlayValue(row.rule)}
                    onchange={(e) => setOverlay(row, (e.currentTarget as HTMLSelectElement).value)}
                  >
                    <option value="default">Default ({prefs.overlay.enabled ? 'on' : 'off'})</option>
                    <option value="on">On</option>
                    {#each MODES as m (m.id)}<option value={m.id}>On: {m.label}</option>{/each}
                    <option value="off">Off</option>
                  </select>
                </td>
                <td data-label="Update rate">
                  <select
                    aria-label="Update rate for {row.name}"
                    value={row.rule.pollMs == null ? 'profile' : String(row.rule.pollMs)}
                    onchange={(e) => {
                      const v = (e.currentTarget as HTMLSelectElement).value;
                      void change(row, { pollMs: v === 'profile' ? null : Number(v) });
                    }}
                  >
                    <option value="profile">The profile's</option>
                    {#each POLL_CHOICES as ms (ms)}<option value={String(ms)}>Every {pollLabel(ms)}</option>{/each}
                  </select>
                </td>
                <td class="actions">
                  <button type="button" onclick={() => void change(row, { ignore: true })}>Not a game</button>
                  {#if row.saved}
                    <button type="button" onclick={() => void change(row, { ...newRule(row.exe, row.name) })}
                      >Reset</button
                    >
                  {/if}
                </td>
              {/if}
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <p class="note">
      Fans: whose fan settings apply (each profile can have its own in the Cooling tab). Update rate: how often Vyro
      reads sensors and refreshes the overlay while the game runs. A saved game is recognised in a window too.
    </p>
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</Card>

<style>
  .table-wrap {
    /* Positioned, so the visually hidden header stays inside the scroll box. */
    position: relative;
    overflow-x: auto;
    container-type: inline-size;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.82rem;
  }
  th,
  td {
    padding: 0.4rem 0.5rem;
    text-align: left;
    vertical-align: middle;
  }
  thead th {
    color: var(--muted);
    font-size: 0.72rem;
    font-weight: 500;
    white-space: nowrap;
  }
  tbody th,
  tbody td {
    border-top: 1px solid var(--line);
  }
  tbody th {
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .tag {
    margin-left: 0.4rem;
    padding: 0.05rem 0.4rem;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent) 18%, var(--bg));
    font-size: 0.7rem;
    font-weight: 500;
  }
  .ignored th,
  .muted {
    color: var(--muted);
  }
  select {
    max-width: 11rem;
    padding: 0.2rem 0.4rem;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
  }
  .actions {
    white-space: nowrap;
  }
  button {
    padding: 0.25rem 0.6rem;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 0.78rem;
    cursor: pointer;
  }
  button + button {
    margin-left: 0.35rem;
  }
  /* Narrower than the table needs (2.0.2): each game a block, its choices labelled two to a line,
     so the table never scrolls sideways. */
  @container (max-width: 58rem) {
    thead {
      position: absolute;
      width: 1px;
      height: 1px;
      overflow: hidden;
      clip-path: inset(50%);
      white-space: nowrap;
    }
    tbody tr {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 0.45rem 0.75rem;
      padding: 0.6rem 0;
      border-top: 1px solid var(--line);
    }
    tbody th,
    tbody td {
      padding: 0;
      border: 0;
    }
    tbody th,
    td[colspan],
    .actions {
      grid-column: 1 / -1;
    }
    td[data-label]::before {
      content: attr(data-label);
      display: block;
      margin-bottom: 0.15rem;
      color: var(--muted);
      font-size: 0.7rem;
    }
    select {
      width: 100%;
      max-width: none;
    }
  }
  .note {
    margin: 0.6rem 0 0;
    color: var(--muted);
    font-size: 0.8rem;
  }
  .error {
    margin: 0.5rem 0 0;
    color: var(--hot);
    font-size: 0.82rem;
  }
</style>
