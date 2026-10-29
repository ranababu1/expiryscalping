<script lang="ts">
  import { engine } from "../engine.svelte";
  import { label } from "../format";

  const st = $derived(engine.snap.strategies);
  const killed = $derived(engine.snap.status?.kill.triggered ?? false);
</script>

{#if !st}
  <div class="empty">Waiting for strategies...</div>
{:else}
  <div class="bar">
    <span>AI is <strong class={st.ai_paused ? "warn" : "up"}>{st.ai_paused ? "paused" : "running"}</strong></span>
    <button onclick={() => engine.send({ type: "pause_ai", paused: !st.ai_paused }, st.ai_paused ? "AI resumed" : "AI paused")}>
      {st.ai_paused ? "Resume AI" : "Pause AI"}
    </button>
    <span class="muted">Pausing stops new entries. Open positions and exits keep running.</span>
  </div>

  <div class="panel">
    {#if st.strategies.length === 0}
      <div class="empty">No strategies configured.</div>
    {:else}
      <table>
        <thead><tr><th>Strategy</th><th>Status</th><th>Description</th><th class="num">Enabled</th></tr></thead>
        <tbody>
          {#each st.strategies as s (s.id)}
            <tr>
              <td class="mono">{s.id}</td>
              <td><span class="badge {s.enabled ? 'ok' : ''}">{label(s.status)}</span></td>
              <td class="muted desc">{s.description}</td>
              <td class="num">
                <button
                  class="small {s.enabled ? 'danger' : 'primary'}"
                  disabled={killed && !s.enabled}
                  title={killed && !s.enabled ? "Re-arm the kill switch first" : ""}
                  onclick={() => engine.send({ type: "set_strategy", id: s.id, enabled: !s.enabled })}
                >
                  {s.enabled ? "Disable" : "Enable"}
                </button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </div>
{/if}

<style>
  .bar { display: flex; align-items: center; gap: 16px; margin-bottom: 10px; }
  .desc { white-space: normal; }
</style>
