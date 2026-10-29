<script lang="ts">
  // Hold-to-confirm kill switch. Holding for `holdMs` fires `onfire`.
  interface Props {
    triggered: boolean;
    disabled?: boolean;
    holdMs?: number;
    onfire: () => void;
  }
  let { triggered, disabled = false, holdMs = 1200, onfire }: Props = $props();

  let progress = $state(0);
  let start = 0;
  let raf = 0;
  let holding = false;

  function begin(): void {
    if (disabled || triggered || holding) return;
    holding = true;
    start = performance.now();
    const step = (now: number): void => {
      if (!holding) return;
      progress = Math.min(1, (now - start) / holdMs);
      if (progress >= 1) {
        holding = false;
        progress = 0;
        onfire();
        return;
      }
      raf = requestAnimationFrame(step);
    };
    raf = requestAnimationFrame(step);
  }

  function end(): void {
    holding = false;
    cancelAnimationFrame(raf);
    progress = 0;
  }

  function keydown(e: KeyboardEvent): void {
    if ((e.key === "Enter" || e.key === " ") && !e.repeat) {
      e.preventDefault();
      begin();
    }
  }
  function keyup(e: KeyboardEvent): void {
    if (e.key === "Enter" || e.key === " ") end();
  }
</script>

<button
  class="kill"
  class:triggered
  {disabled}
  aria-label={triggered ? "Kill switch triggered" : "Hold to trigger the kill switch"}
  onpointerdown={begin}
  onpointerup={end}
  onpointerleave={end}
  onpointercancel={end}
  onkeydown={keydown}
  onkeyup={keyup}
  style:--p={progress}
>
  <span class="fill"></span>
  <span class="text">{triggered ? "KILLED" : progress > 0 ? "HOLD..." : "KILL"}</span>
</button>

<style>
  .kill {
    position: relative;
    overflow: hidden;
    min-width: 120px;
    height: 40px;
    font-size: 15px;
    font-weight: 700;
    letter-spacing: 0.1em;
    color: #fff;
    background: #4a1214;
    border: 2px solid var(--kill);
    user-select: none;
    touch-action: none;
  }
  .kill:hover:not(:disabled) { border-color: #ff6b6f; }
  .kill.triggered { background: var(--kill); cursor: default; }
  .fill {
    position: absolute;
    inset: 0;
    width: calc(var(--p) * 100%);
    background: var(--kill);
  }
  .text { position: relative; }
</style>
