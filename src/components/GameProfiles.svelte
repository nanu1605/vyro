<script lang="ts">
  import Card from './Card.svelte';
  import { gameRows, newRule, pollLabel, POLL_CHOICES, withRule, type GameRow } from '../games';
  import { prefs, profiles, savePrefs } from '../prefs.svelte';
  import type { GameRule, GameSession, Profile } from '../types';

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
  const overlayValue = (r: GameRule) => (r.overlay == null ? 'default' : r.overlay ? 'on' : 'off');
</script>

<Card title="Game profiles" subtitle={prefs.games.enabled ? 'On' : 'Off'} wide>
  {#if !prefs.games.enabled}
    <p class="note">Switching profiles for games is off in Settings → Gaming; what you set here applies once it is on.</p>
  {/if}
  {#if rows.length === 0}
    <p class="note">Games appear here once you play them. Each can have its own profile, fans, overlay and update rate.</p>
  {:else}
    <div class="table-wrap">
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
                <td>
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
                <td>
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
                <td>
                  <select
                    aria-label="Overlay for {row.name}"
                    value={overlayValue(row.rule)}
                    onchange={(e) => {
                      const v = (e.currentTarget as HTMLSelectElement).value;
                      void change(row, { overlay: v === 'default' ? null : v === 'on' });
                    }}
                  >
                    <option value="default">Default ({prefs.overlay.enabled ? 'on' : 'off'})</option>
                    <option value="on">On</option>
                    <option value="off">Off</option>
                  </select>
                </td>
                <td>
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
    white-space: nowrap;
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
