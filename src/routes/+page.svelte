<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";

  interface MonitorInfo {
    index: number;
    name: string;
    width: number;
    height: number;
    x: number;
    y: number;
    is_primary: boolean;
  }

  interface DiscoveredServer {
    server_id: string;
    host: string;
    port: number;
    show_title: string;
    protocol_version: number;
    setup_mode_required: boolean;
  }

  interface ShowSnapshot {
    show_title: string;
    user: {
      id: string;
      username: string;
      roles: string[];
    };
  }

  interface ClientRuntimeStatus {
    protocol_version: number;
    connected: boolean;
    server_address: string | null;
    server_hello: {
      protocol_version: number;
      show_title: string;
      setup_mode_required: boolean;
    } | null;
    session_token: string | null;
    snapshot: ShowSnapshot | null;
  }

  let screenNumber = $state<number | null>(null);
  let monitors = $state<MonitorInfo[]>([]);
  let selected = $state<number[]>([0]);
  let fullscreen = $state(true);

  let servers = $state<DiscoveredServer[]>([]);
  let selectedServerId = $state<string | null>(null);
  let scanning = $state(false);
  let status = $state<ClientRuntimeStatus>({
    protocol_version: 1,
    connected: false,
    server_address: null,
    server_hello: null,
    session_token: null,
    snapshot: null,
  });
  let errorMsg = $state<string | null>(null);

  let setupPin = $state("");
  let username = $state("");
  let password = $state("");
  let authBusy = $state(false);
  let authError = $state<string | null>(null);

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

  async function scanServers() {
    scanning = true;
    errorMsg = null;
    try {
      const list = await invoke<DiscoveredServer[]>("discover_servers");
      servers = list;
      if (list.length > 0 && !list.some((s) => s.server_id === selectedServerId)) {
        selectedServerId = list[0].server_id;
      }
    } catch (e) {
      errorMsg = String(e);
    } finally {
      scanning = false;
    }
  }

  async function start() {
    if (selected.length === 0) return;
    errorMsg = null;

    const chosenServer = servers.find((s) => s.server_id === selectedServerId);
    if (chosenServer) {
      try {
        status = await invoke<ClientRuntimeStatus>("connect_server", {
          host: chosenServer.host,
          port: chosenServer.port,
        });
      } catch (e) {
        errorMsg = String(e);
        return;
      }
    }

    await invoke("launch_screens", {
      selectedMonitors: selected,
      fullscreen,
    });
  }

  async function submitSetup(e: Event) {
    e.preventDefault();
    authBusy = true;
    authError = null;
    try {
      status = await invoke<ClientRuntimeStatus>("complete_setup", {
        setupPin,
        adminUsername: username,
        adminPassword: password,
      });
      setupPin = "";
      password = "";
    } catch (err) {
      authError = String(err);
    } finally {
      authBusy = false;
    }
  }

  async function submitLogin(e: Event) {
    e.preventDefault();
    authBusy = true;
    authError = null;
    try {
      status = await invoke<ClientRuntimeStatus>("login_password", {
        username,
        password,
      });
      password = "";
    } catch (err) {
      authError = String(err);
    } finally {
      authBusy = false;
    }
  }

  async function handleLogout() {
    status = await invoke<ClientRuntimeStatus>("logout_user");
  }

  async function backToLauncher() {
    await invoke("reopen_launcher");
  }

  $effect(() => {
    const params = new URLSearchParams(window.location.search);
    const s = params.get("screen");
    if (s) {
      screenNumber = Number(s);
      invoke<ClientRuntimeStatus>("get_runtime_status").then((st) => {
        status = st;
      });
    } else {
      invoke<MonitorInfo[]>("list_monitors").then((list) => {
        monitors = list;
        if (list.length > 0) {
          selected = [list[0].index];
        }
      });
      invoke<ClientRuntimeStatus>("get_runtime_status").then((st) => {
        status = st;
      });
      scanServers();
    }

    const unlistenPromise = listen<ClientRuntimeStatus>("runtime-status", (ev) => {
      status = ev.payload;
      authError = null;
    });

    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
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
  <main class="flex h-screen w-screen flex-col bg-bg p-4 text-text">
    <header class="flex items-center justify-between border-b border-border pb-3">
      <div class="flex items-center gap-3 text-sm">
        <span class="font-medium text-muted">Screen {screenNumber}</span>
        {#if status.connected && status.server_hello}
          <span class="rounded bg-surface px-2 py-0.5 text-xs text-ok">
            {status.server_hello.show_title} ({status.server_address})
          </span>
        {:else}
          <span class="rounded bg-surface px-2 py-0.5 text-xs text-muted">Offline</span>
        {/if}
      </div>

      <div class="flex items-center gap-2">
        {#if status.snapshot}
          <span class="text-xs text-muted">{status.snapshot.user.username}</span>
          <button
            type="button"
            class="rounded border border-border bg-surface px-3 py-1.5 text-xs text-text active:bg-surface-2"
            onpointerdown={handleLogout}
          >
            Logout
          </button>
        {/if}
        <button
          type="button"
          class="rounded border border-border bg-surface px-3 py-1.5 text-xs text-text active:bg-surface-2"
          onpointerdown={backToLauncher}
        >
          Setup
        </button>
      </div>
    </header>

    {#if status.connected && status.server_hello && !status.snapshot}
      <div class="flex flex-1 items-center justify-center">
        {#if status.server_hello.setup_mode_required}
          <form
            class="w-full max-w-sm space-y-3 rounded border border-border bg-surface p-5"
            onsubmit={submitSetup}
          >
            <div>
              <h2 class="text-base font-semibold text-text">First-Time Server Setup</h2>
              <p class="mt-0.5 text-xs text-muted">
                Enter the 6-digit Setup PIN shown in the Core server terminal to create the initial
                Admin account.
              </p>
            </div>

            <div class="space-y-1">
              <label for="setup-pin" class="block text-xs text-muted">Setup PIN</label>
              <input
                id="setup-pin"
                type="text"
                inputmode="numeric"
                autocomplete="off"
                placeholder="e.g. 123456"
                bind:value={setupPin}
                class="w-full rounded border border-border bg-bg px-3 py-2 text-sm text-text outline-none focus:border-accent"
              />
            </div>

            <div class="space-y-1">
              <label for="setup-user" class="block text-xs text-muted">Admin Username</label>
              <input
                id="setup-user"
                type="text"
                autocomplete="off"
                placeholder="admin"
                bind:value={username}
                class="w-full rounded border border-border bg-bg px-3 py-2 text-sm text-text outline-none focus:border-accent"
              />
            </div>

            <div class="space-y-1">
              <label for="setup-pass" class="block text-xs text-muted">Admin Password / PIN</label>
              <input
                id="setup-pass"
                type="password"
                autocomplete="off"
                bind:value={password}
                class="w-full rounded border border-border bg-bg px-3 py-2 text-sm text-text outline-none focus:border-accent"
              />
            </div>

            {#if authError}
              <div class="rounded border border-danger bg-bg px-2.5 py-1.5 text-xs text-danger">
                {authError}
              </div>
            {/if}

            <button
              type="submit"
              disabled={authBusy || !setupPin.trim() || !username.trim() || !password}
              class="w-full rounded border border-border bg-surface-2 py-2 text-sm font-medium text-text active:bg-bg disabled:opacity-40"
            >
              {authBusy ? "Creating Admin..." : "Complete Setup"}
            </button>
          </form>
        {:else}
          <form
            class="w-full max-w-sm space-y-3 rounded border border-border bg-surface p-5"
            onsubmit={submitLogin}
          >
            <div>
              <h2 class="text-base font-semibold text-text">Login</h2>
              <p class="mt-0.5 text-xs text-muted">{status.server_hello.show_title}</p>
            </div>

            <div class="space-y-1">
              <label for="login-user" class="block text-xs text-muted">Username</label>
              <input
                id="login-user"
                type="text"
                autocomplete="off"
                bind:value={username}
                class="w-full rounded border border-border bg-bg px-3 py-2 text-sm text-text outline-none focus:border-accent"
              />
            </div>

            <div class="space-y-1">
              <label for="login-pass" class="block text-xs text-muted">Password / PIN</label>
              <input
                id="login-pass"
                type="password"
                autocomplete="off"
                bind:value={password}
                class="w-full rounded border border-border bg-bg px-3 py-2 text-sm text-text outline-none focus:border-accent"
              />
            </div>

            {#if authError}
              <div class="rounded border border-danger bg-bg px-2.5 py-1.5 text-xs text-danger">
                {authError}
              </div>
            {/if}

            <button
              type="submit"
              disabled={authBusy || !username.trim() || !password}
              class="w-full rounded border border-border bg-surface-2 py-2 text-sm font-medium text-text active:bg-bg disabled:opacity-40"
            >
              {authBusy ? "Signing in..." : "Login"}
            </button>
          </form>
        {/if}
      </div>
    {/if}
  </main>
{:else}
  <main class="flex min-h-screen flex-col justify-between bg-bg p-4 text-sm text-text">
    <div class="space-y-4">
      <section class="space-y-2">
        <div class="flex items-center justify-between">
          <h2 class="text-xs font-semibold uppercase tracking-wider text-muted">Core Server</h2>
          <button
            type="button"
            class="rounded border border-border bg-surface px-2 py-0.5 text-xs text-text active:bg-surface-2 disabled:opacity-40"
            disabled={scanning}
            onclick={scanServers}
          >
            {scanning ? "Scanning..." : "Refresh"}
          </button>
        </div>

        {#if servers.length === 0}
          <div class="rounded border border-border bg-surface px-3 py-2 text-xs text-muted">
            {scanning ? "Searching LAN for OpenGobo Core..." : "No Core servers found"}
          </div>
        {:else}
          <div class="space-y-1.5">
            {#each servers as srv (srv.server_id)}
              <button
                type="button"
                class="flex w-full items-center justify-between rounded border px-2.5 py-2 text-left {selectedServerId === srv.server_id ? 'border-accent bg-surface-2' : 'border-border bg-surface'}"
                onclick={() => (selectedServerId = srv.server_id)}
              >
                <div>
                  <div class="font-medium text-text">{srv.show_title}</div>
                  <div class="text-xs text-muted">{srv.host}:{srv.port}</div>
                </div>
                {#if srv.setup_mode_required}
                  <span class="rounded bg-bg px-1.5 py-0.5 text-xs text-accent">Setup</span>
                {/if}
              </button>
            {/each}
          </div>
        {/if}
      </section>

      <section class="space-y-2">
        <h2 class="text-xs font-semibold uppercase tracking-wider text-muted">Screens</h2>

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
      </section>

      {#if errorMsg}
        <div class="rounded border border-danger bg-surface px-2.5 py-1.5 text-xs text-danger">
          {errorMsg}
        </div>
      {/if}
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
