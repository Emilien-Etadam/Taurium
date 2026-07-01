let activeId = null;
let settingsOpen = false;
let services = [];
let pendingServiceId = null;
let overlayHideTimer = null;
let sidebarExpanded = false;

// Per-service load state for the status dot: "idle" | "loading" | "loaded"
const serviceStates = {};

const OVERLAY_FALLBACK_MS = 10000;

import { showToast, formatInvokeError, showServicesLoadInfo } from "./toast.js";

function getInvoke() {
  return window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke;
}

function makeGlyphWrap(service) {
  const wrap = document.createElement("span");
  wrap.className = "glyph-wrap";

  // Support both emoji and image icons
  if (service.icon.startsWith("data:image")) {
    const img = document.createElement("img");
    img.src = service.icon;
    img.className = "icon-img";
    img.alt = "";
    wrap.appendChild(img);
  } else {
    const glyph = document.createElement("span");
    glyph.className = "glyph";
    glyph.textContent = service.icon;
    wrap.appendChild(glyph);
  }

  const dot = document.createElement("span");
  dot.className = "status-dot " + (serviceStates[service.id] || "idle");
  wrap.appendChild(dot);

  return wrap;
}

function renderSidebar(services) {
  const serviceList = document.getElementById("service-list");
  const emptyState = document.getElementById("empty-state");

  serviceList.innerHTML = "";

  if (services.length === 0) {
    emptyState.classList.remove("hidden");
  } else {
    emptyState.classList.add("hidden");
  }

  let lastGroup = null;
  services.forEach((service, index) => {
    // Emit a group header whenever the group changes (order is preserved as stored)
    const group = service.group || null;
    if (group && group !== lastGroup) {
      const header = document.createElement("div");
      header.className = "group-header";
      const line = document.createElement("span");
      line.className = "group-line";
      const text = document.createElement("span");
      text.className = "group-text";
      text.textContent = group;
      header.appendChild(line);
      header.appendChild(text);
      serviceList.appendChild(header);
    }
    lastGroup = group;

    const btn = document.createElement("div");
    btn.className = "service-icon";
    btn.dataset.id = service.id;
    btn.title = service.name + (index < 9 ? " (Ctrl+" + (index + 1) + ")" : "");
    // Keyboard accessibility: focusable, announced, activable with Enter/Space
    btn.setAttribute("role", "button");
    btn.setAttribute("tabindex", "0");
    btn.setAttribute("aria-label", service.name);

    btn.appendChild(makeGlyphWrap(service));

    const label = document.createElement("span");
    label.className = "label";
    label.textContent = service.name;
    btn.appendChild(label);

    btn.addEventListener("click", () => switchService(service.id));
    btn.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        switchService(service.id);
      }
    });
    // Right-click shows native context menu via Tauri
    btn.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      const invoke = getInvoke();
      if (!invoke) return;
      invoke("show_service_context_menu", { id: service.id });
    });

    serviceList.appendChild(btn);
  });

  updateActiveState();
}

async function init() {
  let invoke = getInvoke();
  if (!invoke) {
    await new Promise((resolve) => {
      const check = setInterval(() => {
        invoke = getInvoke();
        if (invoke) { clearInterval(check); resolve(); }
      }, 50);
    });
  }

  try {
    // Load preferences and apply theme
    const prefs = await invoke("get_preferences");
    applyPreferences(prefs);

    // Restore pinned sidebar state (repositions native webviews if expanded)
    if (prefs.sidebar_expanded) {
      await setSidebarExpanded(true);
    }

    // Expand / collapse toggle (click + keyboard)
    const toggleBtn = document.getElementById("sidebar-toggle");
    toggleBtn.addEventListener("click", () => setSidebarExpanded(!sidebarExpanded));
    toggleBtn.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        setSidebarExpanded(!sidebarExpanded);
      }
    });

    services = await invoke("get_services");
    renderSidebar(services);

    const loadInfo = await invoke("get_services_load_info");
    showServicesLoadInfo(loadInfo);

    // Settings button (click + keyboard)
    const settingsBtn = document.getElementById("settings-btn");
    settingsBtn.addEventListener("click", openSettings);
    settingsBtn.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        openSettings();
      }
    });

    // Keyboard shortcuts
    document.addEventListener("keydown", handleKeyboard);

    // Restore last active service
    const lastActive = await invoke("get_last_active_service");
    if (lastActive) {
      await switchService(lastActive);
    } else if (services.length > 0) {
      await switchService(services[0].id);
    }
  } catch (err) {
    showToast("Init error: " + formatInvokeError(err), { durationMs: 10000 });
    console.error("Init error:", err);
  }
}

function applyPreferences(prefs) {
  const root = document.documentElement;
  root.style.setProperty("--icon-size", prefs.icon_size + "px");
  root.style.setProperty("--sidebar-color", prefs.sidebar_color);
  root.style.setProperty("--accent-color", prefs.accent_color);
}

// Called from settings webview after prefs change (via Rust eval)
window.__applyPreferences = function(prefs) {
  applyPreferences(prefs);
};

// Expand / collapse the sidebar. The Rust side reflows the native service
// webviews to start after the sidebar and persists the pinned state.
async function setSidebarExpanded(expanded) {
  const invoke = getInvoke();
  if (!invoke) return;

  sidebarExpanded = expanded;
  document.getElementById("sidebar").classList.toggle("expanded", expanded);
  const toggle = document.getElementById("sidebar-toggle");
  if (toggle) toggle.setAttribute("aria-expanded", String(expanded));

  try {
    await invoke("set_sidebar_expanded", { expanded });
  } catch (err) {
    showToast("Could not resize sidebar: " + formatInvokeError(err));
    console.error("Sidebar resize error:", err);
  }
}

// Update the status dot of a single service without rebuilding the sidebar
function setServiceState(id, state) {
  serviceStates[id] = state;
  document.querySelectorAll(".service-icon").forEach((btn) => {
    if (btn.dataset.id === id) {
      const dot = btn.querySelector(".status-dot");
      if (dot) dot.className = "status-dot " + state;
    }
  });
}

function showLoadingOverlay() {
  const overlay = document.getElementById("loading-overlay");
  if (!overlay) return;

  if (overlayHideTimer) {
    clearTimeout(overlayHideTimer);
    overlayHideTimer = null;
  }

  overlay.classList.remove("hidden");
}

function hideLoadingOverlay() {
  const overlay = document.getElementById("loading-overlay");
  if (!overlay) return;

  overlay.classList.add("hidden");

  if (overlayHideTimer) {
    clearTimeout(overlayHideTimer);
    overlayHideTimer = null;
  }

  pendingServiceId = null;
}

// Called from Rust when a service webview reports a real document title (loaded)
window.__serviceLoaded = function(id) {
  // Any service that reports a title is considered loaded (incl. background ones)
  setServiceState(id, "loaded");

  if (id === pendingServiceId) {
    hideLoadingOverlay();
  }
};

async function switchService(id) {
  const invoke = getInvoke();
  if (!invoke) return;

  pendingServiceId = id;
  showLoadingOverlay();
  if (serviceStates[id] !== "loaded") {
    setServiceState(id, "loading");
  }

  try {
    await invoke("switch_service", { id });
    activeId = id;
    settingsOpen = false;
    updateActiveState();
    overlayHideTimer = setTimeout(hideLoadingOverlay, OVERLAY_FALLBACK_MS);
  } catch (err) {
    if (serviceStates[id] !== "loaded") {
      setServiceState(id, "idle");
    }
    showToast("Could not switch service: " + formatInvokeError(err));
    console.error("Switch error:", err);
    hideLoadingOverlay();
  }
}

async function openSettings() {
  const invoke = getInvoke();
  if (!invoke) return;
  try {
    await invoke("open_settings");
    activeId = null;
    settingsOpen = true;
    updateActiveState();
  } catch (err) {
    showToast("Could not open settings: " + formatInvokeError(err));
    console.error("Settings error:", err);
  }
}

function updateActiveState() {
  document.querySelectorAll(".service-icon").forEach((btn) => {
    btn.classList.toggle("active", btn.dataset.id === activeId);
  });
  document.getElementById("settings-btn").classList.toggle("active", settingsOpen);
}

// Keyboard shortcuts (Ctrl+1-9, Ctrl+,)
function handleKeyboard(e) {
  if (!e.ctrlKey && !e.metaKey) return;

  // Ctrl+, = settings
  if (e.key === ",") {
    e.preventDefault();
    openSettings();
    return;
  }

  // Ctrl+B = toggle sidebar expand/collapse
  if (e.key === "b" || e.key === "B") {
    e.preventDefault();
    setSidebarExpanded(!sidebarExpanded);
    return;
  }

  // Ctrl+1-9 = switch service
  const num = parseInt(e.key);
  if (num >= 1 && num <= 9 && num <= services.length) {
    e.preventDefault();
    switchService(services[num - 1].id);
  }
}

// Reload sidebar when services change (called from Rust via eval)
window.__reloadSidebar = async function() {
  const invoke = getInvoke();
  if (!invoke) return;

  try {
    services = await invoke("get_services");
    renderSidebar(services);

    // Re-apply badge counts
    const badges = await invoke("get_badge_counts");
    window.__updateBadges(badges);
  } catch (err) {
    showToast("Could not reload sidebar: " + formatInvokeError(err));
    console.error("Reload sidebar error:", err);
  }
};

// Badge update callback (called from Rust via eval)
window.__updateBadges = function(badges) {
  document.querySelectorAll(".service-icon").forEach((btn) => {
    const id = btn.dataset.id;
    const wrap = btn.querySelector(".glyph-wrap");
    if (!wrap) return;

    // Remove existing badge
    const existing = wrap.querySelector(".badge");
    if (existing) existing.remove();

    const count = badges[id];
    if (count && count > 0) {
      const badge = document.createElement("span");
      badge.className = "badge";
      badge.textContent = count > 99 ? "99+" : count;
      wrap.appendChild(badge);
    }
  });
};

document.addEventListener("DOMContentLoaded", init);
