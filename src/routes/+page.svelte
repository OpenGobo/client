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
  <main class="h-screen w-screen bg-bg p-4 text-text">
    <div class="flex items-center justify-between border-b border-border pb-3">
      <span class="text-sm font-medium text-muted">Screen {screenNumber}</span>
      <button
        type="button"
        class="rounded border border-border bg-surface px-3 py-1.5 text-sm text-text active:bg-surface-2"
        onpointerdown={backToLauncher}
      >
        Screen Setup
      </button>
    </div>
  </main>
{:else}
  <main class="flex min-h-screen flex-col justify-between bg-bg p-4 text-sm text-text">
    <div class="space-y-3">
      <h1 class="text-sm font-semibold text-text">Select Screens</h1>

      <div class="space-y-1.5">
        {#each monitors as m (m.index)}
          {@const slot = selected.indexOf(m.index)}
          <div class="flex items-center justify-between gap-2 rounded border border-border bg-surface px-2.5 py-2">
            <label class="flex flex-1 items-center gap-2">
              <input
                type="checkbox"
                checked={slot !== -1}
                onchange={() => toggleMonitor(m.index)}
              />
              {#if slot !== -1}
                <span class="rounded bg-surface-2 px-1.5 py-0.5 text-xs text-accent">
                  #{slot + 1}
                </span>
              {/if}
              <span>{m.name}</span>
              <span class="text-xs text-muted">{m.width}×{m.height}</span>
            </label>

            {#if slot !== -1 && selected.length > 1}
              <div class="flex gap-1">
                <button
                  type="button"
                  class="rounded border border-border bg-surface-2 px-2 py-0.5 text-xs disabled:opacity-30"
                  disabled={slot === 0}
                  onclick={() => moveMonitor(slot, -1)}
                >
                  ↑
                </button>
                <button
                  type="button"
                  class="rounded border border-border bg-surface-2 px-2 py-0.5 text-xs disabled:opacity-30"
                  disabled={slot === selected.length - 1}
                  onclick={() => moveMonitor(slot, 1)}
                >
                  ↓
                </button>
              </div>
            {/if}
          </div>
        {/each}
      </div>

      <label class="flex items-center gap-2 pt-1 text-muted">
        <input type="checkbox" bind:checked={fullscreen} />
        Fullscreen
      </label>
    </div>

    <button
      type="button"
      class="mt-4 w-full rounded border border-border bg-surface-2 py-2 text-sm font-medium text-text active:bg-surface disabled:opacity-40"
      disabled={selected.length === 0}
      onclick={start}
    >
      Start ({selected.length})
    </button>
  </main>
{/if}
