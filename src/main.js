let activeId = null;
let settingsOpen = false;
let services = [];
let contextServiceId = null;

function getInvoke() {
  return window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke;
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
    const serviceList = document.getElementById("service-list");
    services = await invoke("get_services");

    // Show empty state if no services
    if (services.length === 0) {
      document.getElementById("empty-state").classList.remove("hidden");
    }

    services.forEach((service, index) => {
      const btn = document.createElement("div");
      btn.className = "service-icon";
      btn.textContent = service.icon;
      btn.title = service.name + (index < 9 ? " (Ctrl+" + (index + 1) + ")" : "");
      btn.dataset.id = service.id;
      btn.addEventListener("click", () => switchService(service.id));
      btn.addEventListener("contextmenu", (e) => showContextMenu(e, service.id));
      serviceList.appendChild(btn);
    });

    // Settings button
    document.getElementById("settings-btn").addEventListener("click", openSettings);

    // Keyboard shortcuts
    document.addEventListener("keydown", handleKeyboard);

    // Close context menu on click
    document.addEventListener("click", hideContextMenu);

    // Context menu actions
    document.querySelectorAll(".ctx-item").forEach((item) => {
      item.addEventListener("click", handleContextAction);
    });

    // Restore last active service
    const lastActive = await invoke("get_last_active_service");
    if (lastActive) {
      await switchService(lastActive);
    } else if (services.length > 0) {
      await switchService(services[0].id);
    }
  } catch (err) {
    document.body.style.color = "red";
    document.body.innerHTML += "<pre>Init error: " + err + "</pre>";
  }
}

async function switchService(id) {
  const invoke = getInvoke();
  if (!invoke) return;

  // Show loading spinner
  const overlay = document.getElementById("loading-overlay");
  overlay.classList.remove("hidden");

  try {
    await invoke("switch_service", { id });
    activeId = id;
    settingsOpen = false;
    updateActiveState();
  } catch (err) {
    console.error("Switch error:", err);
  }

  // Hide loading after a short delay (webview takes a moment to show)
  setTimeout(() => overlay.classList.add("hidden"), 500);
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

  // Ctrl+1-9 = switch service
  const num = parseInt(e.key);
  if (num >= 1 && num <= 9 && num <= services.length) {
    e.preventDefault();
    switchService(services[num - 1].id);
  }
}

// Context menu
function showContextMenu(e, serviceId) {
  e.preventDefault();
  contextServiceId = serviceId;
  const menu = document.getElementById("context-menu");
  menu.style.left = e.clientX + "px";
  menu.style.top = e.clientY + "px";
  menu.classList.remove("hidden");
}

function hideContextMenu() {
  document.getElementById("context-menu").classList.add("hidden");
  contextServiceId = null;
}

async function handleContextAction(e) {
  const action = e.target.dataset.action;
  const invoke = getInvoke();
  if (!invoke || !contextServiceId) return;

  if (action === "reload") {
    await invoke("reload_service", { id: contextServiceId });
  } else if (action === "open-browser") {
    const url = await invoke("get_service_url", { id: contextServiceId });
    if (url && window.__TAURI__ && window.__TAURI__.opener) {
      window.__TAURI__.opener.openUrl(url);
    }
  }
  hideContextMenu();
}

// Badge update callback (called from Rust via eval)
window.__updateBadges = function(badges) {
  document.querySelectorAll(".service-icon").forEach((btn) => {
    const id = btn.dataset.id;
    // Remove existing badge
    const existing = btn.querySelector(".badge");
    if (existing) existing.remove();

    const count = badges[id];
    if (count && count > 0) {
      const badge = document.createElement("span");
      badge.className = "badge";
      badge.textContent = count > 99 ? "99+" : count;
      btn.appendChild(badge);
    }
  });
};

document.addEventListener("DOMContentLoaded", init);
