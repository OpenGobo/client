<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  interface MonitorInfo {
    index: number;
    name: string;
    width: number;
    height: number;
    x: number;
    y: number;
    is_primary: boolean;
  }

  let screenNumber = $state<number | null>(null);
  let monitors = $state<MonitorInfo[]>([]);
  let selected = $state<number[]>([0]);
  let fullscreen = $state(true);

  function toggleMonitor(index: number) {
    if (selected.includes(index)) {
      selected = selected.filter((i) => i !== index);
    } else {
      selected = [...selected, index];
    }
  }

  function moveMonitor(pos: number, delta: -1 | 1) {
    const next = pos + delta;
    if (next < 0 || next >= selected.length) return;
    const copy = [...selected];
    [copy[pos], copy[next]] = [copy[next], copy[pos]];
    selected = copy;
  }

  async function start() {
    if (selected.length === 0) return;
    await invoke("launch_screens", {
      selectedMonitors: selected,
      fullscreen,
    });
  }

  async function backToLauncher() {
    await invoke("reopen_launcher");
  }

  $effect(() => {
    const params = new URLSearchParams(window.location.search);
    const s = params.get("screen");
    if (s) {
      screenNumber = Number(s);
    } else {
      invoke<MonitorInfo[]>("list_monitors").then((list) => {
        monitors = list;
        if (list.length > 0) {
          selected = [list[0].index];
        }
      });
    }
  });

  // Harden browser/touch behavior so nothing interrupts live show operation
  $effect(() => {
    const blockContextMenu = (e: Event) => e.preventDefault();
    const blockDrag = (e: Event) => e.preventDefault();
    const blockSelection = (e: Event) => {
      const t = e.target;
      if (!(t instanceof HTMLInputElement || t instanceof HTMLTextAreaElement)) {
        e.preventDefault();
      }
    };
    const blockPinchZoomWheel = (e: WheelEvent) => {
      if (e.ctrlKey || e.metaKey) {
        e.preventDefault();
      }
    };
    const blockMultiTouchPinch = (e: TouchEvent) => {
      if (e.touches.length > 1) {
        e.preventDefault();
      }
    };
    const blockBrowserHotkeys = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;
      const key = e.key.toLowerCase();
      if (
        e.key === "F5" ||
        (e.altKey && (e.key === "ArrowLeft" || e.key === "ArrowRight")) ||
        (mod && ["r", "f", "p", "s", "u", "w", "+", "-", "=", "0"].includes(key))
      ) {
        e.preventDefault();
      }
    };

    document.addEventListener("contextmenu", blockContextMenu);
    document.addEventListener("dragstart", blockDrag);
    document.addEventListener("selectstart", blockSelection);
    document.addEventListener("gesturestart", blockDrag);
    document.addEventListener("gesturechange", blockDrag);
    document.addEventListener("wheel", blockPinchZoomWheel, { passive: false });
    document.addEventListener("touchstart", blockMultiTouchPinch, { passive: false });
    document.addEventListener("touchmove", blockMultiTouchPinch, { passive: false });
    window.addEventListener("keydown", blockBrowserHotkeys);

    return () => {
      document.removeEventListener("contextmenu", blockContextMenu);
      document.removeEventListener("dragstart", blockDrag);
      document.removeEventListener("selectstart", blockSelection);
      document.removeEventListener("gesturestart", blockDrag);
      document.removeEventListener("gesturechange", blockDrag);
      document.removeEventListener("wheel", blockPinchZoomWheel);
      document.removeEventListener("touchstart", blockMultiTouchPinch);
      document.removeEventListener("touchmove", blockMultiTouchPinch);
      window.removeEventListener("keydown", blockBrowserHotkeys);
    };
  });
</script>

{#if screenNumber !== null}
  <main style="padding: 16px;">
    <h2>Screen {screenNumber}</h2>
    <button onpointerdown={backToLauncher}>Screen Setup</button>
  </main>
{:else}
  <main style="padding: 16px;">
    <h3>Select Screens</h3>

    {#each monitors as m (m.index)}
      {@const slot = selected.indexOf(m.index)}
      <div>
        <label>
          <input
            type="checkbox"
            checked={slot !== -1}
            onchange={() => toggleMonitor(m.index)}
          />
          {#if slot !== -1}
            <strong>[Screen {slot + 1}]</strong>
          {/if}
          {m.name} ({m.width}x{m.height})
        </label>
        {#if slot !== -1 && selected.length > 1}
          <button type="button" disabled={slot === 0} onclick={() => moveMonitor(slot, -1)}>
            ↑
          </button>
          <button
            type="button"
            disabled={slot === selected.length - 1}
            onclick={() => moveMonitor(slot, 1)}
          >
            ↓
          </button>
        {/if}
      </div>
    {/each}

    <div style="margin-top: 12px;">
      <label>
        <input type="checkbox" bind:checked={fullscreen} />
        Fullscreen
      </label>
    </div>

    <div style="margin-top: 16px;">
      <button disabled={selected.length === 0} onclick={start}>
        Start ({selected.length})
      </button>
    </div>
  </main>
{/if}

<style>
  :global(html, body) {
    overscroll-behavior: none;
    touch-action: manipulation;
  }

  :global(*) {
    user-select: none;
    -webkit-user-select: none;
    -webkit-touch-callout: none;
    -webkit-user-drag: none;
  }

  :global(input, textarea) {
    user-select: text;
    -webkit-user-select: text;
  }
</style>
